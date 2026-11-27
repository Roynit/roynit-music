package org.roynit.music

import android.app.Activity
import android.app.PendingIntent
import android.appwidget.AppWidgetManager
import android.appwidget.AppWidgetProvider
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.graphics.Bitmap
import android.graphics.BitmapShader
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.LinearGradient
import android.graphics.Paint
import android.graphics.RectF
import android.graphics.Shader
import android.net.Uri
import android.os.Bundle
import android.view.Gravity
import android.view.ViewGroup
import android.widget.LinearLayout
import android.widget.RemoteViews
import android.widget.ScrollView
import androidx.media3.common.Player
import org.json.JSONObject
import java.io.File
import java.util.concurrent.Executors

/**
 * Home-screen widgets:
 *  - Now playing: a wide card tinted with the cover's colour, with previous / play / next / like;
 *  - Cover: a small square with the cover filling it;
 *  - Library: liked songs or a playlist on top and a row of tiles that start any of them;
 *  - Playlist: a square you choose (liked songs or any playlist) that plays it with one tap.
 * The buttons talk to the playback service, so they work with the app closed.
 */
object Widgets {
    const val PLAY = "org.roynit.music.widget.PLAY"
    const val PREV = "org.roynit.music.widget.PREV"
    const val NEXT = "org.roynit.music.widget.NEXT"
    const val LIKE = "org.roynit.music.widget.LIKE"
    const val PLAY_LIST = "org.roynit.music.widget.PLAY_LIST"

    /** The player while the service is alive, so a widget added later can show what is playing right now. */
    @Volatile var player: Player? = null

    private val worker = Executors.newSingleThreadExecutor()

    private fun ids(context: Context, provider: Class<*>) = AppWidgetManager.getInstance(context).getAppWidgetIds(ComponentName(context, provider))

    private fun prefs(context: Context) = context.getSharedPreferences("roy_widgets", Context.MODE_PRIVATE)

    // ---- the lists a widget can play ----

    /** One thing a widget can start: liked songs or a playlist, reduced to what is needed to draw it. */
    class Shortcut(val id: String, val name: String, val subtitle: String, val covers: List<String?>, val liked: Boolean)

    private fun count(n: Int) = "$n " + if (n == 1) "song" else "songs"

    /** Taken on the main thread (the library is only touched there) and then drawn on a worker. */
    fun shortcuts(): List<Shortcut> =
        listOf(Shortcut("liked", "Liked Songs", "Playlist • " + count(Library.favorites.size), emptyList(), true)) +
            Library.playlists.toList().map { Shortcut(it.id, it.name, "Playlist • " + count(it.songs.size), it.coverUrls(), false) }

    /** The songs behind a shortcut, with the name to show under "Playing from". Main thread. */
    fun songsOf(id: String): Pair<String, List<Song>>? =
        if (id == "liked") "Liked Songs" to Library.favorites.toList()
        else Library.playlists.firstOrNull { it.id == id }?.let { it.name to it.songs.toList() }

    fun featured(context: Context) = prefs(context).getString("featured", "liked") ?: "liked"
    fun setFeatured(context: Context, id: String) = prefs(context).edit().putString("featured", id).apply()
    fun listOf(context: Context, widgetId: Int) = prefs(context).getString("list_$widgetId", "liked") ?: "liked"
    fun setList(context: Context, widgetId: Int, id: String) = prefs(context).edit().putString("list_$widgetId", id).apply()

    // ---- refreshing ----

    /** Redraws every widget on the home screen from the player (or, with no player yet, from the saved queue). Main thread. */
    fun refresh(context: Context) {
        val app = context.applicationContext
        appContext = app
        val lists = shortcuts()
        val live = player
        if (live != null && live.mediaItemCount > 0) {
            val song = live.currentMediaItem?.toSong()
            val playing = live.isPlaying
            worker.execute { draw(app, song, playing, lists) }
            return
        }
        worker.execute {
            val song = runCatching {
                val json = JSONObject(File(app.filesDir, "queue.json").readText())
                val array = json.getJSONArray("songs")
                Song.fromJson(array.getJSONObject(json.optInt("index").coerceIn(0, array.length() - 1)))
            }.getOrNull()
            draw(app, song, false, lists)
        }
    }

