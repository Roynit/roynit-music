package org.roynit.music

import android.content.Context
import android.os.Handler
import android.os.Looper
import org.json.JSONArray
import org.json.JSONObject
import java.io.File
import java.util.UUID
import java.util.concurrent.Executors

class Playlist(var name: String, val songs: ArrayList<Song> = ArrayList(), val id: String = UUID.randomUUID().toString())

/** One play: which song and when (seconds since 1970), the same record the PC app keeps. */
class PlayEvent(val song: Song, val at: Long)

/**
 * What the listener keeps: liked songs, history and playlists. It is small, so it lives in memory and is
 * written to one file a moment after each change. Touch it from the main thread only. With a Roy account it is
 * also kept in step with the cloud library the PC app uses (see Sync).
 */
object Library {
    val favorites = ArrayList<Song>()
    val playlists = ArrayList<Playlist>()

    /** Every play, oldest first. */
    val events = ArrayList<PlayEvent>()

    /** Newest first, each song once, at most 100. */
    val history: List<Song>
        get() {
            val seen = HashSet<String>()
            val out = ArrayList<Song>()
            for (i in events.indices.reversed()) {
                if (seen.add(events[i].song.id)) out += events[i].song
                if (out.size == 100) break
            }
            return out
        }

    /** Called after every change, so the screens can redraw. */
    var onChange: (() -> Unit)? = null

    private var file: File? = null
    private val main = Handler(Looper.getMainLooper())
    private val disk = Executors.newSingleThreadExecutor()
    private val saveSoon = Runnable { write() }

    fun load(context: Context) {
        if (file != null) return
        val f = File(context.applicationContext.filesDir, "library.json")
        file = f
        runCatching {
            if (!f.isFile) return
            val root = JSONObject(f.readText())
            read(root.optJSONArray("favorites"), favorites)
            val lists = root.optJSONArray("playlists") ?: JSONArray()
            for (i in 0 until lists.length()) {
                val o = lists.getJSONObject(i)
                val p = Playlist(o.optString("name"), id = o.optString("id").ifEmpty { UUID.randomUUID().toString() })
                read(o.optJSONArray("songs"), p.songs)
                playlists += p
            }
            val plays = root.optJSONArray("events")
            if (plays != null) {
                for (i in 0 until plays.length()) runCatching {
                    val e = plays.getJSONObject(i)
                    events += PlayEvent(Song.fromJson(e.getJSONObject("song")), e.getLong("at"))
                }
            } else {
                // The first version kept a plain list, newest first.
                val old = ArrayList<Song>()
                read(root.optJSONArray("history"), old)
                val base = System.currentTimeMillis() / 1000 - old.size
                old.asReversed().forEachIndexed { i, s -> events += PlayEvent(s, base + i) }
            }
        }
    }

    private fun read(array: JSONArray?, into: ArrayList<Song>) {
        for (i in 0 until (array?.length() ?: 0)) runCatching { into += Song.fromJson(array!!.getJSONObject(i)) }
    }

    private fun write() {
        val f = file ?: return
        val root = JSONObject()
            .put("favorites", JSONArray(favorites.map { it.toJson() }))
            .put("events", JSONArray(events.map { JSONObject().put("song", it.song.toJson()).put("at", it.at) }))
            .put("playlists", JSONArray(playlists.map { JSONObject().put("id", it.id).put("name", it.name).put("songs", JSONArray(it.songs.map { s -> s.toJson() })) }))
        val text = root.toString()
        disk.execute { runCatching { f.writeText(text) } }
    }

    private fun changed(sync: Boolean = true) {
        main.removeCallbacks(saveSoon)
        main.postDelayed(saveSoon, 600)
        onChange?.invoke()
        if (sync) Sync.later()
    }

