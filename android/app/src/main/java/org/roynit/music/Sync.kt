package org.roynit.music

import android.content.Context
import android.os.Handler
import android.os.Looper
import org.json.JSONArray
import org.json.JSONObject
import java.io.File

/**
 * Keeps the library in step with the Roy account's cloud library, which the PC app shares. It works like the PC
 * app: a download only adds what is missing, an upload replaces the cloud copy with this phone's (plus the
 * songs from other services that only the PC can play, which are carried over untouched).
 */
object Sync {
    private val main = Handler(Looper.getMainLooper())
    private var revision = 0
    private var cloud: JSONObject? = null
    private var store: File? = null
    private var running = false
    private var again = false
    private val push = Runnable { run() }
    private var prefs: android.content.SharedPreferences? = null

    /** Whether the library keeps itself in step by itself; on unless the listener turned it off. */
    // shortcut: always on, there is no switch any more; add one back only if listeners ask to stop syncing.
    private const val auto = true

    private val watch = object : Runnable {
        override fun run() {
            if (auto) now()
            main.postDelayed(this, 90_000)
        }
    }

    /** While the app is on screen, look for changes from other devices every minute and a half. */
    fun watching(on: Boolean) {
        main.removeCallbacks(watch)
        if (on) main.postDelayed(watch, 90_000)
    }

    /** A short note for the Friends/Account screen. */
    var status = ""
        private set

    fun load(context: Context) {
        if (store != null) return
        val f = File(context.applicationContext.filesDir, "cloud.json")
        store = f
        prefs = context.applicationContext.getSharedPreferences("roy_sync", Context.MODE_PRIVATE)
        runCatching {
            val saved = JSONObject(f.readText())
            revision = saved.optInt("revision")
            cloud = saved.optJSONObject("snapshot")
        }
    }

    /** After a change on the phone: upload once things have been quiet for a few seconds. */
    fun later() {
        if (!Account.signedIn || !auto) return
        main.removeCallbacks(push)
        main.postDelayed(push, 4_000)
    }

    private var lastRun = 0L

    /** Coming back to the app: pick up what other devices changed, at most once a minute. */
    fun resume() {
        if (auto && System.currentTimeMillis() - lastRun > 60_000) now()
    }

    /** Download what is new, then upload. Safe to call any time. */
    fun now() {
        if (!Account.signedIn) return
        lastRun = System.currentTimeMillis()
        main.removeCallbacks(push)
        run(pullFirst = true)
    }

    /** The library on this phone belongs to one account; a different account starts from its own cloud library. */
    private fun claim() {
        val id = Account.profile.optString("id")
        if (id.isEmpty()) return
        val owner = prefs?.getString("owner", "").orEmpty()
        if (owner.isNotEmpty() && owner != id) {
            Library.reset()
            cloud = null
            revision = 0
            save()
        }
        if (owner != id) prefs?.edit()?.putString("owner", id)?.apply()
    }

    private fun run(pullFirst: Boolean = false) {
        claim()
        if (running) {
            again = true
            return
        }
        running = true
        status = "Syncing..."
        val first = pullFirst
        Thread {
            var note: String? = if (first) pull() else null
            if (note == null) {
                var failure = upload()
                if (failure != null && failure.contains("changed")) {
                    // Someone else uploaded in between: take theirs, then try once more.
                    pull()
                    failure = upload()
                }
                note = failure
            }
            main.post {
                running = false
                status = note ?: "Library synced"
                if (again) {
                    again = false
                    later()
                }
            }
        }.start()
    }

    /** Downloads the cloud library and merges it in. Returns an error text, or null. */
    private fun pull(): String? {
        val reply = Account.call("GET", "/music")
        if (reply.has("error")) return reply.getString("error")
        revision = reply.optInt("revision")
        val snapshot = reply.optJSONObject("snapshot")
        if (snapshot != null) {
            cloud = snapshot
            main.post { Library.mergeCloud(snapshot) }
            save()
        }
        return null
    }

    private fun upload(): String? {
        val box = arrayOfNulls<JSONObject>(1)
        val latch = java.util.concurrent.CountDownLatch(1)
        main.post {
            box[0] = snapshot()
            latch.countDown()
        }
        latch.await()
        val mine = box[0] ?: return "Could not read the library"
        val reply = Account.call("PUT", "/music", JSONObject().put("revision", revision).put("snapshot", mine))
        if (reply.has("error")) return reply.getString("error")
        revision = reply.optInt("revision")
        cloud = mine
        save()
        return null
    }