    private fun draw(context: Context, song: Song?, playing: Boolean, lists: List<Shortcut>) {
        val wide = ids(context, NowPlayingWidget::class.java)
        val square = ids(context, CoverWidget::class.java)
        val library = ids(context, LibraryWidget::class.java)
        val picks = ids(context, ListWidget::class.java)
        if (wide.isEmpty() && square.isEmpty() && library.isEmpty() && picks.isEmpty()) return
        Images.init(context)
        Library.load(context)
        val manager = AppWidgetManager.getInstance(context)

        if (wide.isNotEmpty() || square.isNotEmpty()) {
            val picture = song?.cover?.takeIf { it.isNotEmpty() }?.let { Images.blocking(it, 420) }
            val tint = picture?.let { deep(Images.average(it)) } ?: 0xFF2A2A2A.toInt()
            val liked = song != null && Library.isFavorite(song.id)
            if (wide.isNotEmpty()) {
                val views = RemoteViews(context.packageName, R.layout.widget_now_playing)
                views.setImageViewBitmap(R.id.widget_bg, cardBackground(tint, 700, 300))
                if (picture != null) views.setImageViewBitmap(R.id.widget_cover, rounded(picture, 256, 28f)) else views.setImageViewBitmap(R.id.widget_cover, emptyCover())
                texts(views, song?.title?.ifEmpty { "Roy Music" } ?: "Roy Music", if (song == null) "Tap to start listening" else song.artist, playing)
                views.setImageViewResource(R.id.widget_like, if (liked) R.drawable.ic_heart else R.drawable.ic_heart_outline)
                views.setInt(R.id.widget_like, "setColorFilter", if (liked) Ui.GREEN else Color.WHITE)
                views.setInt(R.id.widget_prev, "setColorFilter", Color.WHITE)
                views.setInt(R.id.widget_next, "setColorFilter", Color.WHITE)
                views.setOnClickPendingIntent(R.id.widget_prev, action(context, PREV))
                views.setOnClickPendingIntent(R.id.widget_next, action(context, NEXT))
                views.setOnClickPendingIntent(R.id.widget_like, action(context, LIKE))
                views.setOnClickPendingIntent(R.id.widget_play, action(context, PLAY))
                views.setOnClickPendingIntent(R.id.widget_root, openApp(context))
                wide.forEach { manager.updateAppWidget(it, views) }
            }
            if (square.isNotEmpty()) {
                val views = RemoteViews(context.packageName, R.layout.widget_cover)
                views.setImageViewBitmap(R.id.widget_bg, fullCover(picture, tint, 520))
                texts(views, song?.title?.ifEmpty { "Roy Music" } ?: "Roy Music", if (song == null) "Tap to play" else song.artist, playing)
                views.setOnClickPendingIntent(R.id.widget_play, action(context, PLAY))
                views.setOnClickPendingIntent(R.id.widget_root, openApp(context))
                square.forEach { manager.updateAppWidget(it, views) }
            }
        }

        if (library.isNotEmpty() || picks.isNotEmpty()) {
            val pictures = HashMap<String, Bitmap?>()
            fun picture(url: String?): Bitmap? = if (url.isNullOrEmpty()) null else pictures.getOrPut(url) { Images.blocking(url, 160) }
            fun art(item: Shortcut, size: Int) = tile(item, size) { picture(it) }

            if (library.isNotEmpty()) {
                val chosen = featured(context)
                val feature = lists.firstOrNull { it.id == chosen } ?: lists.first()
                val big = art(feature, 288)
                val tint = if (feature.liked) 0xFF3D2F9E.toInt() else deep(Images.average(big))
                val views = RemoteViews(context.packageName, R.layout.widget_library)
                views.setImageViewBitmap(R.id.widget_bg, cardBackground(tint, 700, 400))
                views.setImageViewBitmap(R.id.widget_cover, roundedSquare(big, 0.12f))
                views.setTextViewText(R.id.widget_title, feature.name)
                views.setTextViewText(R.id.widget_artist, feature.subtitle)
                views.setImageViewResource(R.id.widget_play, R.drawable.ic_play)
                views.setInt(R.id.widget_play, "setColorFilter", Color.BLACK)
                views.setOnClickPendingIntent(R.id.widget_featured, playList(context, feature.id, false))
                views.setOnClickPendingIntent(R.id.widget_play, playList(context, feature.id, false))
                val tiles = intArrayOf(R.id.tile0, R.id.tile1, R.id.tile2, R.id.tile3, R.id.tile4)
                tiles.forEachIndexed { i, tileId ->
                    val item = lists.getOrNull(i)
                    if (item == null) {
                        views.setViewVisibility(tileId, android.view.View.INVISIBLE)
                    } else {
                        views.setViewVisibility(tileId, android.view.View.VISIBLE)
                        views.setImageViewBitmap(tileId, roundedSquare(art(item, 160), 0.14f))
                        // A tile starts its list and becomes the one shown on top.
                        views.setOnClickPendingIntent(tileId, playList(context, item.id, true))
                    }
                }
                library.forEach { manager.updateAppWidget(it, views) }
            }

            picks.forEach { widgetId ->
                val choice = listOf(context, widgetId)
                val item = lists.firstOrNull { it.id == choice } ?: lists.first()
                val views = RemoteViews(context.packageName, R.layout.widget_cover)
                views.setImageViewBitmap(R.id.widget_bg, fullCover(art(item, 520), 0xFF2A2A2A.toInt(), 520))
                texts(views, item.name, item.subtitle, false)
                views.setOnClickPendingIntent(R.id.widget_play, playList(context, item.id, false))
                views.setOnClickPendingIntent(R.id.widget_root, playList(context, item.id, false))
                manager.updateAppWidget(widgetId, views)
            }
        }
    }