    /**
     * A different account signed in: keep what was here in a backup file and start clean, so one person's likes and
     * playlists never reach another person's cloud library.
     */
    fun reset() {
        val f = file
        if (f != null && f.isFile) runCatching { f.copyTo(File(f.parentFile, "library-before-account-switch-${System.currentTimeMillis() / 1000}.json")) }
        favorites.clear()
        playlists.clear()
        events.clear()
        changed(sync = false)
    }

    fun isFavorite(id: String) = favorites.any { it.id == id }

    fun toggleFavorite(song: Song) {
        val at = favorites.indexOfFirst { it.id == song.id }
        if (at >= 0) favorites.removeAt(at) else favorites.add(0, song)
        changed()
    }

    fun played(song: Song) {
        events += PlayEvent(song, System.currentTimeMillis() / 1000)
        while (events.size > 1500) events.removeAt(0)
        changed()
    }

    fun createPlaylist(name: String): Playlist {
        val p = Playlist(name.trim().ifEmpty { "New playlist" })
        playlists += p
        changed()
        return p
    }

    fun addTo(playlist: Playlist, song: Song) {
        if (playlist.songs.none { it.id == song.id }) playlist.songs += song
        changed()
    }

    fun removeFrom(playlist: Playlist, song: Song) {
        playlist.songs.removeAll { it.id == song.id }
        changed()
    }

    fun rename(playlist: Playlist, name: String) {
        if (name.isNotBlank()) playlist.name = name.trim()
        changed()
    }

    fun delete(playlist: Playlist) {
        playlists.remove(playlist)
        changed()
    }

    /**
     * Adds what the cloud library has that this phone does not, the way the PC app does: nothing here is removed.
     * Only YouTube songs are taken, since those are the ones this app can play.
     */
    fun mergeCloud(snapshot: JSONObject) {
        val tracks = snapshot.optJSONObject("tracks") ?: return
        val songs = HashMap<String, Song>()
        for (key in tracks.keys()) {
            val t = tracks.optJSONObject(key) ?: continue
            val platform = t.optString("platform")
            if (platform == "spotify") {
                // A Spotify song plays from the matching recording on YouTube Music.
                val id = key.removePrefix("spotify:")
                songs[key] = Song("sp:$id", t.optString("title"), t.optString("artist"), t.str("cover").ifEmpty { null }, t.optLong("duration"), key, t.str("album"), "spotify")
                continue
            }
            if (platform != "youtube" && platform != "ytmusic") continue
            val video = android.net.Uri.parse(t.optString("source")).getQueryParameter("v") ?: continue
            songs[key] = Song(video, t.optString("title"), t.optString("artist"), t.str("cover").ifEmpty { "https://i.ytimg.com/vi/$video/mqdefault.jpg" }, t.optLong("duration"), key, t.str("album"), platform)
        }
        val favs = snapshot.optJSONArray("favorites")
        for (i in 0 until (favs?.length() ?: 0)) songs[favs!!.optString(i)]?.let { if (!isFavorite(it.id)) favorites += it }
        val lists = snapshot.optJSONObject("playlists")
        for (id in lists?.keys() ?: emptyList<String>().iterator()) {
            val p = lists!!.optJSONObject(id) ?: continue
            val local = playlists.firstOrNull { it.id == id } ?: Playlist(p.optString("name"), id = id).also { playlists += it }
            val ids = p.optJSONArray("tracks")
            for (i in 0 until (ids?.length() ?: 0)) songs[ids!!.optString(i)]?.let { s -> if (local.songs.none { it.id == s.id }) local.songs += s }
        }
        val past = snapshot.optJSONArray("history")
        val have = events.mapTo(HashSet()) { it.song.id to it.at }
        for (i in 0 until (past?.length() ?: 0)) {
            val e = past!!.optJSONObject(i) ?: continue
            val s = songs[e.optString("track_id")] ?: continue
            if (have.add(s.id to e.optLong("started_at"))) events += PlayEvent(s, e.optLong("started_at"))
        }
        events.sortBy { it.at }
        while (events.size > 1500) events.removeAt(0)
        changed(sync = false)
    }
}
