package org.roynit.music

import android.app.Activity
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
        composer.addView(pill, LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f))
        composer.addView(right, LinearLayout.LayoutParams(c.dp(48), c.dp(48)).apply { marginStart = c.dp(6) })
        bottom.addView(composer, FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT))
        bottom.addView(actions, FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT))
        column.addView(bottom)
        column.addView(panel, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, c.dp(290)))
        addView(column, FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT))

        field.addTextChangedListener(object : TextWatcher {
            override fun afterTextChanged(s: Editable?) {
                val has = !s.isNullOrBlank()
                sendIcon.visibility = if (has) VISIBLE else GONE
                thumb.visibility = if (has) GONE else VISIBLE
            }
            override fun beforeTextChanged(s: CharSequence?, start: Int, count: Int, after: Int) {}
            override fun onTextChanged(s: CharSequence?, start: Int, before: Int, count: Int) {}
        })
        field.setOnTouchListener { _, event -> if (event.action == MotionEvent.ACTION_DOWN && panel.visibility == VISIBLE) panel.visibility = GONE; false }
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
        visibility = GONE
        Notifier.chatOpenWith = null
        handler.removeCallbacks(poll)
        onClosed()
    }

    /** Back: first the selection, then the emoji panel, then the chat itself. */
    fun back() {
        when {
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
        for ((index, m) in messages.withIndex()) {
            val at = m.optLong("at")
            val mine = m.optBoolean("mine")
            val day = dayFormat.format(java.util.Date(at * 1000))
            val gap = previous == null || at - previous.optLong("at") > 3600 || dayFormat.format(java.util.Date(previous.optLong("at") * 1000)) != day
            if (gap) list.addView(c.label(12f, Ui.MUTED).apply {
                text = (if (previous == null || dayFormat.format(java.util.Date(previous.optLong("at") * 1000)) != day) day + "  " else "") + clock.format(java.util.Date(at * 1000))
                gravity = Gravity.CENTER
                setPadding(0, c.dp(12), 0, c.dp(8))
            }, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT))
            val next = messages.getOrNull(index + 1)
            val endsRun = next == null || next.optBoolean("mine") != mine || next.optLong("at") - at > 300
            val startsRun = gap || previous?.optBoolean("mine") != mine || at - (previous?.optLong("at") ?: 0) > 300
            list.addView(row(m, who, showAvatar = !mine && endsRun, spaced = startsRun))
            previous = m
        }
        if (messages.isEmpty()) list.addView(c.label(14f, Ui.MUTED, lines = 3).apply { text = "No messages yet. Say hello to ${who.optString("display_name")}!"; gravity = Gravity.CENTER; ellipsize = null; setPadding(c.dp(24), c.dp(40), c.dp(24), 0) })
        if (stick || atBottom) scroll.post { scroll.fullScroll(View.FOCUS_DOWN) }
    }

    /** One message as a row: the other person's picture beside theirs, mine pushed to the right. */
    private fun row(m: JSONObject, who: JSONObject, showAvatar: Boolean, spaced: Boolean, forClone: Boolean = false): LinearLayout {
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
        val bubble = c.label(16f, if (mine) Color.BLACK else Color.WHITE, lines = 100).apply {
            text = m.optString("body")
            ellipsize = null
            background = roundRect(if (mine) Ui.GREEN else Ui.RAISED, c.dp(18))
            setPadding(c.dp(14), c.dp(9), c.dp(14), c.dp(9))
            maxWidth = (c.resources.displayMetrics.widthPixels * 0.72f).toInt()
            if (id < 0) alpha = 0.6f
        }
        // A message that is only emoji is shown big, with no bubble, like Messenger.
        val onlyEmoji = m.optString("body").let { it.isNotEmpty() && it.length <= 12 && it.none { ch -> ch.isLetterOrDigit() || ch == ' ' } }
        if (onlyEmoji) { bubble.textSize = 38f; bubble.background = null; bubble.setPadding(c.dp(4), c.dp(2), c.dp(4), c.dp(2)) }
        column.addView(bubble, LinearLayout.LayoutParams(ViewGroup.LayoutParams.WRAP_CONTENT, ViewGroup.LayoutParams.WRAP_CONTENT).apply { gravity = if (mine) Gravity.END else Gravity.START })
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
            bubble.setOnClickListener { }
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