    private fun texts(views: RemoteViews, title: String, subtitle: String, playing: Boolean) {
        views.setTextViewText(R.id.widget_title, title)
        views.setTextViewText(R.id.widget_artist, subtitle)
        views.setImageViewResource(R.id.widget_play, if (playing) R.drawable.ic_pause else R.drawable.ic_play)
        views.setInt(R.id.widget_play, "setColorFilter", Color.BLACK)
    }

    private fun action(context: Context, what: String): PendingIntent = PendingIntent.getService(
        context, what.hashCode(), Intent(context, PlaybackService::class.java).setAction(what), PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
    )

    /** Starts a list. The address makes each list's intent its own, since Android treats intents that differ only in extras as one. */
    private fun playList(context: Context, id: String, feature: Boolean): PendingIntent = PendingIntent.getService(
        context, ("list" + id + feature).hashCode(),
        Intent(context, PlaybackService::class.java).setAction(PLAY_LIST).setData(Uri.parse("roy://list/$id?feature=$feature")).putExtra("list", id).putExtra("feature", feature),
        PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
    )

    private fun openApp(context: Context): PendingIntent = PendingIntent.getActivity(
        context, 0, Intent(context, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP or Intent.FLAG_ACTIVITY_CLEAR_TOP), PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
    )

    // ---- drawing ----

    /** The cover's colour, darkened so white text always reads on it. */
    private fun deep(color: Int): Int {
        val hsv = FloatArray(3)
        Color.colorToHSV(color, hsv)
        hsv[1] = (hsv[1] * 1.1f).coerceAtMost(1f)
        hsv[2] = (hsv[2] * 0.6f).coerceIn(0.2f, 0.55f)
        return Color.HSVToColor(hsv)
    }

    /** A rounded card that fades from a colour into the app's black. */
    private fun cardBackground(tint: Int, w: Int, h: Int): Bitmap {
        val out = Bitmap.createBitmap(w, h, Bitmap.Config.ARGB_8888)
        val paint = Paint(Paint.ANTI_ALIAS_FLAG).apply { shader = LinearGradient(0f, 0f, w.toFloat(), h.toFloat(), tint, 0xFF121212.toInt(), Shader.TileMode.CLAMP) }
        Canvas(out).drawRoundRect(RectF(0f, 0f, w.toFloat(), h.toFloat()), 44f, 44f, paint)
        return out
    }

