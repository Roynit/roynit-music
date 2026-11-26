package org.roynit.music

import android.content.Context
import android.graphics.Color
import android.view.Gravity
import android.view.View
import android.view.ViewGroup
import android.widget.BaseAdapter
import android.widget.ImageView
import android.widget.LinearLayout
import android.widget.TextView

/** What a row of songs needs from the screen around it. */
interface SongHost {
    fun play(songs: List<Song>, index: Int)
    fun menu(anchor: View, song: Song)

    /** Opens Search and looks for [query]. */
    fun search(query: String)

    /** Opens the page of an artist, album, playlist or podcast. */
    fun open(item: Item)

    /** Looks the artist up by name and opens their page. */
    fun openArtist(name: String)

    /** Opens the side menu that your picture leads to. */
    fun drawer()

    /** Closes the page on top, if there is one. */
    fun back()

    /** Lets the user crop a picture, then calls [done] with the result (null when cancelled). */
    fun crop(uri: android.net.Uri, circle: Boolean, outW: Int, outH: Int, done: (android.graphics.Bitmap?) -> Unit)
}

/** Rows of songs with a small cover, the title, the artist, a heart and a menu. */
class SongAdapter(private val context: Context, private val host: SongHost) : BaseAdapter() {
    /** Search results say where each song comes from (YT, SP, SC) before the artist, as on the PC app. */
    var showSource = false

    var songs: List<Song> = emptyList()
        set(value) {
            field = value
            notifyDataSetChanged()
        }

    /** The song that is playing now; its title turns green. */
    var current: String? = null
        set(value) {
            if (field != value) {
                field = value
                notifyDataSetChanged()
            }
        }

    private val heartOn: android.graphics.drawable.Drawable? by lazy { context.getDrawable(R.drawable.ic_heart) }
    private val heartOff: android.graphics.drawable.Drawable? by lazy { context.getDrawable(R.drawable.ic_heart_outline) }
    private val arrow: android.graphics.drawable.Drawable? by lazy {
        context.getDrawable(R.drawable.ic_download)?.mutate()?.apply { setTint(Ui.GREEN); setBounds(0, 0, context.dp(14), context.dp(14)) }
    }

    private class Row(val cover: ImageView, val title: TextView, val artist: TextView, val heart: ImageView, val more: ImageView) {
        /** What this row last showed, so a bind only touches the parts that changed. */
        var shownId: String? = null
        var liked: Boolean? = null
        var saved: Boolean? = null
        var current: Boolean? = null
    }

    override fun getCount() = songs.size
    override fun getItem(position: Int) = songs[position]
    override fun getItemId(position: Int) = position.toLong()

    override fun getView(position: Int, convert: View?, parent: ViewGroup): View {
        val view = convert ?: build()
        val row = view.tag as Row
        val song = songs[position]
        val isCurrent = song.id == current
        val saved = Downloads.has(song.id)
        val liked = Library.isFavorite(song.id)
        val same = row.shownId == song.id
        if (!same || row.current != isCurrent) {
            row.title.text = song.title
            row.title.setTextColor(if (isCurrent) Ui.GREEN else Color.WHITE)
        }
        if (!same || row.saved != saved) {
            row.artist.text = if (showSource) {
                android.text.SpannableStringBuilder(song.tag + "  ").apply {
                    setSpan(android.text.style.ForegroundColorSpan(song.tagColor), 0, 2, 0)
                    setSpan(android.text.style.StyleSpan(android.graphics.Typeface.BOLD), 0, 2, 0)
                    setSpan(android.text.style.RelativeSizeSpan(0.8f), 0, 2, 0)
                    append(song.artist)
                }
            } else {
                song.artist
            }
            // A small green arrow marks songs that are saved on the phone.
            row.artist.setCompoundDrawables(if (saved) arrow else null, null, null, null)
            row.artist.compoundDrawablePadding = context.dp(4)
        }
        if (!same) Images.load(row.cover, song.cover, context.dp(48))
        // Setting an icon inflates a vector each time, which adds up in a long list: only change it when it changes.
        if (!same || row.liked != liked) {
            row.heart.setImageDrawable(if (liked) heartOn else heartOff)
            row.heart.setColorFilter(if (liked) Ui.GREEN else Ui.MUTED)
        }
        row.shownId = song.id
        row.current = isCurrent
        row.saved = saved
        row.liked = liked
        row.heart.setOnClickListener { Library.toggleFavorite(song) }
        row.more.setOnClickListener { host.menu(it, song) }
        view.setOnClickListener { host.play(songs, position) }
        return view
    }

    private fun build(): View {
        val c = context
        val cover = ImageView(c).apply {
            scaleType = ImageView.ScaleType.CENTER_CROP
            background = roundRect(Ui.RAISED, c.dp(6))
            clipRounded(c.dp(6))
            layoutParams = LinearLayout.LayoutParams(c.dp(48), c.dp(48))
        }
        val title = c.label(15f)
        val artist = c.label(13f, Ui.MUTED).apply { setPadding(0, c.dp(2), 0, 0) }
        val texts = LinearLayout(c).apply {
            orientation = LinearLayout.VERTICAL
            layoutParams = LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f).apply { marginStart = c.dp(12) }
            addView(title)
            addView(artist)
        }
        val heart = c.icon(R.drawable.ic_heart_outline, Ui.MUTED, 40, 9)
        val more = c.icon(R.drawable.ic_more, Ui.MUTED, 40, 9)
        return LinearLayout(c).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
            setPadding(c.dp(16), c.dp(7), c.dp(8), c.dp(7))
            background = pressable()
            addView(cover)
            addView(texts)
            addView(heart)
            addView(more)
            tag = Row(cover, title, artist, heart, more)
        }
    }
}
