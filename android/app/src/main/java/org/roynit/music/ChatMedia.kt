package org.roynit.music

import android.Manifest
import android.app.Activity
import android.content.ContentUris
import android.content.Context
import android.content.pm.PackageManager
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.Matrix
import android.graphics.Paint
import android.graphics.RectF
import android.media.ExifInterface
import android.media.MediaPlayer
import android.media.MediaRecorder
import android.net.Uri
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.provider.MediaStore
import android.util.Size
import android.view.Gravity
import android.view.View
import android.view.ViewGroup
import android.widget.FrameLayout
import android.widget.GridLayout
import android.widget.ImageView
import android.widget.LinearLayout
import android.widget.ScrollView
import java.io.ByteArrayOutputStream
import java.io.File
import java.net.HttpURLConnection
import java.net.URL
import java.util.concurrent.Executors

/** Photos and voice messages in a conversation: preparing them to send, recording, and playing them back. */
object ChatMedia {
    /** The permission that lets the photo tray list the gallery (it differs by Android version). */
    val galleryPermission get() = if (Build.VERSION.SDK_INT >= 33) Manifest.permission.READ_MEDIA_IMAGES else Manifest.permission.READ_EXTERNAL_STORAGE

    fun has(context: Context, permission: String) = context.checkSelfPermission(permission) == PackageManager.PERMISSION_GRANTED

    /** A photo shrunk to at most 1080 px on its long side, upright, as a JPEG under the server's 450 KB limit. */
    fun photo(context: Context, uri: Uri): ByteArray? = runCatching {
        val resolver = context.contentResolver
        val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
        resolver.openInputStream(uri)?.use { BitmapFactory.decodeStream(it, null, bounds) }
        var sample = 1
        while (maxOf(bounds.outWidth, bounds.outHeight) / (sample * 2) >= 1080) sample *= 2
        val decoded = resolver.openInputStream(uri)?.use { BitmapFactory.decodeStream(it, null, BitmapFactory.Options().apply { inSampleSize = sample }) } ?: return null
        val turn = resolver.openInputStream(uri)?.use {
            when (ExifInterface(it).getAttributeInt(ExifInterface.TAG_ORIENTATION, ExifInterface.ORIENTATION_NORMAL)) {
                ExifInterface.ORIENTATION_ROTATE_90 -> 90f
                ExifInterface.ORIENTATION_ROTATE_180 -> 180f
                ExifInterface.ORIENTATION_ROTATE_270 -> 270f
                else -> 0f
            }
        } ?: 0f
        val scale = 1080f / maxOf(decoded.width, decoded.height)
        val matrix = Matrix().apply { if (scale < 1f) postScale(scale, scale); if (turn != 0f) postRotate(turn) }
        val ready = if (scale < 1f || turn != 0f) Bitmap.createBitmap(decoded, 0, 0, decoded.width, decoded.height, matrix, true) else decoded
        var quality = 80
        var out = ByteArrayOutputStream()
        ready.compress(Bitmap.CompressFormat.JPEG, quality, out)
        while (out.size() > 260_000 && quality > 40) {
            quality -= 10
            out = ByteArrayOutputStream()
            ready.compress(Bitmap.CompressFormat.JPEG, quality, out)
        }
        out.toByteArray().takeIf { it.size <= 440_000 }
    }.getOrNull()

    /** Sends one prepared message to a friend; the reply is the new message or `{"error":...}`. Worker thread. */
    fun send(friend: String, kind: String, bytes: ByteArray, type: String, seconds: Int = 0, replyTo: Long = 0): org.json.JSONObject {
        var path = "/messages/$friend/media?kind=$kind"
        if (seconds > 0) path += "&seconds=$seconds"
        if (replyTo > 0) path += "&reply_to=$replyTo"
        return Account.post(path, bytes, type)
    }
}

/** Records a voice message as small AAC sound (about 3 KB a second), with a level to draw as a waveform. */
class VoiceRecorder(private val context: Context) {
    private var recorder: MediaRecorder? = null
    private var file: File? = null
    private var started = 0L
    private var paused = 0L
    private var pausedAt = 0L
    var isPaused = false
        private set

    val running get() = recorder != null

    fun start(): Boolean = runCatching {
        val target = File(context.cacheDir, "voice-message.m4a").also { it.delete() }
        val r = if (Build.VERSION.SDK_INT >= 31) MediaRecorder(context) else @Suppress("DEPRECATION") MediaRecorder()
        r.setAudioSource(MediaRecorder.AudioSource.MIC)
        r.setOutputFormat(MediaRecorder.OutputFormat.MPEG_4)
        r.setAudioEncoder(MediaRecorder.AudioEncoder.AAC)
        r.setAudioChannels(1)
        r.setAudioSamplingRate(16_000)
        r.setAudioEncodingBitRate(24_000)
        r.setMaxDuration(180_000)
        r.setOutputFile(target.absolutePath)
        r.prepare()
        r.start()
        recorder = r
        file = target
        started = System.currentTimeMillis()
        paused = 0
        isPaused = false
        true
    }.getOrDefault(false)