    /** What one shortcut looks like as a square picture: liked songs are the purple heart, a playlist is its covers (a 2x2 mosaic when it has four). */
    private fun tile(item: Shortcut, size: Int, load: (String?) -> Bitmap?): Bitmap {
        val out = Bitmap.createBitmap(size, size, Bitmap.Config.ARGB_8888)
        val canvas = Canvas(out)
        val all = RectF(0f, 0f, size.toFloat(), size.toFloat())
        if (item.liked) {
            canvas.drawRect(all, Paint().apply { shader = LinearGradient(0f, 0f, size.toFloat(), size.toFloat(), 0xFF450AF5.toInt(), 0xFF8E8EE5.toInt(), Shader.TileMode.CLAMP) })
            // The same heart the app uses, drawn from its vector.
            val icon = androidx.core.content.ContextCompat.getDrawable(appContext ?: return out, R.drawable.ic_heart)
            icon?.mutate()?.let {
                val pad = size / 4
                it.setBounds(pad, pad, size - pad, size - pad)
                it.setTint(Color.WHITE)
                it.draw(canvas)
            }
            return out
        }
        val pictures = item.covers.mapNotNull { load(it) }
        canvas.drawRect(all, Paint().apply { color = 0xFF2A2A2A.toInt() })
        when {
            pictures.size >= 4 -> {
                val half = size / 2
                for (i in 0 until 4) {
                    val piece = squareCrop(pictures[i], half)
                    canvas.drawBitmap(piece, ((i % 2) * half).toFloat(), ((i / 2) * half).toFloat(), null)
                }
            }
            pictures.isNotEmpty() -> canvas.drawBitmap(squareCrop(pictures.first(), size), 0f, 0f, null)
        }
        return out
    }

    private var appContext: Context? = null

    fun attach(context: Context) {
        appContext = context.applicationContext
    }

    private fun squareCrop(source: Bitmap, size: Int): Bitmap {
        val side = minOf(source.width, source.height)
        val square = Bitmap.createBitmap(source, (source.width - side) / 2, (source.height - side) / 2, side, side)
        return Bitmap.createScaledBitmap(square, size, size, true)
    }

    /** The same picture with rounded corners; [fraction] is the corner radius as a share of the side. */
    private fun roundedSquare(source: Bitmap, fraction: Float): Bitmap {
        val out = Bitmap.createBitmap(source.width, source.height, Bitmap.Config.ARGB_8888)
        val radius = source.width * fraction
        Canvas(out).drawRoundRect(RectF(0f, 0f, source.width.toFloat(), source.height.toFloat()), radius, radius, Paint(Paint.ANTI_ALIAS_FLAG).apply { shader = BitmapShader(source, Shader.TileMode.CLAMP, Shader.TileMode.CLAMP) })
        return out
    }

    /** The picture filling the whole square card, with a dark fade along the bottom for the text. */
    private fun fullCover(cover: Bitmap?, tint: Int, size: Int): Bitmap {
        val out = Bitmap.createBitmap(size, size, Bitmap.Config.ARGB_8888)
        val canvas = Canvas(out)
        val round = RectF(0f, 0f, size.toFloat(), size.toFloat())
        if (cover != null) {
            val scaled = squareCrop(cover, size)
            canvas.drawRoundRect(round, 44f, 44f, Paint(Paint.ANTI_ALIAS_FLAG).apply { shader = BitmapShader(scaled, Shader.TileMode.CLAMP, Shader.TileMode.CLAMP) })
        } else {
            canvas.drawRoundRect(round, 44f, 44f, Paint(Paint.ANTI_ALIAS_FLAG).apply { color = tint })
        }
        val scrim = Paint(Paint.ANTI_ALIAS_FLAG).apply { shader = LinearGradient(0f, size * 0.45f, 0f, size.toFloat(), 0x00000000, 0xE6000000.toInt(), Shader.TileMode.CLAMP) }
        canvas.drawRoundRect(round, 44f, 44f, scrim)
        return out
    }

