package org.roynit.music

import android.content.Context
import android.graphics.Color
import android.graphics.drawable.GradientDrawable
import android.view.Gravity
import android.view.View
import android.view.ViewGroup
import android.widget.FrameLayout
import android.widget.HorizontalScrollView
import android.widget.ImageView
import android.widget.LinearLayout
import android.widget.ListView
import android.widget.TextView
import android.widget.Toast
import org.json.JSONObject

/** Something a search or a page found that has a page of its own: an artist, album, playlist or podcast. */
data class Item(val kind: String, val id: String, val params: String, val title: String, val subtitle: String, val cover: String?) {
    companion object {
        fun fromJson(o: JSONObject): Item? {
            val id = o.optString("id")
            if (id.isEmpty()) return null
            // A podcast show is a playlist whose id starts with MPSPP.
            val kind = if (id.startsWith("MPSPP")) "podcast" else o.optString("kind", "playlist")
            return Item(kind, id, o.optString("params").takeIf { it != "null" }.orEmpty(), o.optString("title"), o.str("subtitle"), o.str("cover").ifEmpty { null })
        }
    }
}

/**
 * The page of an artist, an album, a playlist or a podcast, like Spotify's: a big picture tinted into the
 * background, the name, a green play button, the songs, and below them the albums and artists that go with it.
 * Pages open on top of each other and the back button peels them off one by one.
 */
class DetailView(context: Context, private val host: SongHost) : FrameLayout(context) {
    private val c = context
    private val stack = ArrayList<Item>()
    private val list = ListView(c).apply { divider = null }
    private val adapter = SongAdapter(c, host)
    private val footer = LinearLayout(c).apply { orientation = LinearLayout.VERTICAL }
    private var header: View? = null
    private var songs: List<Song> = emptyList()
    private var loadedId = ""

