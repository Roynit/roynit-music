package org.roynit.music

/** The Rust core. Each call blocks until it is done, so call it from a worker thread. */
object Core {
    init {
        System.loadLibrary("roy_mobile")
    }

    /** `{"tracks":[...]}` for a search, or `{"error":"..."}`. */
    @JvmStatic external fun search(query: String): String

    /** `{"tracks":[...]}` for YouTube Music's home page. */
    @JvmStatic external fun home(): String

    /** `{"tracks":[...]}` of songs that follow on from one video. */
    @JvmStatic external fun radio(videoId: String, limit: Int): String

    /** `{"url":"...","duration":n}` for a YouTube video id, or `{"error":"..."}`. */
    @JvmStatic external fun resolve(videoId: String): String

    /** `{"lines":[{"time":s,"text":"..."}],"text":"..."}` for a song, or `{"error":"..."}`. [dir] keeps found lyrics. */
    @JvmStatic external fun lyrics(title: String, artist: String, seconds: Double, dir: String): String

    /** The same lines with a `words` list (time, end, text) on those a provider has word timings for. [lines] is the lines array as JSON. */
    /** `{"name":..,"tracks":[..]}` for a playlist link (YouTube, YouTube Music, Spotify, Deezer, Apple Music...), or `{"error":..}`. */
    @JvmStatic external fun importPlaylist(link: String): String

    @JvmStatic external fun lyricWords(title: String, artist: String, seconds: Double, lines: String): String

    /** `{"mixes":[{name,tracks}],"picks":[...]}` made from a library snapshot (favourites and history). */
    @JvmStatic external fun mixes(library: String): String

    /** YouTube Music's charts page, as `{"tracks":[...]}`. */
    @JvmStatic external fun charts(): String

    /** `{"tracks":[..],"items":[{id,kind,title,subtitle,cover,params}]}` for a search of one kind: songs, albums, artists, playlists. */
    @JvmStatic external fun searchKind(query: String, kind: String): String

    /** An artist's or album's page: `{"tracks":[..],"items":[..]}`. [params] is empty unless the search gave one. */
    @JvmStatic external fun browse(id: String, params: String): String

    /** A podcast show: `{"title":"..","tracks":[episodes]}`. */
    @JvmStatic external fun podcast(id: String): String

    /** `{"tracks":[..]}` from one source: "ytmusic", "youtube", "spotify" or "soundcloud". */
    @JvmStatic external fun searchSource(query: String, source: String): String

    /** An address for a song from any source; Spotify songs are matched to YouTube Music by title, artist and length. */
    @JvmStatic external fun resolveAny(id: String, title: String, artist: String, seconds: Double): String

    @JvmStatic external fun version(): String
}
