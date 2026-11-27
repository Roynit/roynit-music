package org.roynit.music

import android.Manifest
import android.app.Activity
import android.net.Uri
import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.graphics.Color
import android.os.Handler
import android.os.Looper
import android.text.Editable
import android.text.InputType
import android.text.TextWatcher
import android.view.Gravity
import android.view.HapticFeedbackConstants
import android.view.MotionEvent
import android.view.View
import android.view.ViewGroup
import android.view.inputmethod.InputMethodManager
import android.widget.EditText
import android.widget.FrameLayout
import android.widget.ImageView
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.TextView
import org.json.JSONObject

/**
 * A conversation laid out like Messenger's: full screen, so the keyboard only lifts the message box; bubbles with the
 * other person's picture; replies that quote what they answer; reactions you give by holding a message, with the
 * reply/copy/share bar taking the message box's place; an emoji panel and a one-tap thumbs up beside the box.
 */
class ChatView(private val c: Context, private val onProfile: (JSONObject) -> Unit, private val onClosed: () -> Unit) : FrameLayout(c) {
    private var person: JSONObject? = null
    private val messages = ArrayList<JSONObject>()
    private val reactions = HashMap<Long, List<Pair<String, Boolean>>>()
    private var last = 0L
    private var replyingTo: JSONObject? = null
    private var selected: JSONObject? = null
    private var pickingReaction: JSONObject? = null
    private var choosingQuick = false
    private val handler = Handler(Looper.getMainLooper())
    private val clock = java.text.SimpleDateFormat("HH:mm", java.util.Locale.getDefault())
    private val dayFormat = java.text.SimpleDateFormat("d MMMM yyyy", java.util.Locale.getDefault())
    private val quick = listOf("❤️", "😆", "😮", "😢", "😡", "👍")

    val isOpen get() = visibility == VISIBLE

    private val poll = object : Runnable {
        override fun run() {
            if (isOpen) { load(false); handler.postDelayed(this, 1_500) }
        }
    }

