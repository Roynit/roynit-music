package org.roynit.music

import android.content.Context
import android.os.Handler
import android.os.Looper
import org.json.JSONObject
import java.io.File
import java.net.HttpURLConnection
import java.net.URL
import java.util.concurrent.Executors

/**
 * Songs kept on the phone so they play with no connection and use no data. A song is fetched once, one at a time
 * (so a long list does not crowd the network or the memory), and stored as a plain audio file the player reads
 * directly. Play checks here first, so downloaded songs never touch the network.
 */
object Downloads {
    private var dir: File? = null
    private val main = Handler(Looper.getMainLooper())
    private val worker = Executors.newSingleThreadExecutor()
    private val waiting = LinkedHashMap<String, Song>()

    /** The song being fetched and how far along it is (0..100). */
    var activeId: String? = null
        private set

    /** The song being fetched, for the list that shows its progress. */
    var activeSong: Song? = null
        private set

    val waitingCount get() = waiting.size
    var percent = 0
        private set

    /** Called when something finished or progressed, so lists can redraw. */
    var onChange: (() -> Unit)? = null

    /** What has been downloaded, for the Downloaded list (newest first). */
    val songs = ArrayList<Song>()

    fun load(context: Context) {
        if (dir != null) return
        val d = File(context.applicationContext.filesDir, "downloads").apply { mkdirs() }
        dir = d
        runCatching {
            val index = File(d, "index.json")
            if (index.isFile) {
                val array = org.json.JSONArray(index.readText())
                for (i in 0 until array.length()) {
                    val song = Song.fromJson(array.getJSONObject(i))
                    if (file(song.id) != null) songs += song
                }
            }
        }
    }

    private fun name(id: String) = id.replace(':', '_') + ".m4a"

    fun file(id: String): File? = dir?.let { File(it, name(id)) }?.takeIf { it.isFile && it.length() > 0 }

    fun has(id: String) = file(id) != null

    fun isBusy(id: String) = id == activeId || waiting.containsKey(id)

    fun bytes() = songs.sumOf { file(it.id)?.length() ?: 0L }

    private fun saveIndex() {
        val d = dir ?: return
        val text = org.json.JSONArray(songs.map { it.toJson() }).toString()
        runCatching { File(d, "index.json").writeText(text) }
    }

    /** Adds songs to the line; they are fetched one after the other. */
    fun queue(context: Context, list: List<Song>) {
        load(context)
        // SoundCloud songs arrive in segments and cannot be saved as one file.
        for (song in list) if (!song.id.startsWith("sc:") && !has(song.id) && !isBusy(song.id)) waiting[song.id] = song
        pump()
    }

    fun remove(song: Song) {
        waiting.remove(song.id)
        dir?.let { File(it, name(song.id)).delete() }
        songs.removeAll { it.id == song.id }
        saveIndex()
        onChange?.invoke()
    }

    private fun pump() {
        if (activeId != null) return
        val next = waiting.entries.firstOrNull() ?: return
        waiting.remove(next.key)
        activeId = next.key
        activeSong = next.value
        percent = 0
        main.post { onChange?.invoke() }
        worker.execute { fetch(next.value) }
    }

    private fun fetch(song: Song) {
        val d = dir ?: return
        val part = File(d, name(song.id) + ".part")
        var ok = false
        runCatching {
            val reply = JSONObject(Core.resolveAny(song.id, song.title, song.artist, song.seconds.toDouble()))
            val url = reply.optString("url")
            check(url.isNotEmpty()) { reply.toString() }
            // YouTube serves one connection at about the speed of playback, so the song is cut into pieces that are
            // fetched four at a time and written where they belong.
            val probe = URL(url).openConnection() as HttpURLConnection
            probe.setRequestProperty("Range", "bytes=0-0")
            probe.connectTimeout = 10_000
            probe.readTimeout = 20_000
            val total = probe.getHeaderField("Content-Range")?.substringAfter('/')?.toLongOrNull() ?: probe.contentLengthLong
            probe.inputStream.use { it.readBytes() }
            check(total > 0)
            val piece = 1024 * 1024L
            val count = ((total + piece - 1) / piece).toInt()
            val next = java.util.concurrent.atomic.AtomicInteger(0)
            val done = java.util.concurrent.atomic.AtomicLong(0)
            val failed = java.util.concurrent.atomic.AtomicBoolean(false)
            java.io.RandomAccessFile(part, "rw").use { file ->
                file.setLength(total)
                val threads = (0 until 4).map {
                    Thread {
                        val buffer = ByteArray(64 * 1024)
                        while (!failed.get()) {
                            val i = next.getAndIncrement()
                            if (i >= count) break
                            val start = i * piece
                            val end = minOf(start + piece, total) - 1
                            var tries = 0
                            while (true) {
                                try {
                                    val http = URL(url).openConnection() as HttpURLConnection
                                    http.setRequestProperty("Range", "bytes=$start-$end")
                                    http.connectTimeout = 10_000
                                    http.readTimeout = 20_000
                                    var at = start
                                    http.inputStream.use { input ->
                                        while (at <= end) {
                                            val n = input.read(buffer)
                                            if (n < 0) break
                                            synchronized(file) { file.seek(at); file.write(buffer, 0, n) }
                                            at += n
                                            val soFar = done.addAndGet(n.toLong())
                                            val p = (soFar * 100 / total).toInt()
                                            if (p != percent) {
                                                percent = p
                                                main.post { onChange?.invoke() }
                                            }
                                        }
                                    }
                                    if (at <= end) throw java.io.IOException("short piece")
                                    break
                                } catch (e: Exception) {
                                    if (++tries >= 3) {
                                        failed.set(true)
                                        break
                                    }
                                    done.addAndGet(-(0L))
                                }
                            }
                        }
                    }.also { it.start() }
                }
                threads.forEach { it.join() }
            }
            ok = !failed.get() && part.length() == total && part.renameTo(File(d, name(song.id)))
        }.onFailure { android.util.Log.w("RoyDownload", "${song.id}: $it") }
        if (!ok) part.delete()
        main.post {
            if (ok) {
                songs.removeAll { it.id == song.id }
                songs.add(0, song)
                saveIndex()
            }
            activeId = null
            activeSong = null
            onChange?.invoke()
            pump()
        }
    }
}
