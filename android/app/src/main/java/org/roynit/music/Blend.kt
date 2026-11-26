package org.roynit.music

import android.app.Activity
import android.app.AlertDialog
import android.net.Uri
import android.widget.Toast
import org.json.JSONObject

/**
 * Blend: a playlist made from your taste and a friend's. Their most-loved songs come from the Roy account server
 * (only when they allow sharing); the mix puts the songs you both have first, then alternates yours and theirs, and
 * finishes with songs like the ones you share. The match is how many of your artists you have in common.
 */
object Blend {
    /** Asks which friend to blend with, then makes it. */
    fun choose(activity: Activity, host: SongHost) {
        if (!Account.signedIn) {
            activity.say("Sign in from the Friends tab to blend with a friend.")
            return
        }
        Thread {
            val friends = Account.call("GET", "/social").optJSONArray("friends")
            val list = (0 until (friends?.length() ?: 0)).map { friends!!.getJSONObject(it) }
            activity.runOnUiThread {
                if (list.isEmpty()) {
                    activity.say("Add a friend first, then you can blend your music.")
                    return@runOnUiThread
                }
                Sheet.show(activity, "Blend with", "Mix your music with a friend's", null, list.map { friend ->
                    Sheet.Option(friend.optString("display_name"), R.drawable.ic_blend) { make(activity, host, friend.optString("id"), friend.optString("display_name")) }
                })
            }
        }.start()
    }

    fun make(activity: Activity, host: SongHost, friendId: String, friendName: String) {
        activity.say("Blending with $friendName...")
        // Read the library on the main thread, then do the rest away from it.
        val mine = (Library.favorites + Library.history).distinctBy { it.id }
        Thread {
            val reply = Account.call("GET", "/friends/$friendId/taste")
            val error = reply.optString("error")
            val theirs = reply.optJSONArray("tracks")?.let { array ->
                (0 until array.length()).mapNotNull { fromCloud(array.getJSONObject(it)) }
            }.orEmpty()
            if (error.isNotEmpty() || theirs.isEmpty() || mine.isEmpty()) {
                activity.runOnUiThread {
                    activity.say(
                        when {
                            error.isNotEmpty() -> error
                            mine.isEmpty() -> "Like or play a few songs first, so there is something to blend."
                            else -> "$friendName has not shared any songs yet."
                        },
                    )
                }
                return@Thread
            }
            val myIds = mine.mapTo(HashSet()) { it.id }
            val shared = theirs.filter { it.id in myIds }
            val blend = LinkedHashMap<String, Song>()
            shared.forEach { blend[it.id] = it }
            val a = mine.filter { it.id !in blend }.iterator()
            val b = theirs.filter { it.id !in blend }.iterator()
            while (blend.size < 50 && (a.hasNext() || b.hasNext())) {
                if (a.hasNext()) a.next().let { blend.putIfAbsent(it.id, it) }
                if (b.hasNext()) b.next().let { blend.putIfAbsent(it.id, it) }
            }
            // Songs like the ones you share (or like the first of each of you), to widen the mix.
            val seeds = (shared.take(2) + mine.take(1) + theirs.take(1)).distinctBy { it.id }
            for (seed in seeds) {
                songsOf(runCatching { JSONObject(Core.radio(seed.id, 12)) }.getOrDefault(JSONObject())).take(8).forEach { blend.putIfAbsent(it.id, it) }
            }
            val myArtists = mine.map { it.artist.lowercase() }.filter { it.isNotEmpty() }.toSet()
            val theirArtists = theirs.map { it.artist.lowercase() }.filter { it.isNotEmpty() }.toSet()
            val union = (myArtists + theirArtists).size.coerceAtLeast(1)
            val match = ((myArtists intersect theirArtists).size * 100 / union).coerceIn(1, 99)
            val songs = blend.values.toList()
            activity.runOnUiThread {
                val playlist = Library.createPlaylist("Blend: You & $friendName")
                songs.forEach { Library.addTo(playlist, it) }
                activity.ask("You and $friendName: $match% match", "${shared.size} songs you both have, ${songs.size} in all. It is saved in Your Library as \"${playlist.name}\".", ok = "Play", cancel = "Close") {
                    Playback.nextSource = playlist.name
                    host.play(songs, 0)
                }
            }
        }.start()
    }

    /** A song from the cloud's library format, if it is one this app can play. */
    private fun fromCloud(t: JSONObject): Song? {
        val platform = t.optString("platform")
        if (platform != "youtube" && platform != "ytmusic") return null
        val video = Uri.parse(t.optString("source")).getQueryParameter("v") ?: return null
        return Song(video, t.optString("title"), t.optString("artist"), t.str("cover").ifEmpty { "https://i.ytimg.com/vi/$video/mqdefault.jpg" }, t.optLong("duration"), t.optString("id"), t.str("album"))
    }
}