    init {
        setBackgroundColor(Ui.BG)
        visibility = GONE
        isClickable = true
        list.adapter = adapter
        list.addFooterView(footer, null, false)
        addView(list, LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT))
    }

    val isOpen get() = visibility == VISIBLE

    fun nowPlaying(id: String?) {
        adapter.current = id
    }

    fun open(item: Item) {
        stack += item
        if (!isOpen) {
            // Slides in from the right and fades up, like a page being pushed onto a stack.
            alpha = 0f
            translationX = c.dp(36).toFloat()
            visibility = VISIBLE
            animate().alpha(1f).translationX(0f).setDuration(240).setInterpolator(android.view.animation.DecelerateInterpolator(1.8f)).withLayer().start()
        }
        show(item)
    }

    /** One step back; true when there was a page to close. */
    fun back(): Boolean {
        if (!isOpen) return false
        stack.removeLastOrNull()
        if (stack.isEmpty()) {
            animate().alpha(0f).translationX(c.dp(36).toFloat()).setDuration(180).setInterpolator(android.view.animation.AccelerateInterpolator(1.4f)).withLayer()
                .withEndAction { visibility = GONE; translationX = 0f }.start()
        } else {
            show(stack.last())
        }
        return true
    }

    private fun show(item: Item) {
        loadedId = item.id
        songs = emptyList()
        adapter.songs = songs
        footer.removeAllViews()
        drawHeader(item, "")
        footer.addView(c.skeleton(7, round = item.kind == "artist"))
        Thread {
            val reply = runCatching {
                JSONObject(if (item.kind == "podcast") Core.podcast(item.id) else Core.browse(item.id, item.params))
            }.getOrNull()
            // An album page lists its songs without pictures or artists: they take the album's cover and the artist we came from.
            val parentArtist = stack.getOrNull(stack.size - 2)?.takeIf { it.kind == "artist" }?.title.orEmpty()
            val found = reply?.let(::songsOf).orEmpty().map { it.copy(cover = it.cover ?: item.cover, artist = it.artist.ifEmpty { parentArtist }) }
            val others = reply?.optJSONArray("items")?.let { array -> (0 until array.length()).mapNotNull { Item.fromJson(array.getJSONObject(it)) } }.orEmpty()
            post {
                if (loadedId != item.id) return@post
                songs = found
                adapter.songs = if (item.kind == "artist") found.take(10) else found
                val note = reply?.optString("error").orEmpty().ifEmpty { if (found.isEmpty() && others.isEmpty()) "Nothing to show here yet" else "" }
                drawHeader(item, note)
                drawFooter(item, others)
            }
        }.start()
    }

    private fun deep(color: Int): Int {
        val hsv = FloatArray(3)
        Color.colorToHSV(color, hsv)
        hsv[1] = (hsv[1] * 1.1f).coerceAtMost(1f)
        hsv[2] = (hsv[2] * 0.55f).coerceIn(0.18f, 0.5f)
        return Color.HSVToColor(hsv)
    }

    private fun drawHeader(item: Item, note: String) {
        header?.let { list.removeHeaderView(it) }
        val round = item.kind == "artist"
        val picture = ImageView(c).apply {
            scaleType = ImageView.ScaleType.CENTER_CROP
            background = roundRect(Ui.RAISED, if (round) c.dp(95) else c.dp(6))
            clipRounded(if (round) c.dp(95) else c.dp(6))
            elevation = c.dp(8).toFloat()
        }
        val box = LinearLayout(c).apply { orientation = LinearLayout.VERTICAL }
        fun paint(tint: Int) {
            box.background = GradientDrawable(GradientDrawable.Orientation.TOP_BOTTOM, intArrayOf(tint, Ui.BG))
        }
        paint(0xFF2A2A2A.toInt())
        Images.load(picture, item.cover, c.dp(380)) { bitmap -> paint(deep(Images.average(bitmap))) }
        val top = FrameLayout(c).apply {
            addView(picture, LayoutParams(c.dp(190), c.dp(190), Gravity.CENTER_HORIZONTAL or Gravity.TOP).apply { topMargin = c.dp(56) })
            addView(c.icon(R.drawable.ic_arrow_back, Color.WHITE, 44, 10).apply { setOnClickListener { host.back() } }, LayoutParams(c.dp(44), c.dp(44), Gravity.TOP or Gravity.START).apply { setMargins(c.dp(8), c.dp(8), 0, 0) })
        }
        box.addView(top, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, c.dp(262)))
        box.addView(c.label(if (round) 30f else 24f, Color.WHITE, bold = true, lines = 2).apply {
            text = item.title
            if (round) gravity = Gravity.CENTER_HORIZONTAL
            setPadding(c.dp(16), c.dp(10), c.dp(16), 0)
        })
        box.addView(c.label(13f, Ui.MUTED, lines = 3).apply {
            text = item.subtitle
            if (round) gravity = Gravity.CENTER_HORIZONTAL
            setPadding(c.dp(16), c.dp(4), c.dp(16), c.dp(4))
        })
        if (note.isNotEmpty()) box.addView(c.label(14f, Ui.MUTED, lines = 3).apply { text = note; setPadding(c.dp(16), c.dp(8), c.dp(16), c.dp(8)) })
        val play = ImageView(c).apply {
            setImageResource(R.drawable.ic_play)
            setColorFilter(Color.BLACK)
            setPadding(c.dp(14), c.dp(14), c.dp(14), c.dp(14))
            background = GradientDrawable().apply { shape = GradientDrawable.OVAL; setColor(Ui.GREEN) }
            isClickable = true
            setOnClickListener { playAll(item, shuffle = false) }
        }
        val actions = LinearLayout(c).apply {
            gravity = Gravity.CENTER_VERTICAL
            setPadding(c.dp(8), c.dp(8), c.dp(16), c.dp(8))
            addView(c.icon(R.drawable.ic_download, 0xCCFFFFFF.toInt(), 44, 10).apply { setOnClickListener { downloadAll(item) } })
            addView(c.icon(R.drawable.ic_add, 0xCCFFFFFF.toInt(), 44, 10).apply { setOnClickListener { saveAsPlaylist(item) } })
            addView(View(c), LinearLayout.LayoutParams(0, 1, 1f))
            addView(c.icon(R.drawable.ic_shuffle, Color.WHITE, 44, 10).apply { setOnClickListener { playAll(item, shuffle = true) } })
            addView(play, LinearLayout.LayoutParams(c.dp(56), c.dp(56)))
        }
        box.addView(actions)
        if (item.kind == "artist" && songs.isNotEmpty()) box.addView(c.label(20f, Color.WHITE, bold = true).apply { text = "Popular"; setPadding(c.dp(16), c.dp(8), c.dp(16), c.dp(4)) })
        header = box
        list.addHeaderView(box, null, false)
        list.adapter = adapter
    }

    private fun playAll(item: Item, shuffle: Boolean) {
        if (songs.isEmpty()) return
        Playback.nextSource = item.title
        host.play(if (shuffle) songs.shuffled() else songs, 0)
    }

    private fun saveAsPlaylist(item: Item) {
        if (songs.isEmpty()) return
        val playlist = Library.createPlaylist(item.title)
        songs.forEach { Library.addTo(playlist, it) }
        c.say("Saved \"${item.title}\" to Your Library")
    }

    private fun downloadAll(item: Item) {
        if (songs.isEmpty()) return
        Downloads.queue(c, songs)
        c.say("Downloading ${songs.size} songs for offline")
    }

    /** Albums and similar artists under the songs. */
    private fun drawFooter(item: Item, others: List<Item>) {
        footer.removeAllViews()
        val groups = linkedMapOf("album" to "Discography", "artist" to "Fans also like", "playlist" to "Playlists", "podcast" to "Podcasts")
        for ((kind, title) in groups) {
            val ofKind = others.filter { it.kind == kind && it.id != item.id }
            if (ofKind.isEmpty()) continue
            footer.addView(c.label(20f, Color.WHITE, bold = true).apply { text = title; setPadding(c.dp(16), c.dp(22), c.dp(16), c.dp(10)) })
            val row = LinearLayout(c).apply { setPadding(c.dp(16), 0, c.dp(4), 0) }
            ofKind.take(20).forEach { other ->
                val art = ImageView(c).apply {
                    scaleType = ImageView.ScaleType.CENTER_CROP
                    background = roundRect(Ui.RAISED, if (kind == "artist") c.dp(66) else c.dp(6))
                    clipRounded(if (kind == "artist") c.dp(66) else c.dp(6))
                }
                Images.load(art, other.cover, c.dp(260))
                row.addView(
                    LinearLayout(c).apply {
                        orientation = LinearLayout.VERTICAL
                        addView(art, LinearLayout.LayoutParams(c.dp(132), c.dp(132)))
                        addView(c.label(13f, Color.WHITE, bold = true, lines = 2).apply { text = other.title; setPadding(0, c.dp(6), 0, 0); if (kind == "artist") gravity = Gravity.CENTER_HORIZONTAL })
                        addView(c.label(12f, Ui.MUTED).apply { text = other.subtitle; if (kind == "artist") gravity = Gravity.CENTER_HORIZONTAL })
                        setOnClickListener { host.open(other) }
                    },
                    LinearLayout.LayoutParams(c.dp(132), ViewGroup.LayoutParams.WRAP_CONTENT).apply { marginEnd = c.dp(12) },
                )
            }
            footer.addView(HorizontalScrollView(c).apply { isHorizontalScrollBarEnabled = false; addView(row) })
        }
        footer.addView(View(c), LinearLayout.LayoutParams(1, c.dp(120)))
    }
}
