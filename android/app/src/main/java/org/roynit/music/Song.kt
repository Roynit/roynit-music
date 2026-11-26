package org.roynit.music

import android.net.Uri
import android.os.Bundle
import androidx.media3.common.MediaItem
import androidx.media3.common.MediaMetadata
import androidx.media3.common.MimeTypes
import org.json.JSONObject

/**
 * One song. [id] is a YouTube video id, "sc:<number>" for a SoundCloud track, or "sp:<id>" for a Spotify song (which
 * plays from the same recording on YouTube Music). [platform] says where it was found.
 */
data class Song(
    val id: String,
    val title: String,
    val artist: String,
    val cover: String?,
    val seconds: Long = 0,
    /** The id this song has in the Roy account's cloud library (the PC app's id), or empty when it is new here. */
    val key: String = "",
    val album: String = "",
    val platform: String = "",
) {
    fun toJson(): JSONObject = JSONObject()
        .put("id", id).put("title", title).put("artist", artist).put("cover", cover ?: "").put("seconds", seconds)
        .put("key", key).put("album", album).put("platform", platform)

    /** The player's queue holds only the id and the display text; the address is found when it plays. */
    fun toItem(): MediaItem = MediaItem.Builder()
        .setMediaId(id)
        // SoundCloud plays as a segmented stream, which the player must be told about before it looks the address up.
        .apply { if (id.startsWith("sc:")) setMimeType(MimeTypes.APPLICATION_M3U8) }
        .setMediaMetadata(
            MediaMetadata.Builder()
                .setTitle(title)
                .setArtist(artist)
                .setArtworkUri(cover?.let(Uri::parse))
                .setExtras(Bundle().apply { putLong("seconds", seconds); putString("platform", platform) })
                .build(),
        )
        .build()

    /** The short tag shown next to results, as on the PC app: YT, SP or SC. */
    val tag: String
        get() = when {
            id.startsWith("sp:") -> "SP"
            id.startsWith("sc:") -> "SC"
            else -> "YT"
        }

    val tagColor: Int
        get() = when (tag) {
            "SP" -> 0xFF5CDE92.toInt()
            "SC" -> 0xFFFF9B5F.toInt()
            else -> 0xFFFF6D79.toInt()
        }

    companion object {
        fun fromJson(o: JSONObject) = Song(
            o.getString("id"), o.optString("title"), o.optString("artist"),
            o.str("cover").ifEmpty { null }, o.optLong("seconds"), o.optString("key"), o.str("album"), o.optString("platform"),
        )

        /** A track as the Rust core reports it, or null when it is not something this app can play. */
        fun fromTrack(t: JSONObject): Song? {
            // A missing value arrives as JSON null, which optString would turn into the text "null".
            fun text(key: String) = if (t.isNull(key)) "" else t.optString(key)
            val platform = text("platform")
            val trackId = text("id")
            val title = text("title")
            val artist = text("artist")
            val cover = text("cover").ifEmpty { null }
            val seconds = t.optDouble("duration", 0.0).toLong()
            val album = text("album")
            return when (platform) {
                "spotify" -> Song("sp:" + trackId.removePrefix("spotify:"), title, artist, cover, seconds, trackId, album, "spotify")
                "soundcloud" -> Song("sc:" + trackId.removePrefix("soundcloud:"), title, artist, cover, seconds, trackId, album, "soundcloud")
                else -> {
                    val id = Uri.parse(text("source")).getQueryParameter("v") ?: return null
                    Song(id, title, artist, cover, seconds, album = album, platform = if (platform == "youtube") "youtube" else "ytmusic")
                }
            }
        }

        /** Where the player reads a song from; it carries what is needed to find the real address when it plays. */
        fun playUri(id: String, title: String, artist: String, seconds: Long): Uri =
            Uri.parse("roy://track/" + Uri.encode(id)).buildUpon()
                .appendQueryParameter("t", title).appendQueryParameter("a", artist).appendQueryParameter("d", seconds.toString())
                .build()
    }
}

fun MediaItem.toSong() = Song(
    mediaId,
    mediaMetadata.title?.toString().orEmpty(),
    mediaMetadata.artist?.toString().orEmpty(),
    mediaMetadata.artworkUri?.toString(),
    mediaMetadata.extras?.getLong("seconds") ?: 0,
    platform = mediaMetadata.extras?.getString("platform").orEmpty(),
)

/** The item with the address the player will read, as the service queues it. */
fun MediaItem.withPlayUri(): MediaItem {
    val song = toSong()
    return buildUpon()
        .apply { if (song.id.startsWith("sc:")) setMimeType(MimeTypes.APPLICATION_M3U8) }
        .setUri(Song.playUri(song.id, song.title, song.artist, song.seconds))
        .build()
}

/** Songs the app picks by itself (a radio, autoplay): the ones the listener turned off are left out. */
fun suggestionsOf(reply: JSONObject): List<Song> = songsOf(reply).filter { !Library.blocks(it) }

/** The songs in a reply from the Rust core (`{"tracks":[...]}`), without repeats. */
fun songsOf(reply: JSONObject): List<Song> {
    val tracks = reply.optJSONArray("tracks") ?: return emptyList()
    val seen = HashSet<String>()
    val out = ArrayList<Song>()
    for (i in 0 until tracks.length()) {
        val song = Song.fromTrack(tracks.getJSONObject(i)) ?: continue
        if (seen.add(song.id)) out += song
    }
    return out
}

/** A text field that may be JSON null: [org.json.JSONObject.optString] would turn null into the text "null". */
fun JSONObject.str(key: String): String = if (isNull(key)) "" else optString(key)