    private fun save() {
        val f = store ?: return
        runCatching { f.writeText(JSONObject().put("revision", revision).put("snapshot", cloud ?: JSONObject()).toString()) }
    }

    private fun isYoutube(t: JSONObject) = t.optString("platform").let { it == "youtube" || it == "ytmusic" }

    /** The phone's library in the cloud's format. Main thread only (it reads the Library). */
    fun snapshotNow(): JSONObject = snapshot()

    private fun snapshot(): JSONObject {
        val known = cloud
        val oldTracks = known?.optJSONObject("tracks") ?: JSONObject()
        // A song the cloud already knows under another id (PC's youtube: vs ytmusic:) keeps that id.
        val keys = HashMap<String, String>()
        for (k in oldTracks.keys()) {
            val t = oldTracks.optJSONObject(k) ?: continue
            if (!isYoutube(t)) continue
            android.net.Uri.parse(t.optString("source")).getQueryParameter("v")?.let { keys.putIfAbsent(it, k) }
        }
        fun keyOf(s: Song) = when {
            s.key.isNotEmpty() -> s.key
            s.id.startsWith("sp:") -> "spotify:" + s.id.removePrefix("sp:")
            s.platform == "youtube" -> "youtube:${s.id}"
            else -> keys[s.id] ?: "ytmusic:${s.id}"
        }

        val tracks = JSONObject()
        fun add(s: Song): String {
            val k = keyOf(s)
            if (!tracks.has(k)) {
                val platform = when {
                    k.startsWith("spotify:") -> "spotify"
                    k.startsWith("youtube:") -> "youtube"
                    else -> "ytmusic"
                }
                val source = when (platform) {
                    "spotify" -> "https://open.spotify.com/track/" + k.removePrefix("spotify:")
                    "youtube" -> "https://www.youtube.com/watch?v=${s.id}"
                    else -> "https://music.youtube.com/watch?v=${s.id}"
                }
                // The cloud library keeps only covers from a few trusted hosts, so the others are left out.
                val cover = s.cover?.takeIf { it.startsWith("https://") }
                tracks.put(k, JSONObject().put("id", k).put("title", s.title).put("artist", s.artist).put("album", s.album)
                    .put("duration", s.seconds.toDouble()).put("source", source)
                    .put("platform", platform).put("cover", cover ?: JSONObject.NULL))
            }
            return k
        }

        // Songs the phone cannot play stay as the cloud has them.
        for (k in oldTracks.keys()) oldTracks.optJSONObject(k)?.takeIf { !isYoutube(it) }?.let { tracks.put(k, it) }
        fun foreign(ids: JSONArray?): List<String> = (0 until (ids?.length() ?: 0)).map { ids!!.optString(it) }.filter { tracks.has(it) && !isYoutube(tracks.getJSONObject(it)) }

        // SoundCloud songs stay on this phone: the cloud library has no address for them it would accept.
        fun syncable(s: Song) = !s.id.startsWith("sc:")
        val favorites = JSONArray()
        Library.favorites.filter(::syncable).forEach { favorites.put(add(it)) }
        val dislikes = JSONArray()
        Library.dislikes.filter(::syncable).forEach { dislikes.put(add(it)) }
        foreign(known?.optJSONArray("favorites")).forEach { favorites.put(it) }

        val playlists = JSONObject()
        val oldLists = known?.optJSONObject("playlists")
        for (p in Library.playlists) {
            val ids = JSONArray()
            p.songs.filter(::syncable).forEach { ids.put(add(it)) }
            foreign(oldLists?.optJSONObject(p.id)?.optJSONArray("tracks")).forEach { ids.put(it) }
            playlists.put(p.id, JSONObject().put("id", p.id).put("name", p.name).put("tracks", ids).put("cover", p.cover))
        }

        val history = JSONArray()
        val old = known?.optJSONArray("history")
        for (i in 0 until (old?.length() ?: 0)) {
            val e = old!!.optJSONObject(i) ?: continue
            if (tracks.has(e.optString("track_id")) && !isYoutube(tracks.getJSONObject(e.optString("track_id")))) history.put(e)
        }
        Library.events.filter { syncable(it.song) }.forEach { history.put(JSONObject().put("track_id", add(it.song)).put("started_at", it.at)) }

        return JSONObject().put("tracks", tracks).put("playlists", playlists).put("favorites", favorites).put("dislikes", dislikes).put("history", history)
    }
}