    /** The grey square shown before there is a cover. */
    private fun emptyCover(): Bitmap {
        val out = Bitmap.createBitmap(256, 256, Bitmap.Config.ARGB_8888)
        Canvas(out).drawRoundRect(RectF(0f, 0f, 256f, 256f), 28f, 28f, Paint(Paint.ANTI_ALIAS_FLAG).apply { color = 0xFF2A2A2A.toInt() })
        return out
    }

    /** A square, centre-cropped copy with rounded corners. */
    private fun rounded(source: Bitmap, size: Int, radius: Float): Bitmap {
        val scaled = squareCrop(source, size)
        val out = Bitmap.createBitmap(size, size, Bitmap.Config.ARGB_8888)
        Canvas(out).drawRoundRect(RectF(0f, 0f, size.toFloat(), size.toFloat()), radius, radius, Paint(Paint.ANTI_ALIAS_FLAG).apply { shader = BitmapShader(scaled, Shader.TileMode.CLAMP, Shader.TileMode.CLAMP) })
        return out
    }
}

class NowPlayingWidget : AppWidgetProvider() {
    override fun onUpdate(context: Context, manager: AppWidgetManager, ids: IntArray) { Widgets.attach(context); Widgets.refresh(context) }
}

class CoverWidget : AppWidgetProvider() {
    override fun onUpdate(context: Context, manager: AppWidgetManager, ids: IntArray) { Widgets.attach(context); Widgets.refresh(context) }
}

class LibraryWidget : AppWidgetProvider() {
    override fun onUpdate(context: Context, manager: AppWidgetManager, ids: IntArray) { Widgets.attach(context); Library.load(context); Widgets.refresh(context) }
}

class ListWidget : AppWidgetProvider() {
    override fun onUpdate(context: Context, manager: AppWidgetManager, ids: IntArray) { Widgets.attach(context); Library.load(context); Widgets.refresh(context) }
}

/** Opens when a playlist widget is added (and from its reconfigure option): choose liked songs or one of the playlists. */
class WidgetPickActivity : Activity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val widgetId = intent.getIntExtra(AppWidgetManager.EXTRA_APPWIDGET_ID, AppWidgetManager.INVALID_APPWIDGET_ID)
        setResult(RESULT_CANCELED, Intent().putExtra(AppWidgetManager.EXTRA_APPWIDGET_ID, widgetId))
        if (widgetId == AppWidgetManager.INVALID_APPWIDGET_ID) { finish(); return }
        Widgets.attach(this)
        Library.load(this)
        val column = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL; setPadding(dp(16), dp(24), dp(16), dp(24)); setBackgroundColor(Ui.BG) }
        column.addView(label(22f, Color.WHITE, bold = true).apply { text = "Choose what this widget plays" })
        column.addView(label(14f, Ui.MUTED).apply { text = "One tap on the widget starts it."; setPadding(0, dp(4), 0, dp(12)) })
        val current = Widgets.listOf(this, widgetId)
        for (item in Widgets.shortcuts()) {
            val row = LinearLayout(this).apply {
                gravity = Gravity.CENTER_VERTICAL
                background = roundRect(if (item.id == current) 0xFF2F3B33.toInt() else Ui.SURFACE, dp(14))
                setPadding(dp(16), dp(14), dp(16), dp(14))
                addView(LinearLayout(context).apply {
                    orientation = LinearLayout.VERTICAL
                    addView(label(16f, Color.WHITE, bold = true).apply { text = item.name })
                    addView(label(13f, Ui.MUTED).apply { text = item.subtitle })
                }, LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f))
                if (item.id == current) addView(label(13f, Ui.GREEN, bold = true).apply { text = "Selected" })
                setOnClickListener {
                    Widgets.setList(this@WidgetPickActivity, widgetId, item.id)
                    setResult(RESULT_OK, Intent().putExtra(AppWidgetManager.EXTRA_APPWIDGET_ID, widgetId))
                    Widgets.refresh(this@WidgetPickActivity)
                    finish()
                }
            }
            column.addView(row, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT).apply { bottomMargin = dp(8) })
        }
        setContentView(ScrollView(this).apply { setBackgroundColor(Ui.BG); addView(column) })
    }
}