    fun togglePause() {
        val r = recorder ?: return
        runCatching {
            if (isPaused) { r.resume(); paused += System.currentTimeMillis() - pausedAt; isPaused = false } else { r.pause(); pausedAt = System.currentTimeMillis(); isPaused = true }
        }
    }

    /** Seconds recorded so far (a pause does not count). */
    fun seconds(): Int {
        if (recorder == null) return 0
        val now = if (isPaused) pausedAt else System.currentTimeMillis()
        return ((now - started - paused) / 1000).toInt()
    }

    /** 0..1: how loud it is right now. */
    fun level(): Float = if (isPaused) 0f else runCatching { (recorder?.maxAmplitude ?: 0) / 12_000f }.getOrDefault(0f).coerceIn(0f, 1f)

    /** Stops and returns the sound file (with its length), or null when it was too short or failed. */
    fun finish(): Pair<File, Int>? {
        val r = recorder ?: return null
        val length = seconds()
        recorder = null
        val ok = runCatching { r.stop() }.isSuccess
        r.release()
        val done = file
        file = null
        return if (ok && done != null && done.length() > 0 && length >= 1) done to length else null
    }

    fun cancel() {
        val r = recorder ?: return
        recorder = null
        runCatching { r.stop() }
        r.release()
        file?.delete()
        file = null
    }
}

/** Plays one voice message at a time: the file is fetched once into the cache, then played from there. */
object VoicePlayer {
    private var player: MediaPlayer? = null
    private val main = Handler(Looper.getMainLooper())
    private val pool = Executors.newSingleThreadExecutor()
    var current: String? = null
        private set
    @Volatile private var loading: String? = null

    fun isPlaying(url: String) = current == url && player?.isPlaying == true

    /** 0..1 through the message that is playing (0 for any other). */
    fun progress(url: String): Float {
        val p = player ?: return 0f
        if (current != url) return 0f
        return runCatching { if (p.duration > 0) p.currentPosition / p.duration.toFloat() else 0f }.getOrDefault(0f)
    }

    fun stop() {
        runCatching { player?.stop() }
        player?.release()
        player = null
        current = null
    }

    fun toggle(context: Context, url: String, changed: () -> Unit) {
        if (current == url && player != null) {
            val p = player!!
            if (p.isPlaying) p.pause() else p.start()
            changed()
            return
        }
        stop()
        loading = url
        pool.execute {
            val file = File(context.cacheDir, "voice-" + url.hashCode().toUInt().toString(16) + ".m4a")
            if (!file.isFile || file.length() == 0L) {
                runCatching {
                    val connection = URL(url).openConnection() as HttpURLConnection
                    connection.connectTimeout = 8_000
                    connection.readTimeout = 15_000
                    connection.inputStream.use { input -> file.outputStream().use { input.copyTo(it) } }
                }.onFailure { file.delete() }
            }
            main.post {
                if (loading != url) return@post
                loading = null
                if (!file.isFile || file.length() == 0L) { context.say("That voice message could not be played."); return@post }
                val p = MediaPlayer()
                runCatching {
                    p.setDataSource(file.absolutePath)
                    p.setOnCompletionListener { stop(); changed() }
                    p.prepare()
                    p.start()
                    player = p
                    current = url
                }.onFailure { p.release(); context.say("That voice message could not be played.") }
                changed()
            }
        }
    }
}

/** A row of bars: the shape of a voice message, coloured up to how far it has played (or drawn live while recording). */
class WaveView(context: Context, private val bars: Int = 28) : View(context) {
    var levels = FloatArray(bars) { 0.2f }
        set(value) { field = value; invalidate() }
    var progress = 0f
        set(value) { field = value; invalidate() }
    var played = Color.WHITE
    var rest = 0x66FFFFFF
    private val paint = Paint(Paint.ANTI_ALIAS_FLAG)

    override fun onDraw(canvas: Canvas) {
        val gap = width / bars.toFloat()
        val barWidth = gap * 0.5f
        for (i in 0 until bars) {
            val level = levels.getOrElse(i) { 0.2f }.coerceIn(0.12f, 1f)
            val h = height * level
            paint.color = if (i / bars.toFloat() < progress) played else rest
            val x = i * gap + (gap - barWidth) / 2
            canvas.drawRoundRect(RectF(x, (height - h) / 2, x + barWidth, (height + h) / 2), barWidth / 2, barWidth / 2, paint)
        }
    }

    companion object {
        /** A steady, made-up shape for a message (a voice message has no stored waveform). */
        fun shape(seed: Long, bars: Int = 28) = FloatArray(bars) { i -> 0.25f + 0.75f * (((seed * 31 + i * 17) * 2654435761L) and 0xFF) / 255f }
    }
}

