package org.roynit.music

import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.os.Handler
import android.os.Looper
import android.util.LruCache
import android.widget.ImageView
import java.net.HttpURLConnection
import java.net.URL
import java.util.concurrent.Executors

/**
 * Cover pictures with a hard memory budget: each one is asked for at the size it is shown, decoded in 16-bit
 * colour and kept in a small cache, so a long list of songs never grows the app.
 */
object Images {
    private val cache = object : LruCache<String, Bitmap>(6 * 1024 * 1024) {
        override fun sizeOf(key: String, value: Bitmap) = value.byteCount
    }
    private val workers = Executors.newFixedThreadPool(2)
    private val main = Handler(Looper.getMainLooper())

    /** Google's picture servers resize on request: ask for the size that will be drawn, not the original. */
    private val sizing = Regex("=w\\d+-h\\d+[^/]*$")

    private fun sized(url: String, px: Int): String {
        if (!url.contains("googleusercontent.com") && !url.contains("ggpht.com")) return url
        val size = "=w$px-h$px-l90-rj"
        return if (sizing.containsMatchIn(url)) url.replace(sizing, size) else url + size
    }

    /** Shows `url` in `view`; `onBitmap` also gets the picture (the Now Playing screen tints itself with it). */
    fun load(view: ImageView, url: String?, px: Int, onBitmap: ((Bitmap) -> Unit)? = null) {
        view.tag = url
        if (url.isNullOrEmpty()) {
            view.setImageDrawable(null)
            return
        }
        val key = sized(url, px)
        val hit = cache.get(key)
        if (hit != null) {
            show(view, hit)
            onBitmap?.invoke(hit)
            return
        }
        view.setImageDrawable(null)
        workers.execute {
            val bitmap = fetch(key, px) ?: return@execute
            cache.put(key, bitmap)
            main.post {
                if (view.tag == url) {
                    show(view, bitmap)
                    onBitmap?.invoke(bitmap)
                }
            }
        }
    }

    private fun show(view: ImageView, bitmap: Bitmap) {
        val radius = view.getTag(R.id.round_radius) as? Int
        if (radius != null && radius > 0) view.setImageDrawable(RoundDrawable(bitmap, radius.toFloat())) else view.setImageBitmap(bitmap)
    }

    private fun fetch(address: String, px: Int): Bitmap? = runCatching {
        val connection = URL(address).openConnection() as HttpURLConnection
        connection.connectTimeout = 6_000
        connection.readTimeout = 8_000
        val bytes = connection.inputStream.use { it.readBytes() }
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

    /** The picture's overall colour, for tinting the player behind it. */
    fun average(bitmap: Bitmap): Int = Bitmap.createScaledBitmap(bitmap, 1, 1, true).getPixel(0, 0)
}
