package org.roynit.music

import android.content.Context
import android.graphics.Color
import android.graphics.drawable.ClipDrawable
import android.graphics.drawable.GradientDrawable
import android.graphics.drawable.LayerDrawable
import android.view.Gravity
import android.view.MotionEvent
import android.view.View
import android.view.ViewConfiguration
import android.view.ViewGroup
import android.view.animation.DecelerateInterpolator
import android.widget.BaseAdapter
import android.widget.FrameLayout
import android.widget.ImageView
import android.widget.LinearLayout
import android.widget.ListView
import android.widget.ScrollView
import android.widget.TextView
import androidx.media3.common.C
import androidx.media3.common.Player
import org.json.JSONObject

/**
 * The full player, laid out like Spotify's: "Playing from" at the top, the cover edge to edge, the song and a
 * heart, a thin seek bar, big controls, and below the fold the lyrics and what is next. Everything is tinted with
 * the cover's colour. It slides up over the other screens and can be dragged back down; everything that moves is a
 * translation or a fade, which the GPU does on its own.
 */
class NowPlayingView(context: Context, private val host: SongHost, private val onClose: () -> Unit) : FrameLayout(context) {
    private val c = context
    private val cover = ImageView(c).apply {
        scaleType = ImageView.ScaleType.CENTER_CROP
        background = roundRect(Ui.RAISED, c.dp(8))
        clipRounded(c.dp(8))
        elevation = c.dp(12).toFloat()
    }
    private val title = c.label(22f, Color.WHITE, bold = true)
    private val artist = c.label(16f, 0xCCFFFFFF.toInt())
    private val heart = c.icon(R.drawable.ic_heart_outline, Color.WHITE, 48, 11)
    private val seek = Scrubber(c)
    private val elapsed = c.label(12f, 0xB3FFFFFF.toInt())
    private val remaining = c.label(12f, 0xB3FFFFFF.toInt())
    private val shuffle = c.icon(R.drawable.ic_shuffle, Color.WHITE, 48, 11)
    private val prev = c.icon(R.drawable.ic_prev, Color.WHITE, 56, 12)
    private val play = ImageView(c).apply {
        setImageResource(R.drawable.ic_play)
        setColorFilter(Color.BLACK)
        setPadding(c.dp(16), c.dp(16), c.dp(16), c.dp(16))
        background = GradientDrawable().apply { shape = GradientDrawable.OVAL; setColor(Color.WHITE) }
        isClickable = true
    }
    private val next = c.icon(R.drawable.ic_next, Color.WHITE, 56, 12)
    private val repeat = c.icon(R.drawable.ic_repeat, Color.WHITE, 48, 11)
    private val playingFrom = c.label(12f, Color.WHITE, bold = true).apply { gravity = Gravity.CENTER }
    private val playingKind = c.label(10f, 0xCCFFFFFF.toInt(), bold = true)
    private val content = LinearLayout(c).apply { orientation = LinearLayout.VERTICAL }
    private val scroll = ScrollView(c).apply { isVerticalScrollBarEnabled = false; addView(content) }
    private val lyricCard = LinearLayout(c).apply { orientation = LinearLayout.VERTICAL }
    private val lyricPreview = LinearLayout(c).apply { orientation = LinearLayout.VERTICAL }
    private val nextCard = LinearLayout(c).apply { orientation = LinearLayout.VERTICAL }
    private val queueList = ListView(c).apply { divider = null; setBackgroundColor(Ui.BG) }
    private val queuePage = LinearLayout(c).apply { orientation = LinearLayout.VERTICAL; setBackgroundColor(Ui.BG); visibility = View.GONE; isClickable = true }
    private val lyricsPage = FrameLayout(c).apply { visibility = View.GONE; isClickable = true }
    private val lyricsList = LinearLayout(c).apply { orientation = LinearLayout.VERTICAL; setPadding(c.dp(24), c.dp(8), c.dp(24), c.dp(200)) }
    private val lyricsScroll = ScrollView(c).apply { isVerticalScrollBarEnabled = false; addView(lyricsList) }
    private val queueAdapter = QueueAdapter()
    private val lyricsTitle = c.label(14f, Color.WHITE, bold = true)
    private val lyricSeek = Scrubber(c)
    private val lyricPlay = ImageView(c).apply {
        setImageResource(R.drawable.ic_play)
        setColorFilter(Color.BLACK)
        setPadding(c.dp(12), c.dp(12), c.dp(12), c.dp(12))
        background = GradientDrawable().apply { shape = GradientDrawable.OVAL; setColor(Color.WHITE) }
        isClickable = true
    }
    private val lyricsArtist = c.label(12f, 0xCCFFFFFF.toInt())
    private var dragging = false
    private var startY = 0f
    private var shown = false
    private var tone = 0xFF3A3A3A.toInt()
    private val slop = ViewConfiguration.get(c).scaledTouchSlop