    // ---- the pieces ----
    private val header = LinearLayout(c).apply { gravity = Gravity.CENTER_VERTICAL; setPadding(c.dp(6), c.dp(8), c.dp(16), c.dp(8)) }
    private val list = LinearLayout(c).apply { orientation = LinearLayout.VERTICAL; setPadding(c.dp(10), c.dp(8), c.dp(10), c.dp(8)); clipChildren = false }
    private val scroll = ScrollView(c).apply { isVerticalScrollBarEnabled = false; clipChildren = false; addView(list) }
    private val holdLayer = FrameLayout(c).apply { visibility = GONE; setBackgroundColor(0xCC000000.toInt()); isClickable = true; setOnClickListener { dismissSelection() } }
    private val stage = FrameLayout(c).apply { addView(scroll, FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT)); addView(holdLayer, FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT)) }
    private val replyBar = LinearLayout(c).apply { gravity = Gravity.CENTER_VERTICAL; visibility = GONE; setPadding(c.dp(16), c.dp(8), c.dp(8), c.dp(8)); setBackgroundColor(Ui.SURFACE) }
    private val field = EditText(c).apply {
        hint = "Message"
        setHintTextColor(Ui.MUTED)
        setTextColor(Color.WHITE)
        background = null
        inputType = InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_FLAG_CAP_SENTENCES or InputType.TYPE_TEXT_FLAG_MULTI_LINE
        maxLines = 4
        setPadding(c.dp(18), c.dp(11), c.dp(4), c.dp(11))
    }
    private val thumb = c.label(26f, Color.WHITE).apply { text = "👍"; gravity = Gravity.CENTER; includeFontPadding = true }
    private val sendIcon = c.icon(R.drawable.ic_send, Color.BLACK, 40, 10).apply { background = roundRect(Color.WHITE, c.dp(20)); visibility = GONE }
    private val composer = LinearLayout(c).apply { gravity = Gravity.CENTER_VERTICAL; setPadding(c.dp(10), c.dp(6), c.dp(10), c.dp(8)) }
    private val actions = LinearLayout(c).apply { gravity = Gravity.CENTER; visibility = GONE; setBackgroundColor(0xFF1B1B1B.toInt()); setPadding(c.dp(8), c.dp(12), c.dp(8), c.dp(14)) }
    private val bottom = FrameLayout(c)
    private val panel = LinearLayout(c).apply { orientation = LinearLayout.VERTICAL; visibility = GONE; setBackgroundColor(Ui.SURFACE) }
    private val tools = LinearLayout(c).apply { gravity = Gravity.CENTER_VERTICAL }
    private val trayBox = FrameLayout(c).apply { visibility = GONE }
    private var tray: PhotoTray? = null
    private val recorder = VoiceRecorder(c)
    private var recording = false
    private val recordBar = LinearLayout(c).apply { gravity = Gravity.CENTER_VERTICAL; visibility = GONE; setPadding(c.dp(8), c.dp(8), c.dp(10), c.dp(10)) }
    private val recordWave = WaveView(c, 36).apply { played = Color.WHITE; rest = Color.WHITE }
    private val recordTimer = c.label(15f, Color.BLACK, bold = true).apply { text = "0:00" }
    private val recordPause = c.icon(R.drawable.ic_pause, Color.BLACK, 36, 8)
    private var live = FloatArray(36) { 0.15f }
    private val viewer = FrameLayout(c).apply { visibility = GONE; setBackgroundColor(Color.BLACK); isClickable = true }
    private val viewerImage = ImageView(c).apply { scaleType = ImageView.ScaleType.FIT_CENTER }
    private val viewerCount = c.label(14f, Color.WHITE, bold = true)
    private val album = FrameLayout(c).apply { visibility = GONE; setBackgroundColor(Ui.BG); isClickable = true }
    private val albumTitle = c.label(18f, Color.WHITE, bold = true)
    private val albumGrid = android.widget.GridLayout(c).apply { columnCount = 2 }
    private var shots: List<String> = emptyList()
    private var shot = 0


    init {
        setBackgroundColor(Ui.BG)
        visibility = GONE
        isClickable = true
        val column = LinearLayout(c).apply { orientation = LinearLayout.VERTICAL }
        column.addView(header)
        column.addView(View(c).apply { setBackgroundColor(0x22FFFFFF) }, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, 1))
        column.addView(stage, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, 0, 1f))
        column.addView(replyBar)

        val smile = c.icon(R.drawable.ic_emoji, Ui.MUTED, 40, 8).apply { setOnClickListener { togglePanel() } }
        val pill = LinearLayout(c).apply {
            gravity = Gravity.CENTER_VERTICAL
            background = roundRect(Ui.SURFACE, c.dp(24))
            addView(field, LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f))
            addView(smile)
        }
        val right = FrameLayout(c).apply {
            addView(thumb, FrameLayout.LayoutParams(c.dp(44), c.dp(44), Gravity.CENTER))
            addView(sendIcon, FrameLayout.LayoutParams(c.dp(40), c.dp(40), Gravity.CENTER))
        }
        buildMedia()
        composer.addView(tools)
        composer.addView(pill, LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f))
        composer.addView(right, LinearLayout.LayoutParams(c.dp(48), c.dp(48)).apply { marginStart = c.dp(6) })
        bottom.addView(composer, FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT))
        bottom.addView(actions, FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT))
        bottom.addView(recordBar, FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT))
        column.addView(bottom)
        column.addView(panel, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, c.dp(290)))
        column.addView(trayBox, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, c.dp(330)))
        addView(column, FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT))
        addView(album, FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT))
        addView(viewer, FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT))

        field.addTextChangedListener(object : TextWatcher {
            override fun afterTextChanged(s: Editable?) {
                val has = !s.isNullOrBlank()
                sendIcon.visibility = if (has) VISIBLE else GONE
                thumb.visibility = if (has) GONE else VISIBLE
                tools.visibility = if (has || field.hasFocus()) GONE else VISIBLE
            }
            override fun beforeTextChanged(s: CharSequence?, start: Int, count: Int, after: Int) {}
            override fun onTextChanged(s: CharSequence?, start: Int, before: Int, count: Int) {}
        })
        field.setOnTouchListener { _, event -> if (event.action == MotionEvent.ACTION_DOWN && (panel.visibility == VISIBLE || trayBox.visibility == VISIBLE)) hideTrays(); false }
        field.setOnFocusChangeListener { _, focused -> tools.visibility = if (focused || !field.text.isNullOrBlank()) GONE else VISIBLE }
        sendIcon.setOnClickListener { send(field.text.toString().trim()) }
        thumb.setOnClickListener { person?.let { send(Notifier.quickIcon(c, it.optString("id"))) } }
        // Hold the quick icon to choose another one for this friend; it is also what the notification's quick button sends.
        thumb.setOnLongClickListener { choosingQuick = true; pickingReaction = null; thumb.performHapticFeedback(HapticFeedbackConstants.LONG_PRESS); openPanel(); true }
        // When the keyboard opens or closes, keep the latest message in view.
        stage.addOnLayoutChangeListener { _, _, top, _, bottomEdge, _, oldTop, _, oldBottom -> if (bottomEdge - top < oldBottom - oldTop) scroll.post { scroll.fullScroll(View.FOCUS_DOWN) } }
    }

    // ---- opening and closing ----

    fun open(who: JSONObject) {
        person = who
        messages.clear()
        reactions.clear()
        last = 0
        replyingTo = null
        selected = null
        field.setText("")
        updateReplyBar()
        dismissSelection()
        panel.visibility = GONE
        visibility = VISIBLE
        Notifier.chatOpenWith = who.optString("id")
        thumb.text = Notifier.quickIcon(c, who.optString("id"))
        choosingQuick = false
        drawHeader()
        draw(true)
        load(true)
        handler.removeCallbacks(poll)
        handler.postDelayed(poll, 1_500)
    }

    fun close() {
        if (!isOpen) return
        hideKeyboard()
        if (recording) stopRecording(false)
        VoicePlayer.stop()
        hideTrays()
        visibility = GONE
        Notifier.chatOpenWith = null
        handler.removeCallbacks(poll)
        onClosed()
    }

    /** Back: first the selection, then the emoji panel, then the chat itself. */
    fun back() {
        when {
            viewer.visibility == VISIBLE -> viewer.visibility = GONE
            album.visibility == VISIBLE -> album.visibility = GONE
            recording -> stopRecording(false)
            trayBox.visibility == VISIBLE -> hideTrays()
            holdLayer.visibility == VISIBLE -> dismissSelection()
            panel.visibility == VISIBLE -> { panel.visibility = GONE; choosingQuick = false }
            else -> close()
        }
    }

    private fun hideKeyboard() {
        (c.getSystemService(Context.INPUT_METHOD_SERVICE) as InputMethodManager).hideSoftInputFromWindow(field.windowToken, 0)
    }

    private fun avatarFor(name: String, url: String?, size: Int, equipped: JSONObject? = null): View {
        val letter = c.label(size / 2.6f, Color.WHITE, bold = true).apply { text = name.take(1).uppercase(); gravity = Gravity.CENTER }
        val image = ImageView(c).apply { scaleType = ImageView.ScaleType.CENTER_CROP }
        val box = FrameLayout(c).apply {
            background = roundRect(Ui.RAISED, c.dp(size))
            clipRounded(c.dp(size) / 2)
            addView(letter, FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT))
            addView(image, FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT))
        }
        if (url != null) Images.load(image, url, c.dp(size))
        box.layoutParams = LinearLayout.LayoutParams(c.dp(size), c.dp(size))
        return Shop.decorate(c, box, size, equipped)
    }

    private fun drawHeader() {
        val who = person ?: return
        header.removeAllViews()
        header.addView(c.icon(R.drawable.ic_down, Color.WHITE, 44, 10).apply { rotation = 90f; setOnClickListener { close() } })
        header.addView(avatarFor(who.optString("display_name"), Account.avatarUrl(who.optString("id"), who.optInt("avatar_version")), 38, who.optJSONObject("equipped")), LinearLayout.LayoutParams(c.dp(38), c.dp(38)).apply { marginStart = c.dp(2) })
        header.addView(LinearLayout(c).apply {
            orientation = LinearLayout.VERTICAL
            setOnClickListener { onProfile(who) }
            addView(c.label(17f, Color.WHITE, bold = true).apply { text = who.optString("display_name") })
            addView(c.label(12f, Ui.MUTED).apply { text = "@" + who.optString("username") })
        }, LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f).apply { marginStart = c.dp(12) })
    }

    // ---- the network ----

    private fun load(first: Boolean) {
        val who = person ?: return
        Thread {
            val reply = Account.call("GET", "/messages/${who.optString("id")}?after=$last&read=1")
            val found = reply.optJSONArray("messages")
            val marks = reply.optJSONObject("reactions")
            post {
                if (!isOpen || person?.optString("id") != who.optString("id") || found == null) return@post
                if (reply.has("quick")) {
                    // The quick icon is shared with this friend: what either of you picked, on any device.
                    Notifier.syncQuick(c, who.optString("id"), if (reply.isNull("quick")) null else reply.optString("quick"))
                    thumb.text = Notifier.quickIcon(c, who.optString("id"))
                }
                var news = false
                for (i in 0 until found.length()) {
                    val m = found.getJSONObject(i)
                    if (m.optLong("id") > last) {
                        if (m.optBoolean("mine")) messages.removeAll { it.optLong("id") < 0 && it.optString("body") == m.optString("body") }
                        messages += m
                        last = m.optLong("id")
                        news = true
                    }
                }
                // The server sends the reactions of the newest messages every time; those messages take them as they are.
                var changed = false
                val newest = messages.maxOfOrNull { it.optLong("id") } ?: 0
                for (m in messages) {
                    val id = m.optLong("id")
                    if (id <= 0 || id < newest - 60) continue
                    val list = marks?.optJSONArray(id.toString())
                    val parsed = (0 until (list?.length() ?: 0)).map { list!!.getJSONObject(it).let { r -> r.optString("emoji") to r.optBoolean("mine") } }
                    if (reactions[id].orEmpty() != parsed) { reactions[id] = parsed; changed = true }
                }
                if (news || changed || first) draw(first || news)
            }
        }.start()
    }

    private fun send(text: String, replyId: Long = replyingTo?.optLong("id") ?: 0) {
        val who = person ?: return
        if (text.isEmpty()) return
        field.setText("")
        val placeholder = JSONObject().put("id", -1).put("mine", true).put("body", text).put("at", System.currentTimeMillis() / 1000)
        replyingTo?.let { placeholder.put("reply", JSONObject().put("id", it.optLong("id")).put("mine", it.optBoolean("mine")).put("body", it.optString("body").take(120))) }
        messages += placeholder
        replyingTo = null
        updateReplyBar()
        draw(true)
        Thread {
            val body = JSONObject().put("body", text)
            if (replyId > 0) body.put("reply_to", replyId)
            val reply = Account.call("POST", "/messages/${who.optString("id")}", body)
            post {
                messages.remove(placeholder)
                if (!reply.has("error") && messages.none { it.optLong("id") == reply.optLong("id") }) {
                    if (placeholder.has("reply")) reply.put("reply", placeholder.get("reply"))
                    messages += reply
                    last = maxOf(last, reply.optLong("id"))
                } else if (reply.has("error")) c.say(reply.optString("error"))
                draw(true)
            }
        }.start()
    }

    private fun react(message: JSONObject, emoji: String?) {
        val who = person ?: return
        val id = message.optLong("id")
        if (id <= 0) return
        val others = reactions[id].orEmpty().filter { !it.second }
        reactions[id] = if (emoji == null) others else others + (emoji to true)
        draw(false)
        Thread { Account.call("POST", "/messages/${who.optString("id")}/$id/react", JSONObject().put("emoji", emoji ?: JSONObject.NULL)) }.start()
    }

    // ---- drawing the conversation ----

    private fun draw(stick: Boolean) {
        val who = person ?: return
        val atBottom = list.height == 0 || scroll.scrollY + scroll.height >= list.height - c.dp(90)
        list.removeAllViews()
        var previous: JSONObject? = null
        // Photos sent together (same person, within two minutes) share one block.
        fun photo(m: JSONObject) = m.optJSONObject("media")?.let { it.optString("kind") == "image" && !it.optBoolean("expired") && !it.optBoolean("pending") } == true
        val groups = ArrayList<MutableList<JSONObject>>()
        for (m in messages) {
            val tail = groups.lastOrNull()?.last()
            if (tail != null && photo(m) && photo(tail) && tail.optBoolean("mine") == m.optBoolean("mine") && m.optLong("at") - tail.optLong("at") <= 120) groups.last() += m
            else groups += mutableListOf(m)
        }
        for ((index, group) in groups.withIndex()) {
            val m = group.last()
            val at = group.first().optLong("at")
            val mine = m.optBoolean("mine")
            val day = dayFormat.format(java.util.Date(at * 1000))
            val gap = previous == null || at - previous.optLong("at") > 3600 || dayFormat.format(java.util.Date(previous.optLong("at") * 1000)) != day
            if (gap) list.addView(c.label(12f, Ui.MUTED).apply {
                text = (if (previous == null || dayFormat.format(java.util.Date(previous.optLong("at") * 1000)) != day) day + "  " else "") + clock.format(java.util.Date(at * 1000))
                gravity = Gravity.CENTER
                setPadding(0, c.dp(12), 0, c.dp(8))
            }, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT))
            val next = groups.getOrNull(index + 1)?.first()
            val endsRun = next == null || next.optBoolean("mine") != mine || next.optLong("at") - m.optLong("at") > 300
            val startsRun = gap || previous?.optBoolean("mine") != mine || at - (previous?.optLong("at") ?: 0) > 300
            list.addView(row(m, who, showAvatar = !mine && endsRun, spaced = startsRun, group = group.takeIf { it.size > 1 }))
            previous = m
        }
        if (messages.isEmpty()) list.addView(c.label(14f, Ui.MUTED, lines = 3).apply { text = "No messages yet. Say hello to ${who.optString("display_name")}!"; gravity = Gravity.CENTER; ellipsize = null; setPadding(c.dp(24), c.dp(40), c.dp(24), 0) })
        if (stick || atBottom) scroll.post { scroll.fullScroll(View.FOCUS_DOWN) }
    }

    /** One message as a row: the other person's picture beside theirs, mine pushed to the right. */
    private fun row(m: JSONObject, who: JSONObject, showAvatar: Boolean, spaced: Boolean, forClone: Boolean = false, group: List<JSONObject>? = null): LinearLayout {
        val mine = m.optBoolean("mine")
        val id = m.optLong("id")
        val column = LinearLayout(c).apply { orientation = LinearLayout.VERTICAL; clipChildren = false }
        m.optJSONObject("reply")?.let { r ->
            val whoReplied = if (mine) "You" else who.optString("display_name")
            val target = if (r.optBoolean("mine")) (if (mine) "yourself" else "you") else (if (mine) who.optString("display_name") else "themselves")
            column.addView(LinearLayout(c).apply {
                gravity = Gravity.CENTER_VERTICAL
                addView(c.icon(R.drawable.ic_reply, Ui.MUTED, 14, 1).apply { isClickable = false; background = null }, LinearLayout.LayoutParams(c.dp(14), c.dp(14)))
                addView(c.label(12f, Ui.MUTED).apply { text = "$whoReplied replied to $target"; setPadding(c.dp(6), 0, 0, 0) })
            }, LinearLayout.LayoutParams(ViewGroup.LayoutParams.WRAP_CONTENT, ViewGroup.LayoutParams.WRAP_CONTENT).apply { gravity = if (mine) Gravity.END else Gravity.START; bottomMargin = c.dp(3) })
            column.addView(c.label(14f, 0xFFB8B8B8.toInt(), lines = 2).apply {
                text = r.optString("body")
                background = roundRect(0xFF262626.toInt(), c.dp(16))
                setPadding(c.dp(14), c.dp(9), c.dp(14), c.dp(14))
            }, LinearLayout.LayoutParams(ViewGroup.LayoutParams.WRAP_CONTENT, ViewGroup.LayoutParams.WRAP_CONTENT).apply { gravity = if (mine) Gravity.END else Gravity.START; bottomMargin = -c.dp(8) })
        }
        val media = m.optJSONObject("media")
        val textBubble = c.label(16f, if (mine) Color.BLACK else Color.WHITE, lines = 100).apply {
            text = m.optString("body")
            ellipsize = null
            background = roundRect(if (mine) Ui.GREEN else Ui.RAISED, c.dp(18))
            setPadding(c.dp(14), c.dp(9), c.dp(14), c.dp(9))
            maxWidth = (c.resources.displayMetrics.widthPixels * 0.72f).toInt()
            if (id < 0) alpha = 0.6f
        }
        // A message that is only emoji is shown big, with no bubble, like Messenger.
        val onlyEmoji = m.optString("body").let { it.isNotEmpty() && it.length <= 12 && it.none { ch -> ch.isLetterOrDigit() || ch == ' ' } }
        if (onlyEmoji && media == null) { textBubble.textSize = 38f; textBubble.background = null; textBubble.setPadding(c.dp(4), c.dp(2), c.dp(4), c.dp(2)) }
        val bubble: View = when {
            group != null -> gallery(group.map { it.optJSONObject("media")?.optString("url").orEmpty() })
            media != null -> mediaBubble(media, mine, id)
            else -> textBubble
        }
        if (media != null && id < 0) bubble.alpha = 0.6f
        // A photo keeps the size it was given; everything else sizes itself.
        val bubbleParams = (bubble.layoutParams as? LinearLayout.LayoutParams) ?: LinearLayout.LayoutParams(ViewGroup.LayoutParams.WRAP_CONTENT, ViewGroup.LayoutParams.WRAP_CONTENT)
        bubbleParams.gravity = if (mine) Gravity.END else Gravity.START
        column.addView(bubble, bubbleParams)
        val marks = reactions[id].orEmpty()
        if (marks.isNotEmpty()) {
            val text = marks.map { it.first }.distinct().joinToString("") + if (marks.size > 1) " ${marks.size}" else ""
            column.addView(c.label(13f, Color.WHITE).apply {
                this.text = text
                background = roundRect(0xFF232323.toInt(), c.dp(12)).also { it.setStroke(c.dp(2), Ui.BG) }
                setPadding(c.dp(7), c.dp(2), c.dp(7), c.dp(2))
            }, LinearLayout.LayoutParams(ViewGroup.LayoutParams.WRAP_CONTENT, ViewGroup.LayoutParams.WRAP_CONTENT).apply { gravity = if (mine) Gravity.END else Gravity.START; topMargin = -c.dp(6); marginStart = c.dp(8); marginEnd = c.dp(8) })
        }
        val row = LinearLayout(c).apply {
            gravity = if (mine) Gravity.END or Gravity.BOTTOM else Gravity.START or Gravity.BOTTOM
            clipChildren = false
            if (!mine) addView(
                if (showAvatar) avatarFor(who.optString("display_name"), Account.avatarUrl(who.optString("id"), who.optInt("avatar_version")), 30) else View(c),
                LinearLayout.LayoutParams(c.dp(30), c.dp(30)).apply { marginEnd = c.dp(8) },
            )
            addView(column, LinearLayout.LayoutParams(ViewGroup.LayoutParams.WRAP_CONTENT, ViewGroup.LayoutParams.WRAP_CONTENT))
        }
        if (!forClone && id > 0) {
            val hold = View.OnLongClickListener { select(m, row); true }
            bubble.setOnLongClickListener(hold)
            if (media == null) bubble.setOnClickListener { }
        }
        row.layoutParams = LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT).apply { topMargin = c.dp(if (spaced) 8 else 2) }
        return row
    }

    // ---- holding a message: reactions above it, reply/copy/share in place of the message box ----

    private fun select(m: JSONObject, source: View) {
        hideKeyboard()
        panel.visibility = GONE
        selected = m
        performHapticFeedback(HapticFeedbackConstants.LONG_PRESS)
        val who = person ?: return
        val spot = IntArray(2); source.getLocationInWindow(spot)
        val base = IntArray(2); stage.getLocationInWindow(base)
        val top = spot[1] - base[1]
        holdLayer.removeAllViews()
        val clone = row(m, who, showAvatar = false, spaced = false, forClone = true)
        val mine = m.optBoolean("mine")
        val cloneWidth = source.width
        holdLayer.addView(clone, FrameLayout.LayoutParams(ViewGroup.LayoutParams.WRAP_CONTENT, ViewGroup.LayoutParams.WRAP_CONTENT).apply {
            gravity = Gravity.TOP or Gravity.START
            leftMargin = spot[0] - base[0]
            topMargin = top
            width = cloneWidth
        })
        // The reactions, in a pill above the message (below it when there is no room).
        val mineNow = reactions[m.optLong("id")].orEmpty().firstOrNull { it.second }?.first
        val bar = LinearLayout(c).apply {
            gravity = Gravity.CENTER_VERTICAL
            background = roundRect(0xFF2A2A2A.toInt(), c.dp(30))
            setPadding(c.dp(8), c.dp(6), c.dp(8), c.dp(6))
            elevation = c.dp(8).toFloat()
            quick.forEach { e ->
                addView(c.label(28f, Color.WHITE).apply {
                    text = e
                    gravity = Gravity.CENTER
                    if (e == mineNow) background = roundRect(0x55FFFFFF, c.dp(22))
                    setOnClickListener { react(m, if (e == mineNow) null else e); dismissSelection() }
                }, LinearLayout.LayoutParams(c.dp(44), c.dp(44)))
            }
            addView(c.icon(R.drawable.ic_add_circle, Ui.MUTED, 40, 6).apply { setOnClickListener { pickingReaction = m; choosingQuick = false; holdLayer.visibility = GONE; showActions(false); openPanel() } }, LinearLayout.LayoutParams(c.dp(40), c.dp(44)))
        }
        val barHeight = c.dp(56)
        val aboveTop = top - barHeight - c.dp(6)
        holdLayer.addView(bar, FrameLayout.LayoutParams(ViewGroup.LayoutParams.WRAP_CONTENT, barHeight).apply {
            gravity = Gravity.TOP or Gravity.START
            topMargin = if (aboveTop >= c.dp(4)) aboveTop else top + source.height + c.dp(6)
            val screen = c.resources.displayMetrics.widthPixels
            val wanted = c.dp(8) + (if (mine) screen - c.dp(8) - c.dp(8) - c.dp(44) * quick.size - c.dp(40) - c.dp(16) else 0)
            leftMargin = wanted.coerceIn(c.dp(8), (screen - c.dp(44) * quick.size - c.dp(40) - c.dp(24)).coerceAtLeast(c.dp(8)))
        })
        holdLayer.visibility = VISIBLE
        showActions(true)
    }

    private fun showActions(on: Boolean) {
        composer.visibility = if (on) GONE else VISIBLE
        actions.visibility = if (on) VISIBLE else GONE
        if (!on) return
        actions.removeAllViews()
        val m = selected ?: return
        fun item(icon: Int, label: String, run: () -> Unit) = LinearLayout(c).apply {
            orientation = LinearLayout.VERTICAL
            gravity = Gravity.CENTER
            background = pressable()
            setPadding(0, c.dp(4), 0, c.dp(4))
            addView(ImageView(c).apply { setImageResource(icon); setColorFilter(Color.WHITE) }, LinearLayout.LayoutParams(c.dp(26), c.dp(26)))
            addView(c.label(13f, Color.WHITE).apply { text = label; gravity = Gravity.CENTER; setPadding(0, c.dp(6), 0, 0) })
            setOnClickListener { run() }
        }
        val grow = { LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f) }
        actions.addView(item(R.drawable.ic_reply, "Reply") { replyingTo = m; updateReplyBar(); dismissSelection(); field.requestFocus(); (c.getSystemService(Context.INPUT_METHOD_SERVICE) as InputMethodManager).showSoftInput(field, 0) }, grow())
        actions.addView(item(R.drawable.ic_copy, "Copy") {
            (c.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager).setPrimaryClip(ClipData.newPlainText("message", m.optString("body")))
            dismissSelection()
            c.say("Copied")
        }, grow())
        actions.addView(item(R.drawable.ic_share, "Share") {
            dismissSelection()
            c.startActivity(Intent.createChooser(Intent(Intent.ACTION_SEND).setType("text/plain").putExtra(Intent.EXTRA_TEXT, m.optString("body")), null))
        }, grow())
    }

    private fun dismissSelection() {
        selected = null
        pickingReaction = null
        holdLayer.visibility = GONE
        holdLayer.removeAllViews()
        showActions(false)
    }

    private fun updateReplyBar() {
        val m = replyingTo
        replyBar.removeAllViews()
        if (m == null) { replyBar.visibility = GONE; return }
        replyBar.visibility = VISIBLE
        val who = person
        replyBar.addView(LinearLayout(c).apply {
            orientation = LinearLayout.VERTICAL
            addView(c.label(13f, Color.WHITE, bold = true).apply { text = "Replying to " + if (m.optBoolean("mine")) "yourself" else who?.optString("display_name").orEmpty() })
            addView(c.label(13f, Ui.MUTED).apply { text = m.optString("body") })
        }, LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f))
        replyBar.addView(c.icon(R.drawable.ic_close, Ui.MUTED, 36, 8).apply { setOnClickListener { replyingTo = null; updateReplyBar() } })
    }

    // ---- photos and voice messages ----

    private fun buildMedia() {
        fun tool(icon: Int, run: () -> Unit) = c.icon(icon, Ui.GREEN, 40, 8).apply { setOnClickListener { run() } }
        tools.addView(tool(R.drawable.ic_camera) { openCamera() })
        tools.addView(tool(R.drawable.ic_image) { toggleTray() })
        tools.addView(tool(R.drawable.ic_mic) { startRecording() })
        // The recording bar: bin, then a green pill with pause, the level, the time, and send.
        recordBar.addView(c.icon(R.drawable.ic_trash, Color.WHITE, 44, 10).apply { setOnClickListener { stopRecording(false) } })
        val pill = LinearLayout(c).apply {
            gravity = Gravity.CENTER_VERTICAL
            background = roundRect(Ui.GREEN, c.dp(26))
            setPadding(c.dp(6), c.dp(4), c.dp(16), c.dp(4))
            addView(recordPause.apply { background = roundRect(Color.WHITE, c.dp(18)); setOnClickListener { recorder.togglePause(); setImageResource(if (recorder.isPaused) R.drawable.ic_play else R.drawable.ic_pause) } }, LinearLayout.LayoutParams(c.dp(36), c.dp(36)))
            addView(recordWave, LinearLayout.LayoutParams(0, c.dp(28), 1f).apply { marginStart = c.dp(12); marginEnd = c.dp(12) })
            addView(recordTimer)
        }
        recordBar.addView(pill, LinearLayout.LayoutParams(0, c.dp(44), 1f).apply { marginStart = c.dp(4) })
        recordBar.addView(c.icon(R.drawable.ic_send, Ui.GREEN, 44, 10).apply { setOnClickListener { stopRecording(true) } })
        viewer.addView(viewerImage, FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT))
        viewer.addView(c.icon(R.drawable.ic_close, Color.WHITE, 44, 10).apply { setOnClickListener { viewer.visibility = GONE } }, FrameLayout.LayoutParams(c.dp(44), c.dp(44), Gravity.TOP or Gravity.END).apply { setMargins(0, c.dp(36), c.dp(12), 0) })
        // Swipe left or right to move through the photos that were sent together; a tap closes the photo.
        val swipe = android.view.GestureDetector(c, object : android.view.GestureDetector.SimpleOnGestureListener() {
            override fun onDown(e: android.view.MotionEvent) = true
            override fun onSingleTapUp(e: android.view.MotionEvent): Boolean { viewer.visibility = GONE; return true }
            override fun onFling(a: android.view.MotionEvent?, b: android.view.MotionEvent, vx: Float, vy: Float): Boolean {
                if (Math.abs(vx) < Math.abs(vy) || Math.abs(vx) < 400) return false
                showShot(if (vx < 0) shot + 1 else shot - 1)
                return true
            }
        })
        viewer.setOnTouchListener { _, e -> swipe.onTouchEvent(e) }
        viewer.addView(viewerCount, FrameLayout.LayoutParams(ViewGroup.LayoutParams.WRAP_CONTENT, ViewGroup.LayoutParams.WRAP_CONTENT, Gravity.TOP or Gravity.CENTER_HORIZONTAL).apply { topMargin = c.dp(46) })
        val head = LinearLayout(c).apply {
            gravity = Gravity.CENTER_VERTICAL
            setPadding(c.dp(12), c.dp(36), c.dp(12), c.dp(8))
            addView(c.icon(R.drawable.ic_close, Color.WHITE, 44, 10).apply { setOnClickListener { album.visibility = GONE } }, LinearLayout.LayoutParams(c.dp(44), c.dp(44)))
            addView(albumTitle.apply { setPadding(c.dp(8), 0, 0, 0) })
        }
        val body = LinearLayout(c).apply {
            orientation = LinearLayout.VERTICAL
            addView(head)
            addView(android.widget.ScrollView(c).apply { addView(albumGrid); setPadding(c.dp(10), 0, c.dp(10), c.dp(20)); clipToPadding = false }, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, 0, 1f))
        }
        album.addView(body, FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT))
    }

    private fun showShot(index: Int) {
        if (index !in shots.indices) return
        shot = index
        viewerImage.setImageDrawable(null)
        Images.load(viewerImage, shots[index], c.dp(1080))
        viewerCount.text = if (shots.size > 1) "${index + 1} / ${shots.size}" else ""
    }

    /** Photos sent together open as a grid; a single photo opens straight to the full view. */
    private fun openPhotos(urls: List<String>) {
        shots = urls
        if (urls.size == 1) { showShot(0); viewer.visibility = VISIBLE; return }
        albumTitle.text = "${urls.size} photos"
        albumGrid.removeAllViews()
        val side = (c.resources.displayMetrics.widthPixels - c.dp(20) - c.dp(24)) / 2
        urls.forEachIndexed { i, url ->
            albumGrid.addView(ImageView(c).apply {
                scaleType = ImageView.ScaleType.CENTER_CROP
                background = roundRect(Ui.RAISED, c.dp(14))
                clipRounded(c.dp(14))
                Images.load(this, url, c.dp(360))
                setOnClickListener { showShot(i); viewer.visibility = VISIBLE }
            }, android.widget.GridLayout.LayoutParams().apply { width = side; height = side; setMargins(c.dp(6), c.dp(6), c.dp(6), c.dp(6)) })
        }
        album.visibility = VISIBLE
    }

    /** Photos sent together, as one block of up to four pictures ("+N" over the last when there are more). */
    private fun gallery(urls: List<String>): View {
        val width = (c.resources.displayMetrics.widthPixels * 0.62f).toInt()
        val gap = c.dp(2)
        fun tile(url: String, w: Int, h: Int, more: Int = 0) = FrameLayout(c).apply {
            addView(ImageView(c).apply { scaleType = ImageView.ScaleType.CENTER_CROP; setBackgroundColor(Ui.RAISED); Images.load(this, url, c.dp(360)) }, FrameLayout.LayoutParams(w, h))
            if (more > 0) addView(c.label(22f, Color.WHITE, bold = true).apply { text = "+$more"; gravity = Gravity.CENTER; setBackgroundColor(0x99000000.toInt()) }, FrameLayout.LayoutParams(w, h))
        }
        fun line(vararg tiles: View) = LinearLayout(c).apply { tiles.forEachIndexed { i, t -> addView(t, LinearLayout.LayoutParams(ViewGroup.LayoutParams.WRAP_CONTENT, ViewGroup.LayoutParams.WRAP_CONTENT).apply { if (i > 0) marginStart = gap }) } }
        val half = (width - gap) / 2
        val box = LinearLayout(c).apply { orientation = LinearLayout.VERTICAL }
        fun add(row: View, top: Boolean = true) = box.addView(row, LinearLayout.LayoutParams(ViewGroup.LayoutParams.WRAP_CONTENT, ViewGroup.LayoutParams.WRAP_CONTENT).apply { if (!top) topMargin = gap })
        when {
            urls.size == 2 -> add(line(tile(urls[0], half, half * 5 / 4), tile(urls[1], half, half * 5 / 4)))
            urls.size == 3 -> { add(line(tile(urls[0], width, width * 3 / 5))); add(line(tile(urls[1], half, half), tile(urls[2], half, half)), false) }
            else -> {
                add(line(tile(urls[0], half, half), tile(urls[1], half, half)))
                add(line(tile(urls[2], half, half), tile(urls[3], half, half, urls.size - 4)), false)
            }
        }
        return box.apply {
            clipRounded(c.dp(18))
            layoutParams = LinearLayout.LayoutParams(ViewGroup.LayoutParams.WRAP_CONTENT, ViewGroup.LayoutParams.WRAP_CONTENT)
            setOnClickListener { openPhotos(urls) }
        }
    }

    private fun hideTrays() {
        panel.visibility = GONE
        trayBox.visibility = GONE
        tray?.clear()
    }

    private fun toggleTray() {
        if (trayBox.visibility == VISIBLE) { hideTrays(); return }
        if (!ChatMedia.has(c, ChatMedia.galleryPermission)) { (c as Activity).requestPermissions(arrayOf(ChatMedia.galleryPermission), 21); return }
        openTray()
    }

    private fun openTray() {
        hideKeyboard()
        panel.visibility = GONE
        if (tray == null) tray = PhotoTray(c, { sendPhotos(it) }, { browse() }).also { trayBox.addView(it, FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT)) }
        trayBox.visibility = VISIBLE
        tray?.load()
    }

    /** The system's own picker, for when the tray cannot list photos. */
    private fun browse() {
        hideTrays()
        (c as Activity).startActivityForResult(Intent(Intent.ACTION_GET_CONTENT).setType("image/*").putExtra(Intent.EXTRA_ALLOW_MULTIPLE, true), 14)
    }

    private fun openCamera() {
        val file = java.io.File(c.cacheDir, "camera.jpg").also { it.delete() }
        val uri = androidx.core.content.FileProvider.getUriForFile(c, "${c.packageName}.files", file)
        (c as Activity).startActivityForResult(Intent(android.provider.MediaStore.ACTION_IMAGE_CAPTURE).putExtra(android.provider.MediaStore.EXTRA_OUTPUT, uri).addFlags(Intent.FLAG_GRANT_WRITE_URI_PERMISSION), 15)
    }

    /** Results from the system picker (14) and the camera (15), forwarded by the activity that shows this chat. */
    fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        if (resultCode != Activity.RESULT_OK) return
        when (requestCode) {
            14 -> {
                val uris = ArrayList<Uri>()
                data?.clipData?.let { clip -> for (i in 0 until minOf(clip.itemCount, 5)) uris += clip.getItemAt(i).uri }
                if (uris.isEmpty()) data?.data?.let { uris += it }
                sendPhotos(uris)
            }
            15 -> sendPhotos(listOf(Uri.fromFile(java.io.File(c.cacheDir, "camera.jpg"))))
        }
    }

    fun onPermissionResult(requestCode: Int, granted: Boolean) {
        when (requestCode) {
            21 -> if (granted) openTray() else browse()
            22 -> if (granted) startRecording() else c.say("Allow the microphone to send voice messages.")
        }
    }

    private fun pendingMedia(kind: String, seconds: Int): JSONObject =
        JSONObject().put("id", -1).put("mine", true).put("body", if (kind == "image") "\uD83D\uDCF7 Photo" else "\uD83C\uDFA4 Voice message").put("at", System.currentTimeMillis() / 1000)
            .put("media", JSONObject().put("kind", kind).put("pending", true).put("seconds", seconds).put("url", "").put("expired", false))

    /** Shows [placeholder] at once, runs [work] on a worker thread, and swaps in what the server answers. */
    private fun deliver(placeholder: JSONObject, work: () -> JSONObject) {
        messages += placeholder
        draw(true)
        Thread {
            val reply = work()
            post {
                messages.remove(placeholder)
                if (!reply.has("error") && messages.none { it.optLong("id") == reply.optLong("id") }) {
                    messages += reply
                    last = maxOf(last, reply.optLong("id"))
                } else if (reply.has("error")) c.say(reply.optString("error"))
                draw(true)
            }
        }.start()
    }

    private fun sendPhotos(uris: List<Uri>) {
        val who = person ?: return
        hideTrays()
        val replyId = replyingTo?.optLong("id") ?: 0
        replyingTo = null
        updateReplyBar()
        for (uri in uris) deliver(pendingMedia("image", 0)) {
            val bytes = ChatMedia.photo(c, uri)
            if (bytes == null) JSONObject().put("error", "That photo could not be read.") else ChatMedia.send(who.optString("id"), "image", bytes, "image/jpeg", 0, replyId)
        }
    }

    private fun startRecording() {
        if (!ChatMedia.has(c, Manifest.permission.RECORD_AUDIO)) { (c as Activity).requestPermissions(arrayOf(Manifest.permission.RECORD_AUDIO), 22); return }
        if (!recorder.start()) { c.say("Could not start recording."); return }
        VoicePlayer.stop()
        recording = true
        hideKeyboard()
        hideTrays()
        composer.visibility = GONE
        recordBar.visibility = VISIBLE
        recordPause.setImageResource(R.drawable.ic_pause)
        live = FloatArray(36) { 0.15f }
        handler.post(recordTick)
    }

    private val recordTick = object : Runnable {
        override fun run() {
            if (!recording) return
            live = live.copyOfRange(1, live.size) + recorder.level().coerceAtLeast(0.12f)
            recordWave.levels = live
            val s = recorder.seconds()
            recordTimer.text = "%d:%02d".format(s / 60, s % 60)
            if (s >= 180) { stopRecording(true); return }
            handler.postDelayed(this, 70)
        }
    }

    private fun stopRecording(send: Boolean) {
        if (!recording) return
        recording = false
        recordBar.visibility = GONE
        composer.visibility = VISIBLE
        val who = person
        if (!send || who == null) { recorder.cancel(); return }
        val done = recorder.finish()
        if (done == null) { c.say("Hold on a little longer to record."); return }
        val (file, seconds) = done
        val replyId = replyingTo?.optLong("id") ?: 0
        replyingTo = null
        updateReplyBar()
        deliver(pendingMedia("voice", seconds)) {
            val reply = ChatMedia.send(who.optString("id"), "voice", file.readBytes(), "audio/mp4", seconds, replyId)
            file.delete()
            reply
        }
    }

    /** A photo or voice message as a bubble. */
    private fun mediaBubble(media: JSONObject, mine: Boolean, id: Long): View {
        val kind = media.optString("kind")
        val maxWidth = (c.resources.displayMetrics.widthPixels * 0.62f).toInt()
        if (media.optBoolean("expired")) {
            return c.label(14f, Ui.MUTED, lines = 2).apply {
                text = (if (kind == "image") "Photo" else "Voice message") + " expired"
                background = roundRect(Ui.RAISED, c.dp(18))
                setPadding(c.dp(14), c.dp(10), c.dp(14), c.dp(10))
            }
        }
        if (media.optBoolean("pending")) {
            return FrameLayout(c).apply {
                background = roundRect(if (kind == "voice" && mine) Ui.GREEN else Ui.RAISED, c.dp(18))
                alpha = 0.6f
                addView(c.label(14f, if (kind == "voice" && mine) Color.BLACK else Ui.MUTED).apply { text = if (kind == "image") "Sending photo\u2026" else "Sending voice message\u2026"; setPadding(c.dp(16), c.dp(12), c.dp(16), c.dp(12)) })
            }
        }
        val url = media.optString("url")
        if (kind == "image") {
            val image = ImageView(c).apply {
                scaleType = ImageView.ScaleType.CENTER_CROP
                background = roundRect(Ui.RAISED, c.dp(18))
                clipRounded(c.dp(18))
                layoutParams = LinearLayout.LayoutParams(maxWidth, (maxWidth * 0.75f).toInt())
            }
            Images.load(image, url, c.dp(480)) { bitmap ->
                val ratio = (bitmap.height / bitmap.width.toFloat()).coerceIn(0.6f, 1.5f)
                (image.layoutParams as? LinearLayout.LayoutParams)?.let { it.height = (maxWidth * ratio).toInt(); image.layoutParams = it }
            }
            image.setOnClickListener { openPhotos(listOf(url)) }
            return image
        }
        val seconds = media.optInt("seconds")
        val ink = if (mine) Color.BLACK else Color.WHITE
        val play = c.icon(R.drawable.ic_play, if (mine) Color.WHITE else Color.BLACK, 36, 8).apply { background = roundRect(if (mine) Color.BLACK else Ui.GREEN, c.dp(18)); isClickable = false }
        val wave = WaveView(c).apply { levels = WaveView.shape(id); played = ink; rest = if (mine) 0x66000000 else 0x55FFFFFF }
        val box = LinearLayout(c).apply {
            gravity = Gravity.CENTER_VERTICAL
            background = roundRect(if (mine) Ui.GREEN else Ui.RAISED, c.dp(24))
            setPadding(c.dp(8), c.dp(8), c.dp(16), c.dp(8))
            addView(play, LinearLayout.LayoutParams(c.dp(36), c.dp(36)))
            addView(wave, LinearLayout.LayoutParams(c.dp(110), c.dp(26)).apply { marginStart = c.dp(10); marginEnd = c.dp(10) })
            addView(c.label(13f, ink, bold = true).apply { text = "%d:%02d".format(seconds / 60, seconds % 60) })
        }
        val tick = object : Runnable {
            override fun run() {
                val playing = VoicePlayer.isPlaying(url)
                play.setImageResource(if (playing) R.drawable.ic_pause else R.drawable.ic_play)
                wave.progress = if (VoicePlayer.current == url) VoicePlayer.progress(url) else 0f
                if (playing && box.isAttachedToWindow) box.postDelayed(this, 120)
            }
        }
        box.setOnClickListener { VoicePlayer.toggle(c, url) { tick.run() } }
        box.post(tick)
        return box
    }

    // ---- the emoji panel ----

    private val sets = listOf(
        "😀" to "😀😃😄😁😆😅😂🤣😊😇🙂🙃😉😌😍🥰😘😗😙😚😋😛😝😜🤪🤨🧐🤓😎🤩🥳😏😒😞😔😟😕🙁😣😖😫😩🥺😢😭😤😠😡🤬🤯😳🥵🥶😱😨😰😥😓🤗🤔🤭🤫😶😐😑😬🙄😯😦😧😮😲🥱😴🤤😪😵🤐🥴🤢🤮🤧😷🤒🤕🤑🤠😈👿👹👺🤡💩👻💀👽👾🤖🎃",
        "👍" to "👍👎👊✊🤛🤜🤞✌️🤟🤘👌🤏👈👉👆👇☝️✋🤚🖐️🖖👋🤙💪🙏🤝👏🙌👐🤲❤️🧡💛💚💙💜🖤🤍🤎💔💕💞💓💗💖💘💝💟",
        "🐶" to "🐶🐱🐭🐹🐰🦊🐻🐼🐨🐯🦁🐮🐷🐸🐵🙈🙉🙊🐔🐧🐦🐤🦄🐝🦋🐢🐙🦀🐠🐳🐬🌸🌼🌻🌹🌺🌴🌵🍀🍁",
        "🍕" to "🍎🍊🍋🍌🍉🍇🍓🍒🍑🥭🍍🥝🍅🥑🌽🥐🍞🧀🍗🍔🍟🍕🌭🌮🍜🍣🍰🎂🍩🍪🍫🍿☕🍺🍷🥂",
        "🎵" to "🎵🎶🎤🎧🎸🎹🥁🎷🎺🎻🎬🎮🎯🎲⚽🏀🏈🎾🏆🎉🎊🎁🎈✨🔥💯⭐🌟💫🌈☀️🌙⚡💥",
    )
    private var panelBuilt = false

    private fun clusters(text: String): List<String> {
        val it = android.icu.text.BreakIterator.getCharacterInstance()
        it.setText(text)
        val out = ArrayList<String>()
        var start = it.first()
        var end = it.next()
        while (end != android.icu.text.BreakIterator.DONE) { out += text.substring(start, end); start = end; end = it.next() }
        return out
    }

    private fun buildPanel() {
        if (panelBuilt) return
        panelBuilt = true
        val grid = LinearLayout(c).apply { orientation = LinearLayout.VERTICAL; setPadding(c.dp(6), c.dp(4), c.dp(6), c.dp(8)) }
        val tabs = LinearLayout(c).apply { gravity = Gravity.CENTER; setPadding(c.dp(6), c.dp(4), c.dp(6), c.dp(4)) }
        val scrollGrid = ScrollView(c).apply { isVerticalScrollBarEnabled = false; addView(grid) }
        fun show(index: Int) {
            grid.removeAllViews()
            clusters(sets[index].second).chunked(8).forEach { chunk ->
                grid.addView(LinearLayout(c).apply {
                    chunk.forEach { e ->
                        addView(c.label(28f, Color.WHITE).apply {
                            text = e
                            gravity = Gravity.CENTER
                            background = pressable(circle = true)
                            setOnClickListener { pick(e) }
                        }, LinearLayout.LayoutParams(0, c.dp(46), 1f))
                    }
                    repeat(8 - chunk.size) { addView(View(c), LinearLayout.LayoutParams(0, c.dp(46), 1f)) }
                })
            }
            for (i in 0 until tabs.childCount) tabs.getChildAt(i).alpha = if (i == index) 1f else 0.45f
        }
        sets.forEachIndexed { i, set ->
            tabs.addView(c.label(22f, Color.WHITE).apply { text = set.first; gravity = Gravity.CENTER; setOnClickListener { show(i) } }, LinearLayout.LayoutParams(0, c.dp(40), 1f))
        }
        panel.addView(tabs)
        panel.addView(scrollGrid, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, 0, 1f))
        show(0)
    }

    private fun pick(emoji: String) {
        if (choosingQuick) {
            choosingQuick = false
            person?.let { Notifier.setQuickIcon(c, it.optString("id"), emoji) }
            thumb.text = emoji
            panel.visibility = GONE
            return
        }
        val target = pickingReaction
        if (target != null) {
            val mineNow = reactions[target.optLong("id")].orEmpty().firstOrNull { it.second }?.first
            react(target, if (mineNow == emoji) null else emoji)
            pickingReaction = null
            panel.visibility = GONE
            dismissSelection()
            return
        }
        val at = field.selectionStart.coerceAtLeast(0)
        field.text.insert(at, emoji)
    }

    private fun openPanel() {
        buildPanel()
        hideKeyboard()
        trayBox.visibility = GONE
        panel.visibility = VISIBLE
    }

    private fun togglePanel() {
        if (panel.visibility == VISIBLE) {
            panel.visibility = GONE
            field.requestFocus()
            (c.getSystemService(Context.INPUT_METHOD_SERVICE) as InputMethodManager).showSoftInput(field, 0)
        } else {
            pickingReaction = null
            choosingQuick = false
            openPanel()
        }
    }
}