/** The photo tray under the message box: recent photos in a grid, tap to choose, then send. */
class PhotoTray(private val c: Context, private val onSend: (List<Uri>) -> Unit, private val onBrowse: () -> Unit) : LinearLayout(c) {
    private val chosen = ArrayList<Uri>()
    private val grid = GridLayout(c).apply { columnCount = 3 }
    private val send = c.label(15f, Color.BLACK, bold = true).apply {
        gravity = Gravity.CENTER
        background = roundRect(Ui.GREEN, c.dp(24))
        setPadding(c.dp(22), c.dp(11), c.dp(22), c.dp(11))
        visibility = GONE
        setOnClickListener { val take = chosen.toList(); chosen.clear(); refreshSend(); onSend(take) }
    }
    private val workers = Executors.newFixedThreadPool(3)
    private val main = Handler(Looper.getMainLooper())
    private var loaded = false

    init {
        orientation = VERTICAL
        setBackgroundColor(Ui.SURFACE)
        val head = LinearLayout(c).apply {
            gravity = Gravity.CENTER_VERTICAL
            setPadding(c.dp(16), c.dp(10), c.dp(16), c.dp(8))
            addView(c.label(16f, Color.WHITE, bold = true).apply { text = "All photos" }, LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f))
            addView(c.label(14f, Ui.GREEN, bold = true).apply { text = "Browse"; setPadding(c.dp(8), c.dp(4), c.dp(8), c.dp(4)); setOnClickListener { onBrowse() } })
        }
        addView(head)
        val area = FrameLayout(c)
        area.addView(ScrollView(c).apply { isVerticalScrollBarEnabled = false; addView(grid) })
        area.addView(send, FrameLayout.LayoutParams(ViewGroup.LayoutParams.WRAP_CONTENT, ViewGroup.LayoutParams.WRAP_CONTENT, Gravity.BOTTOM or Gravity.END).apply { setMargins(0, 0, c.dp(16), c.dp(16)) })
        addView(area, LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, 0, 1f))
    }

    private fun refreshSend() {
        send.visibility = if (chosen.isEmpty()) GONE else VISIBLE
        send.text = "Send" + if (chosen.size > 1) " (${chosen.size})" else ""
        for (i in 0 until grid.childCount) (grid.getChildAt(i).tag as? Uri)?.let { uri -> grid.getChildAt(i).findViewWithTag<View>("mark")?.visibility = if (uri in chosen) VISIBLE else GONE }
    }

    /** Lists the newest photos once; call when the tray opens. */
    fun load() {
        if (loaded) return
        loaded = true
        workers.execute {
            val found = ArrayList<Uri>()
            runCatching {
                c.contentResolver.query(MediaStore.Images.Media.EXTERNAL_CONTENT_URI, arrayOf(MediaStore.Images.Media._ID), null, null, MediaStore.Images.Media.DATE_ADDED + " DESC")?.use { cursor ->
                    while (cursor.moveToNext() && found.size < 90) found += ContentUris.withAppendedId(MediaStore.Images.Media.EXTERNAL_CONTENT_URI, cursor.getLong(0))
                }
            }
            main.post { show(found) }
        }
    }

    private fun show(found: List<Uri>) {
        grid.removeAllViews()
        val side = c.resources.displayMetrics.widthPixels / 3
        for (uri in found) {
            val cell = FrameLayout(c).apply { tag = uri; setBackgroundColor(Ui.RAISED) }
            val image = ImageView(c).apply { scaleType = ImageView.ScaleType.CENTER_CROP }
            val mark = FrameLayout(c).apply {
                tag = "mark"
                visibility = GONE
                setBackgroundColor(0x66000000)
                addView(c.icon(R.drawable.ic_check_circle, Ui.GREEN, 30, 0), FrameLayout.LayoutParams(c.dp(30), c.dp(30), Gravity.TOP or Gravity.END).apply { setMargins(0, c.dp(6), c.dp(6), 0) })
            }
            cell.addView(image, FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT))
            cell.addView(mark, FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT))
            cell.setOnClickListener { if (!chosen.remove(uri)) { if (chosen.size < 5) chosen += uri else c.say("You can send up to 5 photos at once") }; refreshSend() }
            grid.addView(cell, GridLayout.LayoutParams().apply { width = side - 2; height = side - 2; setMargins(1, 1, 1, 1) })
            workers.execute {
                val bitmap = runCatching {
                    if (Build.VERSION.SDK_INT >= 29) c.contentResolver.loadThumbnail(uri, Size(300, 300), null)
                    else c.contentResolver.openInputStream(uri)?.use { BitmapFactory.decodeStream(it, null, BitmapFactory.Options().apply { inSampleSize = 8 }) }
                }.getOrNull()
                if (bitmap != null) main.post { image.setImageBitmap(bitmap) }
            }
        }
    }

    /** Forgets the chosen photos (the tray is closed or they were sent). */
    fun clear() {
        chosen.clear()
        refreshSend()
    }
}