    // lyrics state
    private val lyricTimes = ArrayList<Double>()
    private val lyricTexts = ArrayList<String>()
    private var lyricsFor: String? = null
    private var lyricsReady = false
    private var activeLine = -1
    private var plainLyrics = ""

    // Word mode: when a provider has word timings, the words of the line being sung light up one by one.
    private class LyricWord(val time: Double, val text: String)
    private val lyricWords = ArrayList<List<LyricWord>>()
    private val prefs = c.getSharedPreferences("roy", android.content.Context.MODE_PRIVATE)
    private var wordMode = prefs.getBoolean("lyric_words", true)
    private var sungWords = -1
    private val wordChip = c.label(12f, Color.WHITE, bold = true).apply {
        text = "Words"
        gravity = Gravity.CENTER
        setPadding(c.dp(8), c.dp(6), c.dp(8), c.dp(6))
    }
    private val wordTick = object : Runnable {
        override fun run() {
            highlightWords()
            if (lyricsPage.visibility == View.VISIBLE && wordMode && lyricWords.any { it.isNotEmpty() }) postDelayed(this, 80)
        }
    }

    init {
        setBackgroundColor(Ui.BG)
        visibility = GONE
        isClickable = true
        val pad = c.dp(24)

        val top = LinearLayout(c).apply {
            gravity = Gravity.CENTER_VERTICAL
            addView(c.icon(R.drawable.ic_chevron_down, Color.WHITE, 48, 11).apply { setOnClickListener { close() } })
            addView(
                LinearLayout(c).apply {
                    orientation = LinearLayout.VERTICAL
                    gravity = Gravity.CENTER
                    addView(playingKind.apply { gravity = Gravity.CENTER; letterSpacing = 0.08f })
                    addView(playingFrom.apply { setPadding(0, c.dp(2), 0, 0) })
                },
                LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f),
            )
            addView(c.icon(R.drawable.ic_more, Color.WHITE, 48, 11).apply { setOnClickListener { moreMenu(it) } })
        }
        val info = LinearLayout(c).apply {
            gravity = Gravity.CENTER_VERTICAL
            addView(
                LinearLayout(c).apply {
                    orientation = LinearLayout.VERTICAL
                    addView(title)
                    addView(artist.apply { setPadding(0, c.dp(3), 0, 0); setOnClickListener { Playback.song?.let { s -> close(); host.openArtist(s.artist.substringBefore(",")) } } })
                },
                LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f),
            )
            addView(heart.apply { setOnClickListener { Playback.song?.let(Library::toggleFavorite) } })
        }
        seek.onScrub = { value, done ->
            elapsed.text = clock(value.toLong())
            if (done) Playback.controller?.seekTo(value.toLong())
        }
        val times = LinearLayout(c).apply {
            addView(elapsed, LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f))
            addView(remaining)
            setPadding(c.dp(4), 0, c.dp(4), 0)
        }
        val controls = LinearLayout(c).apply {
            gravity = Gravity.CENTER
            addView(shuffle.apply { setOnClickListener { Playback.controller?.let { it.shuffleModeEnabled = !it.shuffleModeEnabled } } })
            addView(prev.apply { setOnClickListener { Playback.controller?.seekToPreviousMediaItem() } }, LinearLayout.LayoutParams(c.dp(56), c.dp(56)).apply { marginStart = c.dp(10) })
            addView(play.apply { setOnClickListener { Playback.toggle() } }, LinearLayout.LayoutParams(c.dp(68), c.dp(68)).apply { setMargins(c.dp(10), 0, c.dp(10), 0) })
            addView(next.apply { setOnClickListener { Playback.controller?.seekToNextMediaItem() } }, LinearLayout.LayoutParams(c.dp(56), c.dp(56)).apply { marginEnd = c.dp(10) })
            addView(repeat.apply { setOnClickListener { Playback.cycleRepeat() } })
        }
        // Spotify's bottom row: devices on the left, share and the queue on the right.
        val tools = LinearLayout(c).apply {
            gravity = Gravity.CENTER_VERTICAL
            addView(c.icon(R.drawable.ic_devices, 0xCCFFFFFF.toInt(), 44, 11).apply { setOnClickListener { sleepTimer() } })
            addView(View(c), LinearLayout.LayoutParams(0, 1, 1f))
            addView(c.icon(R.drawable.ic_share, 0xCCFFFFFF.toInt(), 44, 11).apply { setOnClickListener { share() } })
            addView(c.icon(R.drawable.ic_queue, 0xCCFFFFFF.toInt(), 44, 11).apply { setOnClickListener { openQueue() } })
        }

        val player = LinearLayout(c).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(pad, 0, pad, c.dp(8))
            addView(top, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, c.dp(56)).apply { setMargins(-c.dp(12), 0, -c.dp(12), 0) })
            addView(cover, LinearLayout.LayoutParams(0, 0))
            addView(info, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT).apply { topMargin = c.dp(28) })
            addView(seek, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT).apply { setMargins(-c.dp(8), c.dp(10), -c.dp(8), 0) })
            addView(times)
            addView(controls, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT).apply { topMargin = c.dp(10) })
            addView(tools, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT).apply { topMargin = c.dp(8) })
        }
        content.addView(player)

        // Below the fold: lyrics and what plays next, as rounded cards in the cover's colour.
        lyricCard.apply {
            setPadding(c.dp(18), c.dp(16), c.dp(18), c.dp(18))
            addView(LinearLayout(c).apply {
                gravity = Gravity.CENTER_VERTICAL
                addView(c.label(16f, Color.WHITE, bold = true).apply { text = "Lyrics" }, LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f))
                addView(c.label(12f, Color.WHITE, bold = true).apply {
                    text = "Show more"
                    background = roundRect(0x33000000, c.dp(14))
                    setPadding(c.dp(12), c.dp(6), c.dp(12), c.dp(6))
                })
            })
            addView(lyricPreview, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT).apply { topMargin = c.dp(12) })
            setOnClickListener { openLyrics() }
        }
        content.addView(lyricCard, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT).apply { setMargins(c.dp(16), c.dp(8), c.dp(16), 0) })
        nextCard.apply {
            background = roundRect(0xFF1E1E1E.toInt(), c.dp(10))
            setPadding(c.dp(18), c.dp(16), c.dp(18), c.dp(16))
        }
        content.addView(nextCard, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT).apply { setMargins(c.dp(16), c.dp(14), c.dp(16), c.dp(32)) })

        addView(scroll, LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT))
        buildQueuePage()
        buildLyricsPage()
    }

    // ---- the queue page ----

    private fun buildQueuePage() {
        queueList.adapter = queueAdapter
        queueList.setOnItemClickListener { _, _, position, _ -> Playback.controller?.seekTo(position, 0) }
        val bar = LinearLayout(c).apply {
            gravity = Gravity.CENTER_VERTICAL
            setPadding(c.dp(4), c.dp(8), c.dp(16), c.dp(8))
            addView(c.icon(R.drawable.ic_chevron_down, Color.WHITE, 48, 11).apply { setOnClickListener { closeQueue() } })
            addView(c.label(18f, Color.WHITE, bold = true).apply { text = "Queue"; gravity = Gravity.CENTER }, LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f))
            addView(View(c), LinearLayout.LayoutParams(c.dp(48), 1))
        }
        queuePage.addView(bar)
        queuePage.addView(queueList, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, 0, 1f))
        addView(queuePage, LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT))
    }

    private fun openQueue() {
        queueAdapter.notifyDataSetChanged()
        queueList.setSelection(Playback.controller?.currentMediaItemIndex ?: 0)
        queuePage.alpha = 0f
        queuePage.visibility = View.VISIBLE
        queuePage.animate().alpha(1f).setDuration(180).withLayer().start()
    }

    private fun closeQueue() {
        queuePage.animate().alpha(0f).setDuration(140).withLayer().withEndAction { queuePage.visibility = View.GONE }.start()
    }

    // ---- the lyrics page: big words on the cover's colour, the line being sung in white ----

    private fun buildLyricsPage() {
        val bar = LinearLayout(c).apply {
            gravity = Gravity.CENTER_VERTICAL
            setPadding(c.dp(4), c.dp(8), c.dp(16), c.dp(8))
            addView(c.icon(R.drawable.ic_chevron_down, Color.WHITE, 48, 11).apply { setOnClickListener { closeLyrics() } })
            addView(
                LinearLayout(c).apply {
                    orientation = LinearLayout.VERTICAL
                    gravity = Gravity.CENTER
                    addView(lyricsTitle.apply { gravity = Gravity.CENTER })
                    addView(lyricsArtist.apply { gravity = Gravity.CENTER })
                },
                LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f),
            )
            addView(wordChip.apply {
                alpha = if (wordMode) 1f else 0.5f
                background = roundRect(0x33FFFFFF, c.dp(14))
                setOnClickListener {
                    wordMode = !wordMode
                    prefs.edit().putBoolean("lyric_words", wordMode).apply()
                    alpha = if (wordMode) 1f else 0.5f
                    sungWords = -1
                    if (wordMode) { removeCallbacks(wordTick); post(wordTick) } else showPlainActive()
                }
            }, LinearLayout.LayoutParams(ViewGroup.LayoutParams.WRAP_CONTENT, ViewGroup.LayoutParams.WRAP_CONTENT))
        }
        val column = LinearLayout(c).apply {
            orientation = LinearLayout.VERTICAL
            addView(bar)
            addView(lyricsScroll, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, 0, 1f))
            // The words are never in the way of the music: a bar and the main buttons stay at the bottom.
            lyricSeek.onScrub = { value, done -> if (done) Playback.controller?.seekTo(value.toLong()) }
            addView(lyricSeek, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT).apply { setMargins(c.dp(16), 0, c.dp(16), 0) })
            addView(
                LinearLayout(c).apply {
                    gravity = Gravity.CENTER
                    setPadding(0, 0, 0, c.dp(12))
                    addView(c.icon(R.drawable.ic_prev, Color.WHITE, 52, 12).apply { setOnClickListener { Playback.controller?.seekToPreviousMediaItem() } })
                    addView(lyricPlay.apply { setOnClickListener { Playback.toggle() } }, LinearLayout.LayoutParams(c.dp(56), c.dp(56)).apply { setMargins(c.dp(14), 0, c.dp(14), 0) })
                    addView(c.icon(R.drawable.ic_next, Color.WHITE, 52, 12).apply { setOnClickListener { Playback.controller?.seekToNextMediaItem() } })
                },
            )
        }
        lyricsPage.addView(column, LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT))
        addView(lyricsPage, LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT))
    }

    private fun openLyrics() {
        lyricsTitle.text = title.text
        lyricsArtist.text = artist.text
        lyricsPage.setBackgroundColor(tone)
        redrawLyricsPage()
        lyricsPage.alpha = 0f
        lyricsPage.visibility = View.VISIBLE
        removeCallbacks(wordTick)
        post(wordTick)
        lyricsPage.animate().alpha(1f).setDuration(200).withLayer().start()
    }

    private fun closeLyrics() {
        lyricsPage.animate().alpha(0f).setDuration(150).withLayer().withEndAction { lyricsPage.visibility = View.GONE }.start()
    }

    private fun redrawLyricsPage() {
        lyricsList.removeAllViews()
        when {
            !lyricsReady -> lyricsList.addView(lyricLine("Looking for lyrics...", 20f, false))
            lyricTexts.isNotEmpty() -> lyricTexts.forEach { lyricsList.addView(lyricLine(it, 26f, true)) }
            else -> lyricsList.addView(lyricLine(plainLyrics.ifBlank { "No lyrics found for this song" }, 18f, false))
        }
        activeLine = -1
        followLyrics(Playback.controller?.currentPosition ?: 0)
    }

    private fun lyricLine(text: String, size: Float, bold: Boolean) = c.label(size, 0x80FFFFFF.toInt(), bold = bold).apply {
        this.text = text
        maxLines = Int.MAX_VALUE
        ellipsize = null
        setPadding(0, c.dp(10), 0, c.dp(10))
    }

    private fun loadLyrics() {
        val song = Playback.song ?: return
        if (lyricsFor == song.id) return
        lyricsFor = song.id
        lyricsReady = false
        lyricTimes.clear()
        lyricTexts.clear()
        lyricWords.clear()
        sungWords = -1
        plainLyrics = ""
        activeLine = -1
        drawLyricPreview()
        Thread {
            val reply = runCatching { JSONObject(Core.lyrics(song.title, song.artist, song.seconds.toDouble(), c.filesDir.path)) }.getOrDefault(JSONObject())
            val lines = reply.optJSONArray("lines")
            post {
                if (lyricsFor != song.id) return@post
                lyricTimes.clear()
                lyricTexts.clear()
                val synced = lines != null && lines.length() > 0 && lines.getJSONObject(lines.length() - 1).optDouble("time") > 0
                if (synced) {
                    for (i in 0 until lines!!.length()) {
                        val line = lines.getJSONObject(i)
                        lyricTimes += line.optDouble("time")
                        lyricTexts += line.optString("text")
                    }
                } else {
                    plainLyrics = reply.optString("text").ifBlank {
                        if (lines == null) "" else (0 until lines.length()).joinToString("\n") { lines.getJSONObject(it).optString("text") }
                    }
                    if (plainLyrics.isBlank() && reply.optBoolean("instrumental")) plainLyrics = "Instrumental"
                }
                lyricsReady = true
                drawLyricPreview()
                if (lyricsPage.visibility == View.VISIBLE) redrawLyricsPage()
                if (synced) fetchWords(song, lines!!)
            }
        }.start()
    }

    /** Asks for word timings in the background; the lines show up as before and light up word by word once they arrive. */
    private fun fetchWords(song: Song, lines: org.json.JSONArray) {
        Thread {
            val reply = runCatching { JSONObject(Core.lyricWords(song.title, song.artist, song.seconds.toDouble(), lines.toString())) }.getOrDefault(JSONObject())
            val merged = reply.optJSONArray("lines") ?: return@Thread
            val parsed = (0 until merged.length()).map { i ->
                val words = merged.getJSONObject(i).optJSONArray("words")
                (0 until (words?.length() ?: 0)).map { w -> words!!.getJSONObject(w).let { LyricWord(it.optDouble("time"), it.optString("text")) } }
            }
            post {
                if (lyricsFor != song.id || parsed.none { it.isNotEmpty() }) return@post
                lyricWords.clear()
                lyricWords.addAll(parsed)
                sungWords = -1
                removeCallbacks(wordTick)
                post(wordTick)
            }
        }.start()
    }

    private fun showPlainActive() {
        if (activeLine in 0 until lyricsList.childCount && activeLine < lyricTexts.size) (lyricsList.getChildAt(activeLine) as TextView).text = lyricTexts[activeLine]
    }

    /** Lights up the words of the line being sung that have started, leaving the rest dim. */
    private fun highlightWords() {
        if (!wordMode || activeLine !in 0 until lyricsList.childCount) return
        val words = lyricWords.getOrNull(activeLine)?.takeIf { it.isNotEmpty() } ?: return
        val position = (Playback.controller?.currentPosition ?: return) / 1000.0
        val sung = words.count { it.time <= position }
        if (sung == sungWords) return
        sungWords = sung
        val line = lyricTexts[activeLine]
        val text = android.text.SpannableString(line)
        var cursor = 0
        var split = 0
        words.forEachIndexed { i, word ->
            val at = line.indexOf(word.text.trim(), cursor)
            if (at >= 0) {
                cursor = at + word.text.trim().length
                if (i < sung) split = cursor
            }
        }
        text.setSpan(android.text.style.ForegroundColorSpan(Color.WHITE), 0, split, android.text.Spanned.SPAN_EXCLUSIVE_EXCLUSIVE)
        text.setSpan(android.text.style.ForegroundColorSpan(0x66FFFFFF), split, line.length, android.text.Spanned.SPAN_EXCLUSIVE_EXCLUSIVE)
        (lyricsList.getChildAt(activeLine) as TextView).text = text
    }

    /** The card below the player: a few lines around the one being sung. */
    private fun drawLyricPreview() {
        lyricPreview.removeAllViews()
        fun add(text: String, on: Boolean) = lyricPreview.addView(c.label(if (on) 19f else 17f, if (on) Color.WHITE else 0x99FFFFFF.toInt(), bold = true, lines = 3).apply {
            this.text = text
            setPadding(0, c.dp(4), 0, c.dp(4))
        })
        when {
            !lyricsReady -> add("Looking for lyrics...", false)
            lyricTexts.isNotEmpty() -> {
                val now = activeLine.coerceAtLeast(0)
                for (i in now until minOf(now + 4, lyricTexts.size)) add(lyricTexts[i], i == activeLine)
            }
            else -> add(plainLyrics.lineSequence().filter { it.isNotBlank() }.take(4).joinToString("\n").ifBlank { "No lyrics found for this song" }, false)
        }
    }

    private fun followLyrics(position: Long) {
        if (lyricTimes.isEmpty()) return
        val now = lyricTimes.indexOfLast { it * 1000 <= position }
        if (now == activeLine) return
        if (activeLine in 0 until lyricsList.childCount) {
            (lyricsList.getChildAt(activeLine) as TextView).setTextColor(0x80FFFFFF.toInt())
            showPlainActive()
        }
        activeLine = now
        sungWords = -1
        if (now >= 0 && now < lyricsList.childCount) {
            (lyricsList.getChildAt(now) as TextView).setTextColor(Color.WHITE)
            highlightWords()
            if (lyricsPage.visibility == View.VISIBLE) lyricsScroll.smoothScrollTo(0, (lyricsList.getChildAt(now).top - lyricsScroll.height / 3).coerceAtLeast(0))
        }
        drawLyricPreview()
    }

    // ---- small menus ----

    private fun moreMenu(anchor: View) {
        val song = Playback.song ?: return
        val menu = SheetMenu(c, anchor)
        menu.menu.add(0, 1, 0, if (Library.isFavorite(song.id)) "Remove from liked songs" else "Add to liked songs")
        menu.menu.add(0, 2, 1, "Start radio")
        menu.menu.add(0, 3, 2, "Sleep timer")
        menu.menu.add(0, 4, 3, "Share")
        menu.setOnMenuItemClickListener {
            when (it.itemId) {
                1 -> Library.toggleFavorite(song)
                2 -> Thread {
                    val songs = listOf(song) + songsOf(JSONObject(Core.radio(song.id, 40))).filter { s -> s.id != song.id }
                    post { if (songs.size > 1) Playback.play(songs, 0) }
                }.start()
                3 -> sleepTimer()
                4 -> share()
            }
            true
        }
        menu.show()
    }

    private fun share() {
        val song = Playback.song ?: return
        val send = android.content.Intent(android.content.Intent.ACTION_SEND)
            .setType("text/plain")
            .putExtra(android.content.Intent.EXTRA_TEXT, "${song.title} - ${song.artist}\nhttps://music.youtube.com/watch?v=${song.id}")
        c.startActivity(android.content.Intent.createChooser(send, "Share").addFlags(android.content.Intent.FLAG_ACTIVITY_NEW_TASK))
    }

    /** Stops the music after a while; this is the moon on Spotify's player. */
    private fun sleepTimer() {
        val names = listOf("Off", "5 minutes", "15 minutes", "30 minutes", "45 minutes", "1 hour")
        val minutes = intArrayOf(0, 5, 15, 30, 45, 60)
        Sheet.show(c as android.app.Activity, "Sleep timer", "The music stops by itself", null, names.mapIndexed { i, name ->
            Sheet.Option(name, R.drawable.ic_moon) {
                Playback.sleepIn(minutes[i])
                c.say(if (minutes[i] == 0) "Sleep timer off" else "Music stops in $name")
            }
        })
    }

    // ---- sliding in and out ----

    override fun onSizeChanged(w: Int, h: Int, oldw: Int, oldh: Int) {
        super.onSizeChanged(w, h, oldw, oldh)
        sizeCover(w, h)
        if (!shown) translationY = h.toFloat()
    }

    /** The cover is a square as wide as the screen allows. */
    private fun sizeCover(w: Int, h: Int) {
        if (w <= 0) return
        // As wide as the screen allows, but never so tall that the controls fall off the bottom of a short screen.
        val side = minOf(w - c.dp(48), h - c.dp(445)).coerceAtLeast(c.dp(120))
        val params = cover.layoutParams as LinearLayout.LayoutParams
        if (params.width != side || params.height != side) {
            params.width = side
            params.height = side
            params.topMargin = c.dp(14)
            params.gravity = Gravity.CENTER_HORIZONTAL
            cover.layoutParams = params
        }
    }

    val isOpen get() = shown

    fun open() {
        shown = true
        visibility = VISIBLE
        scroll.scrollTo(0, 0)
        sizeCover(width.takeIf { it > 0 } ?: c.resources.displayMetrics.widthPixels, height.takeIf { it > 0 } ?: c.resources.displayMetrics.heightPixels)
        refresh()
        animate().translationY(0f).setDuration(300).setInterpolator(DecelerateInterpolator(1.6f)).withLayer().start()
    }

    fun close() {
        if (queuePage.visibility == View.VISIBLE) return closeQueue()
        if (lyricsPage.visibility == View.VISIBLE) return closeLyrics()
        shown = false
        animate().translationY(height.toFloat()).setDuration(240).withLayer().withEndAction { if (!shown) visibility = GONE }.start()
        onClose()
    }

    /** Pulling the screen down while it is at the top puts it away. */
    override fun onInterceptTouchEvent(event: MotionEvent): Boolean {
        when (event.actionMasked) {
            MotionEvent.ACTION_DOWN -> {
                startY = event.rawY
                dragging = false
            }
            MotionEvent.ACTION_MOVE -> if (!dragging && event.rawY - startY > slop * 2 && scroll.scrollY == 0 &&
                queuePage.visibility != View.VISIBLE && lyricsPage.visibility != View.VISIBLE && event.y < height * 0.7f
            ) {
                dragging = true
            }
        }
        return dragging
    }

    override fun onTouchEvent(event: MotionEvent): Boolean {
        when (event.actionMasked) {
            MotionEvent.ACTION_MOVE -> if (dragging) translationY = (event.rawY - startY).coerceAtLeast(0f)
            MotionEvent.ACTION_UP, MotionEvent.ACTION_CANCEL -> if (dragging) {
                dragging = false
                if (translationY > height * 0.22f) close() else animate().translationY(0f).setDuration(200).withLayer().start()
            }
        }
        return true
    }

    private var shownId: String? = null

    /** Everything on screen that depends on the player; cheap enough to run on every player event. */
    fun refresh() {
        val controller = Playback.controller ?: return
        val song = Playback.song
        title.text = song?.title.orEmpty()
        artist.text = song?.artist.orEmpty()
        playingKind.text = if (Playback.sourceName.isEmpty()) "NOW PLAYING" else "PLAYING FROM"
        playingFrom.text = Playback.sourceName.ifEmpty { song?.artist.orEmpty() }
        if (song?.id != shownId) {
            shownId = song?.id
            Images.load(cover, song?.cover, c.dp(420)) { bitmap -> tint(Images.average(bitmap)) }
            queueAdapter.notifyDataSetChanged()
            loadLyrics()
        }
        drawNext(controller)
        val liked = song != null && Library.isFavorite(song.id)
        heart.setImageResource(if (liked) R.drawable.ic_heart else R.drawable.ic_heart_outline)
        heart.setColorFilter(if (liked) Ui.GREEN else Color.WHITE)
        val playing = controller.isPlaying || controller.playbackState == Player.STATE_BUFFERING && controller.playWhenReady
        play.setImageResource(if (playing) R.drawable.ic_pause else R.drawable.ic_play)
        lyricPlay.setImageResource(if (playing) R.drawable.ic_pause else R.drawable.ic_play)
        shuffle.setColorFilter(if (controller.shuffleModeEnabled) Ui.GREEN else Color.WHITE)
        repeat.setColorFilter(if (controller.repeatMode != Player.REPEAT_MODE_OFF) Ui.GREEN else Color.WHITE)
        repeat.alpha = if (controller.repeatMode == Player.REPEAT_MODE_ONE) 0.6f else 1f
        progress()
    }

    /** "Next in queue": the next two songs and a way into the full queue. */
    private fun drawNext(controller: androidx.media3.session.MediaController) {
        nextCard.removeAllViews()
        val index = controller.currentMediaItemIndex
        val upcoming = (index + 1 until controller.mediaItemCount).take(2).map { controller.getMediaItemAt(it) }
        nextCard.addView(LinearLayout(c).apply {
            gravity = Gravity.CENTER_VERTICAL
            addView(c.label(16f, Color.WHITE, bold = true).apply { text = "Next in queue" }, LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f))
            addView(c.label(12f, Color.WHITE, bold = true).apply {
                text = "Open queue"
                background = roundRect(0x33FFFFFF, c.dp(14))
                setPadding(c.dp(12), c.dp(6), c.dp(12), c.dp(6))
            })
            setOnClickListener { openQueue() }
        })
        if (upcoming.isEmpty()) {
            nextCard.addView(c.label(14f, Ui.MUTED).apply { text = "Nothing queued. Roy will pick songs like this one."; setPadding(0, c.dp(10), 0, 0) })
        }
        upcoming.forEach { item ->
            val row = LinearLayout(c).apply {
                gravity = Gravity.CENTER_VERTICAL
                setPadding(0, c.dp(10), 0, 0)
            }
            val art = ImageView(c).apply {
                scaleType = ImageView.ScaleType.CENTER_CROP
                background = roundRect(Ui.RAISED, c.dp(4))
                clipRounded(c.dp(4))
            }
            Images.load(art, item.mediaMetadata.artworkUri?.toString(), c.dp(48))
            row.addView(art, LinearLayout.LayoutParams(c.dp(48), c.dp(48)))
            row.addView(
                LinearLayout(c).apply {
                    orientation = LinearLayout.VERTICAL
                    addView(c.label(15f, Color.WHITE, bold = true).apply { text = item.mediaMetadata.title?.toString().orEmpty() })
                    addView(c.label(13f, Ui.MUTED).apply { text = item.mediaMetadata.artist?.toString().orEmpty() })
                },
                LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f).apply { marginStart = c.dp(12) },
            )
            nextCard.addView(row)
        }
    }

    /** The time bar, a few times a second while the screen is open. */
    fun progress() {
        val controller = Playback.controller ?: return
        if (seek.dragging) return
        val known = controller.duration.takeIf { it != C.TIME_UNSET && it > 0 } ?: ((Playback.song?.seconds ?: 0) * 1000)
        seek.max = known.toInt().coerceAtLeast(1)
        seek.progress = if (known > 0) controller.currentPosition.toInt() else 0
        lyricSeek.max = seek.max
        if (!lyricSeek.dragging) lyricSeek.progress = seek.progress
        elapsed.text = clock(controller.currentPosition)
        remaining.text = if (known > 0) "-" + clock((known - controller.currentPosition).coerceAtLeast(0)) else ""
        followLyrics(controller.currentPosition)
    }

    private fun clock(ms: Long): String {
        val s = (ms / 1000).coerceAtLeast(0)
        return "%d:%02d".format(s / 60, s % 60)
    }

    /** Spotify colours the whole player with the cover: the top is the cover's colour, fading into the app's black. */
    private fun tint(color: Int) {
        val hsv = FloatArray(3)
        Color.colorToHSV(color, hsv)
        hsv[1] = (hsv[1] * 1.15f).coerceAtMost(1f)
        hsv[2] = (hsv[2] * 0.62f).coerceIn(0.2f, 0.55f)
        tone = Color.HSVToColor(hsv)
        scroll.background = GradientDrawable(GradientDrawable.Orientation.TOP_BOTTOM, intArrayOf(tone, Ui.BG, Ui.BG))
        lyricCard.background = roundRect(Color.HSVToColor(floatArrayOf(hsv[0], hsv[1], (hsv[2] + 0.08f).coerceAtMost(0.7f))), c.dp(10))
        if (lyricsPage.visibility == View.VISIBLE) lyricsPage.setBackgroundColor(tone)
    }

    /** The queue, with the song that is playing marked. */
    private inner class QueueAdapter : BaseAdapter() {
        override fun getCount() = Playback.controller?.mediaItemCount ?: 0
        override fun getItem(position: Int) = Playback.controller?.getMediaItemAt(position)
        override fun getItemId(position: Int) = position.toLong()
        override fun getView(position: Int, convert: View?, parent: ViewGroup): View {
            val row = (convert?.tag as? Array<*>)?.let { convert } ?: build()
            @Suppress("UNCHECKED_CAST") val parts = row.tag as Array<View>
            val item = Playback.controller?.getMediaItemAt(position)
            val current = position == Playback.controller?.currentMediaItemIndex
            Images.load(parts[0] as ImageView, item?.mediaMetadata?.artworkUri?.toString(), c.dp(48))
            (parts[1] as TextView).apply {
                text = item?.mediaMetadata?.title?.toString().orEmpty()
                setTextColor(if (current) Ui.GREEN else Color.WHITE)
            }
            (parts[2] as TextView).text = item?.mediaMetadata?.artist?.toString().orEmpty()
            return row
        }

        private fun build(): View {
            val art = ImageView(c).apply {
                scaleType = ImageView.ScaleType.CENTER_CROP
                background = roundRect(Ui.RAISED, c.dp(4))
                clipRounded(c.dp(4))
            }
            val name = c.label(15f, Color.WHITE, bold = true)
            val by = c.label(13f, Ui.MUTED)
            return LinearLayout(c).apply {
                gravity = Gravity.CENTER_VERTICAL
                setPadding(c.dp(16), c.dp(8), c.dp(16), c.dp(8))
                background = pressable()
                addView(art, LinearLayout.LayoutParams(c.dp(48), c.dp(48)))
                addView(
                    LinearLayout(c).apply {
                        orientation = LinearLayout.VERTICAL
                        addView(name)
                        addView(by)
                    },
                    LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f).apply { marginStart = c.dp(12) },
                )
                tag = arrayOf<View>(art, name, by)
            }
        }
    }
}
