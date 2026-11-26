package org.roynit.music

import android.content.Context
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.os.Handler
import android.os.Looper
import android.util.LruCache
import android.widget.ImageView
import java.io.File
import java.net.HttpURLConnection
import java.net.URL
import java.security.MessageDigest
import java.util.concurrent.Executors

/**
 * Pictures that show at once. Each is asked for at the size it is shown and kept three ways: decoded in memory (a
 * small budget, 16-bit colour), as the downloaded file on disk (so a picture seen once never waits for the network
 * again, even after the app restarts), and ahead of time: lists ask for the pictures that are about to scroll into view.
 */
object Images {
    private val cache = object : LruCache<String, Bitmap>(16 * 1024 * 1024) {
        override fun sizeOf(key: String, value: Bitmap) = value.byteCount
    }
    private val workers = Executors.newFixedThreadPool(4)
    private val main = Handler(Looper.getMainLooper())
    private var folder: File? = null
    private val inFlight = HashSet<String>()

    /** Call once at start: gives the disk cache a home and trims it to a sensible size. */
    fun init(context: Context) {
        if (folder != null) return
        folder = File(context.applicationContext.cacheDir, "pictures").apply { mkdirs() }
        workers.execute {
            val files = folder?.listFiles() ?: return@execute
            var total = files.sumOf { it.length() }
            // The oldest go first once the folder passes 160 MB.
            for (file in files.sortedBy { it.lastModified() }) {
                if (total <= 160L * 1024 * 1024) break
                total -= file.length()
                file.delete()
            }
        }
    }

    /** Google's picture servers resize on request: ask for the size that will be drawn, not the original. */
    private val sizing = Regex("=w\\d+-h\\d+[^/]*$")

    private fun sized(url: String, px: Int): String {
        if (!url.contains("googleusercontent.com") && !url.contains("ggpht.com")) return url
        val size = "=w$px-h$px-l90-rj"
        return if (sizing.containsMatchIn(url)) url.replace(sizing, size) else url + size
    }

    /** Decoded pictures are kept per size class, so a small copy never stands in for a big one. */
    private fun memoryKey(key: String, px: Int) = key + "@" + (if (px <= 96) 96 else if (px <= 192) 192 else if (px <= 384) 384 else 768)

    private fun fileFor(address: String): File? {
        val dir = folder ?: return null
        val hash = MessageDigest.getInstance("SHA-1").digest(address.toByteArray()).joinToString("") { "%02x".format(it) }
        return File(dir, hash)
    }

    /** Shows `url` in `view`; `onBitmap` also gets the picture (the Now Playing screen tints itself with it). */
    fun load(view: ImageView, url: String?, px: Int, onBitmap: ((Bitmap) -> Unit)? = null) {
        view.tag = url
        if (url.isNullOrEmpty()) {
            view.setImageDrawable(null)
            return
        }
        val key = sized(url, px)
        val memory = memoryKey(key, px)
        val hit = cache.get(memory)
        if (hit != null) {
            show(view, hit)
            onBitmap?.invoke(hit)
            return
        }
        // Already on disk and small: decode it right here, so the picture is there on the very first frame.
        val saved = fileFor(key)?.takeIf { it.isFile && it.length() in 1..150_000 }
        if (saved != null) {
            val bitmap = decode(saved.readBytes(), px)
            if (bitmap != null) {
                saved.setLastModified(System.currentTimeMillis())
                cache.put(memory, bitmap)
                show(view, bitmap)
                onBitmap?.invoke(bitmap)
                return
            }
        }
        view.setImageDrawable(null)
        workers.execute {
            val bitmap = fetch(key, px) ?: return@execute
            cache.put(memory, bitmap)
            main.post {
                if (view.tag == url) {
                    show(view, bitmap)
                    onBitmap?.invoke(bitmap)
                }
            }
        }
    }

    /** Gets pictures ready before they are asked for: downloaded to disk and decoded into memory. */
    fun prefetch(urls: List<String?>, px: Int) {
        for (url in urls.asSequence().filterNotNull().filter { it.isNotEmpty() }.distinct().take(40)) {
            val key = sized(url, px)
            if (cache.get(memoryKey(key, px)) != null) continue
            val fresh = synchronized(inFlight) { inFlight.add(key) }
            if (!fresh) continue
            workers.execute {
                try {
                    fetch(key, px)?.let { cache.put(memoryKey(key, px), it) }
                } finally {
                    synchronized(inFlight) { inFlight.remove(key) }
                }
            }
        }
    }

    private fun show(view: ImageView, bitmap: Bitmap) {
        val radius = view.getTag(R.id.round_radius) as? Int
        if (radius != null && radius > 0) view.setImageDrawable(RoundDrawable(bitmap, radius.toFloat())) else view.setImageBitmap(bitmap)
    }

    private fun decode(bytes: ByteArray, px: Int): Bitmap? = runCatching {
        val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
        BitmapFactory.decodeByteArray(bytes, 0, bytes.size, bounds)
        var sample = 1
        while (bounds.outWidth / (sample * 2) >= px && bounds.outHeight / (sample * 2) >= px) sample *= 2
        val options = BitmapFactory.Options().apply {
            inSampleSize = sample
            inPreferredConfig = Bitmap.Config.RGB_565
        }
        BitmapFactory.decodeByteArray(bytes, 0, bytes.size, options)
    }.getOrNull()

    /** The bytes of a picture: from the disk copy when there is one, otherwise downloaded and saved. */
    private fun fetch(address: String, px: Int): Bitmap? = runCatching {
        val file = fileFor(address)
        val bytes = if (file != null && file.isFile && file.length() > 0) {
            file.setLastModified(System.currentTimeMillis())
            file.readBytes()
        } else {
            val connection = URL(address).openConnection() as HttpURLConnection
            connection.connectTimeout = 6_000
            connection.readTimeout = 8_000
            val downloaded = connection.inputStream.use { it.readBytes() }
            if (file != null && downloaded.isNotEmpty()) {
                val part = File(file.parentFile, file.name + ".part")
                part.writeBytes(downloaded)
                part.renameTo(file)
            }
            downloaded
        }
        decode(bytes, px)
    }.getOrNull()

    /** The picture's overall colour, for tinting the player behind it. */
    fun average(bitmap: Bitmap): Int = Bitmap.createScaledBitmap(bitmap, 1, 1, true).getPixel(0, 0)
}
