package org.roynit.music

import android.app.Activity
import android.app.AlertDialog
import android.content.Context
import android.content.Intent
import android.graphics.Bitmap
import android.graphics.Color
import android.net.Uri
import android.os.Handler
import android.os.Looper
import android.text.InputType
import android.view.Gravity
import android.view.View
import android.view.ViewGroup
import android.widget.EditText
import android.widget.FrameLayout
import android.widget.ImageView
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.TextView
import android.widget.Toast
import org.json.JSONObject
import java.io.ByteArrayOutputStream

/**
 * The Roy account on the phone, laid out like the PC app's Friends tab: sign in (password or Google), friends with
 * what they are listening to, requests, chat, and your profile with picture, cover, status and Orbs.
 */
class FriendsPage(context: Context, host: SongHost) : Page(context, host) {
    private enum class Mode { Friends, Profile, Chat, Person }

    private val body = LinearLayout(context).apply { orientation = LinearLayout.VERTICAL }
    private val scroll = ScrollView(context).apply { isVerticalScrollBarEnabled = false; addView(body) }
    private val ui = Handler(Looper.getMainLooper())
    private var creating = false
    private var busy = false
    private var mode = Mode.Friends
    private var social = JSONObject()
    private var suggestions = emptyList<JSONObject>()
    private var chatWith: JSONObject? = null
    private var lastMessage = 0L
    private var shown = false
    private var picking = 0

    private val poll = object : Runnable {
        override fun run() {
            if (shown && Account.signedIn) {
                if (mode == Mode.Chat) loadChat(false) else if (mode == Mode.Friends) loadFriends()
            }
            ui.postDelayed(this, if (mode == Mode.Chat) 1_500 else 20_000)
        }
    }

    private val messages = ArrayList<JSONObject>()
    private val chatList = LinearLayout(c).apply { orientation = LinearLayout.VERTICAL; setPadding(c.dp(16), c.dp(8), c.dp(16), c.dp(8)) }
    private val chatScroll = ScrollView(c).apply { isVerticalScrollBarEnabled = false; addView(chatList) }
    private val chatHead = LinearLayout(c).apply { gravity = Gravity.CENTER_VERTICAL; setPadding(c.dp(8), c.dp(6), c.dp(16), c.dp(6)) }
    private val chatField = EditText(c).apply {
        hint = "Message"
        setHintTextColor(Ui.MUTED)
        setTextColor(Color.WHITE)
        background = null
        inputType = InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_FLAG_CAP_SENTENCES or InputType.TYPE_TEXT_FLAG_MULTI_LINE
        maxLines = 4
        setPadding(c.dp(18), c.dp(12), c.dp(8), c.dp(12))
    }
    private val chatRoot = LinearLayout(c).apply {
        orientation = LinearLayout.VERTICAL
        visibility = GONE
        addView(chatHead)
        addView(View(c).apply { setBackgroundColor(0x22FFFFFF) }, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, 1))
        addView(chatScroll, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, 0, 1f))
    }
    private val clock = java.text.SimpleDateFormat("HH:mm", java.util.Locale.getDefault())
    private val dayFormat = java.text.SimpleDateFormat("d MMMM yyyy", java.util.Locale.getDefault())

    init {
        addView(scroll, LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT))
        addView(chatRoot, LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT))
        buildComposer()
        body.setPadding(c.dp(20), c.dp(16), c.dp(20), c.dp(16))
        Account.load(context)
        render()
        ui.postDelayed(poll, 1_500)
    }

    override fun libraryChanged() = Unit

    /** Called when the tab comes to the front or goes away. */
    fun visible(on: Boolean) {
        shown = on
        if (!on) Notifier.chatOpenWith = null
        if (on) {
            mode = Mode.Friends
            if (Account.signedIn) {
                loadFriends()
                Thread { Account.refresh(); ui.post { if (mode == Mode.Profile) render() } }.start()
            }
            render()
        }
    }

    /** Back button: leaves a chat or the profile first. */
    fun back(): Boolean {
        if (mode == Mode.Friends) return false
        mode = Mode.Friends
        render()
        loadFriends()
        return true
    }

    // ---- small building blocks ----

    private fun input(hint: String, password: Boolean = false, text: String = "") = EditText(c).apply {
        this.hint = hint
        setText(text)
        setHintTextColor(Ui.MUTED)
        setTextColor(Color.WHITE)
        setSingleLine()
        inputType = if (password) InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_VARIATION_PASSWORD else InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_VARIATION_EMAIL_ADDRESS
        background = roundRect(Ui.SURFACE, c.dp(24))
        setPadding(c.dp(18), c.dp(12), c.dp(18), c.dp(12))
    }

    private fun button(text: String, primary: Boolean = false, action: () -> Unit) = c.label(14f, if (primary) Color.BLACK else Color.WHITE, bold = true).apply {
        this.text = text
        gravity = Gravity.CENTER
        background = roundRect(if (primary) Color.WHITE else Ui.SURFACE, c.dp(22))
        setPadding(c.dp(18), c.dp(10), c.dp(18), c.dp(10))
        setOnClickListener { action() }
    }

    private fun wide(top: Int = 10) = LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT).apply { topMargin = c.dp(top) }

    private fun side(left: Int = 10) = LinearLayout.LayoutParams(ViewGroup.LayoutParams.WRAP_CONTENT, ViewGroup.LayoutParams.WRAP_CONTENT).apply { leftMargin = c.dp(left) }

    private fun title(text: String) = c.label(26f, Color.WHITE, bold = true).apply { this.text = text }

    private fun note(text: String, color: Int = Ui.MUTED) = c.label(14f, color, lines = 6).apply { this.text = text; setPadding(0, c.dp(8), 0, c.dp(4)) }

    private fun section(text: String) {
        body.addView(c.label(16f, Color.WHITE, bold = true).apply { this.text = text; setPadding(0, c.dp(22), 0, c.dp(6)) })
    }

    /** A round picture, or the person's first letter until (or unless) there is one. */
    private fun avatar(name: String, url: String?, size: Int): View {
        val letter = c.label(size / 2.6f, Color.WHITE, bold = true).apply {
            text = name.take(1).uppercase()
            gravity = Gravity.CENTER
        }
        val image = ImageView(c).apply { scaleType = ImageView.ScaleType.CENTER_CROP }
        val box = FrameLayout(c).apply {
            background = roundRect(Ui.RAISED, c.dp(size))
            clipRounded(c.dp(size) / 2)
            addView(letter, FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT))
            addView(image, FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT))
        }
        if (url != null) Images.load(image, url, c.dp(size))
        box.layoutParams = LinearLayout.LayoutParams(c.dp(size), c.dp(size))
        return box
    }

    /** A picture address from the profile; the server sends null when there is none, which optString turns into "null". */
    private fun link(json: JSONObject, key: String): String? = if (json.isNull(key)) null else json.optString(key).ifEmpty { null }

    private fun failed(reply: JSONObject): Boolean {
        val error = reply.optString("error")
        if (error.isNotEmpty()) c.say(error)
        return error.isNotEmpty()
    }

    // ---- screens ----

    private fun render() {
        Notifier.chatOpenWith = if (mode == Mode.Chat && shown) chatWith?.optString("id") else null
        val chat = Account.signedIn && mode == Mode.Chat
        val profile = Account.signedIn && (mode == Mode.Profile || mode == Mode.Person)
        scroll.visibility = if (chat) GONE else VISIBLE
        chatRoot.visibility = if (chat) VISIBLE else GONE
        body.removeAllViews()
        // The profile runs edge to edge so its cover can; everything else has a margin.
        body.setPadding(if (profile) 0 else c.dp(20), if (profile) 0 else c.dp(16), if (profile) 0 else c.dp(20), c.dp(16))
        when {
            !Account.signedIn -> signInForm()
            mode == Mode.Person -> drawPerson()
            profile -> drawProfile()
            chat -> drawChat()
            else -> drawFriends()
        }
    }

    private fun signInForm() {
        body.addView(title("Friends"))
        body.addView(note("Sign in with your Roy account to see your friends and what they are listening to. It is the same account as on the PC app, and your library follows you."))
        val login = input(if (creating) "Username" else "Email or username")
        val email = input("Email").takeIf { creating }
        val password = input("Password", password = true)
        val error = c.label(13f, 0xFFFF6B6B.toInt(), lines = 4)
        body.addView(login, wide())
        email?.let { body.addView(it, wide()) }
        body.addView(password, wide())
        body.addView(error, wide())
        body.addView(button(if (creating) "Create account" else "Sign in", true) {
            if (busy) return@button
            busy = true
            error.text = ""
            val (a, b, e) = Triple(login.text.toString().trim(), password.text.toString(), email?.text?.toString()?.trim())
            Thread {
                val problem = Account.enter(a, b, e)
                ui.post {
                    busy = false
                    if (problem != null) error.text = problem else signedIn()
                }
            }.start()
        }, wide())
        if (!creating) body.addView(button("Continue with Google") { google(error) }, wide())
        body.addView(button(if (creating) "I already have an account" else "Create an account") {
            creating = !creating
            render()
        }, wide())
        if (!creating) body.addView(c.label(13f, Ui.MUTED).apply {
            text = "Forgot your password?"
            setPadding(0, c.dp(16), 0, 0)
            setOnClickListener { forgot(login.text.toString().trim()) }
        })
    }

    private fun signedIn() {
        mode = Mode.Friends
        render()
        loadFriends()
        Sync.now()
    }

    /** Google sign-in happens in the browser; this waits for it to finish. */
    private fun google(error: TextView) {
        if (busy) return
        busy = true
        error.text = "Opening Google..."
        Thread {
            val start = Account.googleStart()
            if (start == null) {
                ui.post { busy = false; error.text = "Google sign-in is not available right now." }
                return@Thread
            }
            ui.post { c.startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(start.first)).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)); error.text = "Finish signing in with Google in your browser, then come back here." }
            val until = System.currentTimeMillis() + 10 * 60_000
            while (System.currentTimeMillis() < until) {
                Thread.sleep(2_000)
                val result = Account.googlePickup(start.second) ?: continue
                ui.post {
                    busy = false
                    if (result.isEmpty()) signedIn() else error.text = result
                }
                return@Thread
            }
            ui.post { busy = false; error.text = "Google sign-in expired. Try again." }
        }.start()
    }

    private fun forgot(login: String) {
        val field = input("Your email or username", text = login)
        dialog("Reset password", field, "Send code") {
            val email = field.text.toString().trim()
            Thread {
                val reply = Account.call("POST", "/password/forgot", JSONObject().put("login", email))
                ui.post { if (!failed(reply)) resetPassword(email) }
            }.start()
        }
    }

    private fun resetPassword(email: String) {
        val code = input("6-digit code").apply { inputType = InputType.TYPE_CLASS_NUMBER }
        val password = input("New password", password = true)
        val column = LinearLayout(c).apply {
            orientation = LinearLayout.VERTICAL
            addView(note("We emailed a code to $email. Check Spam or Junk if you do not see it."))
            addView(code, wide())
            addView(password, wide())
        }
        dialog("Enter the code", column, "Change password") {
            Thread {
                val reply = Account.call("POST", "/password/reset", JSONObject().put("login", email).put("code", code.text.toString().trim()).put("password", password.text.toString()))
                ui.post { if (!failed(reply)) c.say("Password changed. Sign in with it now.") }
            }.start()
        }
    }

    private fun dialog(heading: String, content: View, ok: String, action: () -> Unit) {
        val holder = LinearLayout(c).apply {
            setPadding(c.dp(20), c.dp(8), c.dp(20), 0)
            addView(content, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT))
        }
        (c as Activity).ask(heading, content = holder, ok = ok) { action() }
    }

    // ---- friends ----

    private fun loadFriends() {
        Thread {
            val reply = Account.call("GET", "/social")
            val found = Account.call("GET", "/friends/suggestions").optJSONArray("suggestions")
            val people = (0 until (found?.length() ?: 0)).map { found!!.getJSONObject(it) }
            ui.post {
                suggestions = people
                if (reply.has("error")) {
                    if (mode == Mode.Friends && !Account.signedIn) render()
                    social = reply
                } else {
                    social = reply
                }
                if (mode == Mode.Friends) render()
            }
        }.start()
    }

    private fun drawFriends() {
        body.addView(title("Friends"))
        val me = Account.profile
        val card = LinearLayout(c).apply {
            gravity = Gravity.CENTER_VERTICAL
            background = roundRect(Ui.SURFACE, c.dp(14))
            setPadding(c.dp(12), c.dp(10), c.dp(12), c.dp(10))
            addView(avatar(Account.name, link(me, "avatar_url"), 44))
            addView(LinearLayout(c).apply {
                orientation = LinearLayout.VERTICAL
                addView(c.label(15f, Color.WHITE, bold = true).apply { text = Account.name })
                addView(c.label(13f, Ui.MUTED).apply { text = (if (Account.invisible) "Invisible" else "Online") + "  -  ${Account.orbs} Orbs" })
            }, LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f).apply { leftMargin = c.dp(12) })
            setOnClickListener { mode = Mode.Profile; render() }
        }
        body.addView(card, wide(12))
        val actions = LinearLayout(c)
        actions.addView(button("Add friend", true) { addFriend() })
        body.addView(actions, wide(12))
        if (social.has("error")) {
            body.addView(note(social.getString("error")))
            return
        }
        val incoming = social.optJSONArray("incoming")
        if (incoming != null && incoming.length() > 0) {
            section("Requests")
            for (i in 0 until incoming.length()) request(incoming.getJSONObject(i))
        }
        val friends = social.optJSONArray("friends")
        section("Your friends")
        if (friends == null || friends.length() == 0) {
            body.addView(note("No friends yet. Add one by username."))
        } else {
            (0 until friends.length()).map { friends.getJSONObject(it) }.sortedByDescending { it.optBoolean("online") }.forEach(::friend)
        }
        if (suggestions.isNotEmpty()) {
            section("People you may know")
            suggestions.forEach(::suggestion)
        }
        val outgoing = social.optJSONArray("outgoing")
        if (outgoing != null && outgoing.length() > 0) {
            section("Waiting for an answer")
            for (i in 0 until outgoing.length()) outgoing.getJSONObject(i).let { p -> row(p, "Request sent").apply { background = pressable(); setOnClickListener { openPerson(p) } } }
        }
    }

    private fun row(person: JSONObject, line: String, extra: View? = null, badge: Int = 0): LinearLayout {
        val name = person.optString("display_name").ifEmpty { person.optString("username") }
        val texts = LinearLayout(c).apply {
            orientation = LinearLayout.VERTICAL
            addView(c.label(15f, Color.WHITE, bold = true).apply { text = name })
            addView(c.label(13f, Ui.MUTED).apply { text = line; setPadding(0, c.dp(2), 0, 0) })
        }
        val item = LinearLayout(c).apply {
            gravity = Gravity.CENTER_VERTICAL
            setPadding(0, c.dp(8), 0, c.dp(8))
            addView(avatar(name, Account.avatarUrl(person.optString("id"), person.optInt("avatar_version")), 44))
            addView(texts, LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f).apply { leftMargin = c.dp(12) })
            if (badge > 0) addView(c.label(12f, Color.BLACK, bold = true).apply {
                text = badge.toString()
                gravity = Gravity.CENTER
                background = roundRect(Ui.GREEN, c.dp(10))
                setPadding(c.dp(7), c.dp(2), c.dp(7), c.dp(2))
            })
            extra?.let { addView(it) }
        }
        body.addView(item)
        return item
    }

    private fun friend(person: JSONObject) {
        val listening = person.optJSONObject("listening")
        val line = when {
            listening != null -> (if (listening.optBoolean("playing")) "Listening to " else "Paused: ") + listening.optString("title") + " - " + listening.optString("artist")
            person.optBoolean("online") -> "Online"
            else -> "Offline"
        }
        val unread = social.optJSONObject("unread")?.optInt(person.optString("id")) ?: 0
        row(person, line, badge = unread).apply {
            background = pressable()
            setOnClickListener { openPerson(person) }
        }
    }

    private var viewing: JSONObject? = null
    private var details: JSONObject? = null

    /** Opens someone's profile card: cover, picture, bio, the friends you share, and a Message button. */
    private fun openPerson(person: JSONObject) {
        viewing = person
        details = null
        mode = Mode.Person
        render()
        val id = person.optString("id")
        Thread {
            val reply = Account.call("GET", "/people/$id")
            ui.post {
                if (viewing?.optString("id") == id) {
                    details = reply.takeIf { !it.has("error") }
                    if (mode == Mode.Person) render()
                }
            }
        }.start()
    }

    private fun drawPerson() {
        val person = viewing ?: return
        val info = details
        val id = person.optString("id")
        val name = person.optString("display_name")
        val relation = info?.optString("relation") ?: if (social.optJSONArray("friends")?.let { list -> (0 until list.length()).any { list.getJSONObject(it).optString("id") == id } } == true) "friend" else "none"
        val tint = intArrayOf(0xFF5B3A8E.toInt(), 0xFF8E3A5B.toInt(), 0xFF3A6E8E.toInt(), 0xFF8E6E3A.toInt(), 0xFF3A8E5B.toInt())[(name.hashCode() and 0x7fffffff) % 5]
        val cover = ImageView(c).apply {
            scaleType = ImageView.ScaleType.CENTER_CROP
            background = android.graphics.drawable.GradientDrawable(android.graphics.drawable.GradientDrawable.Orientation.TL_BR, intArrayOf(tint, 0xFF14141C.toInt()))
        }
        Account.bannerUrl(id, person.optInt("banner_version"))?.let { Images.load(cover, it, c.dp(480)) }
        body.addView(FrameLayout(c).apply {
            addView(cover, LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, c.dp(130)))
            addView(c.icon(R.drawable.ic_down, Color.WHITE, 40, 9).apply {
                rotation = 90f
                background = roundRect(0x99000000.toInt(), c.dp(20))
                setOnClickListener { back() }
            }, LayoutParams(c.dp(40), c.dp(40), Gravity.TOP or Gravity.START).apply { setMargins(c.dp(12), c.dp(12), 0, 0) })
            if (relation == "friend") addView(c.icon(R.drawable.ic_more, Color.WHITE, 40, 9).apply {
                background = roundRect(0x99000000.toInt(), c.dp(20))
                setOnClickListener { friendMenu(person, person.optJSONObject("listening")) }
            }, LayoutParams(c.dp(40), c.dp(40), Gravity.TOP or Gravity.END).apply { setMargins(0, c.dp(12), c.dp(12), 0) })
        }, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT))

        val listening = person.optJSONObject("listening")
        val online = person.optBoolean("online")
        val ring = FrameLayout(c).apply {
            background = roundRect(Ui.BG, c.dp(52))
            setPadding(c.dp(5), c.dp(5), c.dp(5), c.dp(5))
            addView(avatar(name, Account.avatarUrl(id, person.optInt("avatar_version")), 86), LayoutParams(c.dp(86), c.dp(86)))
            if (relation == "friend") addView(View(c).apply {
                background = roundRect(if (online) 0xFF23A55A.toInt() else 0xFF80848E.toInt(), c.dp(10)).also { it.setStroke(c.dp(3), Ui.BG) }
            }, LayoutParams(c.dp(24), c.dp(24), Gravity.BOTTOM or Gravity.END))
        }
        body.addView(ring, LinearLayout.LayoutParams(c.dp(96), c.dp(96)).apply { topMargin = -c.dp(46); marginStart = c.dp(14) })
        body.addView(c.label(25f, Color.WHITE, bold = true).apply { text = name; setPadding(c.dp(18), c.dp(8), c.dp(18), 0) })
        body.addView(c.label(14f, Ui.MUTED).apply { text = "@" + person.optString("username"); setPadding(c.dp(18), c.dp(2), c.dp(18), 0) })

        // Message and the rest, like the buttons under a Discord profile.
        fun wideButton(label: String, primary: Boolean, run: () -> Unit) = c.label(15f, if (primary) Color.BLACK else Color.WHITE, bold = true).apply {
            text = label
            gravity = Gravity.CENTER
            background = roundRect(if (primary) Color.WHITE else Ui.SURFACE, c.dp(24))
            setPadding(0, c.dp(13), 0, c.dp(13))
            setOnClickListener { run() }
        }
        val actions = LinearLayout(c)
        fun put(view: View) = actions.addView(view, LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f).apply { if (actions.childCount > 0) marginStart = c.dp(10) })
        when (relation) {
            "friend" -> {
                put(wideButton("Message", true) { chatWith = person; lastMessage = 0; mode = Mode.Chat; render(); loadChat(true) })
                put(wideButton("Blend", false) { Blend.make(c as Activity, host, id, name) })
            }
            "incoming" -> {
                put(wideButton("Accept", true) { answer(person, "accept"); mode = Mode.Friends; render() })
                put(wideButton("Decline", false) { answer(person, "decline"); mode = Mode.Friends; render() })
            }
            "outgoing" -> put(wideButton("Request sent", false) {})
            "none" -> put(wideButton("Add friend", true) {
                Thread {
                    val reply = Account.call("POST", "/friends/request", JSONObject().put("username", person.optString("username")))
                    ui.post { if (!failed(reply)) c.say(if (reply.optBoolean("accepted")) "You are now friends" else "Request sent to $name"); loadFriends(); openPerson(person) }
                }.start()
            })
        }
        if (actions.childCount > 0) body.addView(actions, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT).apply { setMargins(c.dp(16), c.dp(14), c.dp(16), 0) })

        // The friends you both have: little overlapping pictures and a count.
        val mutual = info?.optJSONArray("mutual")
        if (mutual != null && mutual.length() > 0) {
            val people = (0 until mutual.length()).map { mutual.getJSONObject(it) }
            val faces = LinearLayout(c)
            people.take(4).forEachIndexed { i, p ->
                faces.addView(FrameLayout(c).apply {
                    background = roundRect(Ui.SURFACE, c.dp(15))
                    setPadding(c.dp(2), c.dp(2), c.dp(2), c.dp(2))
                    addView(avatar(p.optString("display_name"), Account.avatarUrl(p.optString("id"), p.optInt("avatar_version")), 26), LayoutParams(c.dp(26), c.dp(26)))
                }, LinearLayout.LayoutParams(c.dp(30), c.dp(30)).apply { if (i > 0) marginStart = -c.dp(8) })
            }
            card(LinearLayout(c).apply {
                gravity = Gravity.CENTER_VERTICAL
                background = pressable()
                addView(faces)
                addView(c.label(15f, Color.WHITE, bold = true).apply {
                    text = if (people.size == 1) "1 mutual friend" else "${people.size} mutual friends"
                    setPadding(c.dp(12), 0, 0, 0)
                }, LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f))
                addView(c.label(18f, Ui.MUTED).apply { text = "›" })
                setOnClickListener {
                    Sheet.show(c as Activity, "Mutual friends", "You and $name", null, people.map { p ->
                        Sheet.Option(p.optString("display_name") + "  @" + p.optString("username")) { openPerson(p) }
                    })
                }
            })
        }

        if (listening != null) card(
            c.label(14f, Ui.MUTED, bold = true).apply { text = if (listening.optBoolean("playing")) "Listening to" else "Paused" },
            c.label(16f, Color.WHITE, bold = true).apply { text = listening.optString("title"); setPadding(0, c.dp(6), 0, 0) },
            c.label(14f, Ui.GREEN).apply { text = listening.optString("artist") },
        ).apply { setOnClickListener { host.search("${listening.optString("title")} ${listening.optString("artist")}") } }

        fun date(seconds: Long) = java.text.SimpleDateFormat("d MMMM yyyy", java.util.Locale.getDefault()).format(java.util.Date(seconds * 1000))
        val bio = person.optString("bio").ifEmpty { info?.optString("bio").orEmpty() }
        val facts = ArrayList<View>()
        facts += c.label(14f, Ui.MUTED, bold = true).apply { text = "About me" }
        facts += c.label(15f, Color.WHITE, lines = 10).apply { text = bio.ifEmpty { "Nothing here yet." }; ellipsize = null; setPadding(0, c.dp(6), 0, c.dp(12)) }
        if (info != null && info.optLong("joined") > 0) {
            facts += c.label(14f, Ui.MUTED, bold = true).apply { text = "Member since" }
            facts += c.label(15f, Color.WHITE).apply { text = date(info.optLong("joined")); setPadding(0, c.dp(6), 0, c.dp(12)) }
        }
        if (info != null && !info.isNull("friends_since") && info.optLong("friends_since") > 0) {
            facts += c.label(14f, Ui.MUTED, bold = true).apply { text = "Friends since" }
            facts += c.label(15f, Color.WHITE).apply { text = date(info.optLong("friends_since")); setPadding(0, c.dp(6), 0, 0) }
        }
        card(*facts.toTypedArray())
        body.addView(View(c), LinearLayout.LayoutParams(1, c.dp(24)))
    }

    private fun friendMenu(person: JSONObject, listening: JSONObject?) {
        val options = ArrayList<Pair<String, () -> Unit>>()
        options += "Message" to { chatWith = person; lastMessage = 0; mode = Mode.Chat; render(); loadChat(true) }
        options += "Blend our music" to { Blend.make(c as Activity, host, person.optString("id"), person.optString("display_name")) }
        if (listening != null) options += "Find what they are playing" to { host.search("${listening.optString("title")} ${listening.optString("artist")}") }
        options += "Remove friend" to {
            (c as Activity).ask("Remove ${person.optString("display_name")}?", "Your chat with them is deleted too.", ok = "Remove", danger = true) {
                Thread { Account.call("DELETE", "/friends/${person.optString("id")}"); loadFriends() }.start()
            }
        }
        Sheet.show(c as Activity, person.optString("display_name"), "@" + person.optString("username"), Account.avatarUrl(person.optString("id"), person.optInt("avatar_version")),
            options.map { (label, run) -> Sheet.Option(label, color = if (label == "Remove friend") 0xFFFF6B6B.toInt() else Color.WHITE, run = run) })
    }

    private fun request(person: JSONObject) {
        val buttons = LinearLayout(c).apply {
            addView(button("Accept", true) { answer(person, "accept") })
            addView(button("Decline") { answer(person, "decline") }, side(8))
        }
        row(person, "@" + person.optString("username"), buttons).apply { background = pressable(); setOnClickListener { openPerson(person) } }
    }

    /** Someone your friends know: one tap sends the request. */
    private fun suggestion(person: JSONObject) {
        val mutual = person.optInt("mutual")
        val line = "@" + person.optString("username") + "  •  " + (if (mutual == 1) "1 mutual friend" else "$mutual mutual friends")
        val add = button("Add", true) {
            Thread {
                val reply = Account.call("POST", "/friends/request", JSONObject().put("username", person.optString("username")))
                ui.post {
                    if (!failed(reply)) c.say(if (reply.optBoolean("accepted")) "You are now friends" else "Request sent to ${person.optString("display_name")}")
                    loadFriends()
                }
            }.start()
        }
        row(person, line, add).apply { background = pressable(); setOnClickListener { openPerson(person) } }
    }

    private fun answer(person: JSONObject, what: String) {
        Thread {
            Account.call("POST", "/friends/$what", JSONObject().put("id", person.optString("id")))
            loadFriends()
        }.start()
    }

    private fun addFriend() {
        val field = input("Their username")
        dialog("Add friend", field, "Send request") {
            val name = field.text.toString().trim()
            Thread {
                val reply = Account.call("POST", "/friends/request", JSONObject().put("username", name))
                ui.post {
                    if (!failed(reply)) c.say(if (reply.optBoolean("accepted")) "You are now friends" else "Request sent")
                    loadFriends()
                }
            }.start()
        }
    }

    private fun buildComposer() {
        val send = c.icon(R.drawable.ic_send, Color.BLACK, 40, 10).apply { background = roundRect(Color.WHITE, c.dp(20)) }
        val pill = LinearLayout(c).apply {
            gravity = Gravity.CENTER_VERTICAL
            background = roundRect(Ui.SURFACE, c.dp(26))
            setPadding(0, c.dp(2), c.dp(6), c.dp(2))
            addView(chatField, LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f))
            addView(send)
        }
        chatRoot.addView(pill, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT).apply { setMargins(c.dp(12), c.dp(6), c.dp(12), c.dp(12)) })
        send.setOnClickListener {
            val person = chatWith ?: return@setOnClickListener
            val text = chatField.text.toString().trim()
            if (text.isEmpty()) return@setOnClickListener
            chatField.setText("")
            // Show it at once; the server's answer replaces this placeholder.
            val placeholder = JSONObject().put("id", -1).put("mine", true).put("body", text).put("at", System.currentTimeMillis() / 1000)
            messages += placeholder
            drawMessages()
            Thread {
                val reply = Account.call("POST", "/messages/${person.optString("id")}", JSONObject().put("body", text))
                ui.post {
                    messages.remove(placeholder)
                    if (!failed(reply) && messages.none { it.optLong("id") == reply.optLong("id") }) {
                        messages += reply
                        lastMessage = maxOf(lastMessage, reply.optLong("id"))
                    }
                    drawMessages()
                }
            }.start()
        }
    }

    private fun drawChat() {
        val person = chatWith ?: return
        chatHead.removeAllViews()
        chatHead.addView(c.icon(R.drawable.ic_down, Color.WHITE, 44, 10).apply { rotation = 90f; setOnClickListener { back() } })
        chatHead.addView(avatar(person.optString("display_name"), Account.avatarUrl(person.optString("id"), person.optInt("avatar_version")), 38), side(2))
        chatHead.addView(LinearLayout(c).apply {
            orientation = LinearLayout.VERTICAL
            addView(c.label(17f, Color.WHITE, bold = true).apply { text = person.optString("display_name") })
            addView(c.label(12f, Ui.MUTED).apply { text = "@" + person.optString("username") })
        }, side(12))
        drawMessages()
    }

    /** Messages the way Discord lays them out: a name and time above each run of messages, then plain lines. */
    private fun drawMessages() {
        val person = chatWith ?: return
        chatList.removeAllViews()
        var previousMine: Boolean? = null
        var previousAt = 0L
        var previousDay = ""
        for (message in messages) {
            val at = message.optLong("at")
            val mine = message.optBoolean("mine")
            val day = dayFormat.format(java.util.Date(at * 1000))
            if (day != previousDay) {
                chatList.addView(LinearLayout(c).apply {
                    gravity = Gravity.CENTER_VERTICAL
                    addView(View(c).apply { setBackgroundColor(0x22FFFFFF) }, LinearLayout.LayoutParams(0, 1, 1f))
                    addView(c.label(12f, Ui.MUTED, bold = true).apply { text = day; setPadding(c.dp(10), 0, c.dp(10), 0) })
                    addView(View(c).apply { setBackgroundColor(0x22FFFFFF) }, LinearLayout.LayoutParams(0, 1, 1f))
                }, wide(14))
            }
            val startsRun = day != previousDay || previousMine != mine || at - previousAt > 300
            if (startsRun) {
                val name = if (mine) Account.name else person.optString("display_name")
                val picture = if (mine) link(Account.profile, "avatar_url") else Account.avatarUrl(person.optString("id"), person.optInt("avatar_version"))
                chatList.addView(LinearLayout(c).apply {
                    gravity = Gravity.CENTER_VERTICAL
                    addView(avatar(name, picture, 40))
                    addView(c.label(15f, Color.WHITE, bold = true).apply { text = name }, side(12))
                    addView(c.label(12f, Ui.MUTED).apply { text = clock.format(java.util.Date(at * 1000)) }, side(8))
                }, wide(14))
            }
            chatList.addView(c.label(15f, 0xFFE6E6E6.toInt(), lines = 100).apply {
                text = message.optString("body")
                ellipsize = null
                setTextIsSelectable(true)
                setPadding(c.dp(52), c.dp(if (startsRun) 4 else 2), 0, 0)
            })
            previousMine = mine
            previousAt = at
            previousDay = day
        }
        if (messages.isEmpty()) chatList.addView(note("No messages yet. Say hello to ${person.optString("display_name")}!"))
        chatScroll.post { chatScroll.fullScroll(View.FOCUS_DOWN) }
    }

    private fun loadChat(first: Boolean) {
        val person = chatWith ?: return
        if (first) messages.clear()
        Thread {
            val reply = Account.call("GET", "/messages/${person.optString("id")}?after=$lastMessage&read=1")
            val found = reply.optJSONArray("messages")
            ui.post {
                if (mode != Mode.Chat || found == null) return@post
                var news = false
                for (i in 0 until found.length()) {
                    val m = found.getJSONObject(i)
                    if (m.optLong("id") > lastMessage) {
                        if (m.optBoolean("mine")) messages.removeAll { it.optLong("id") < 0 && it.optString("body") == m.optString("body") }
                        messages += m
                        lastMessage = m.optLong("id")
                        news = true
                    }
                }
                if (news || first) drawMessages()
            }
        }.start()
    }

    // ---- profile ----

    private fun card(vararg views: View) = LinearLayout(c).apply {
        orientation = LinearLayout.VERTICAL
        background = roundRect(Ui.SURFACE, c.dp(16))
        setPadding(c.dp(16), c.dp(14), c.dp(16), c.dp(14))
        views.forEach { addView(it) }
    }.also { body.addView(it, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT).apply { setMargins(c.dp(16), c.dp(12), c.dp(16), 0) }) }

    /** A tappable line inside a card: a label and an arrow. */
    private fun action(text: String, color: Int = Color.WHITE, run: () -> Unit) = LinearLayout(c).apply {
        gravity = Gravity.CENTER_VERTICAL
        background = pressable()
        setPadding(0, c.dp(13), 0, c.dp(13))
        addView(c.label(15f, color).apply { this.text = text }, LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f))
        addView(c.label(18f, Ui.MUTED).apply { this.text = "›" })
        setOnClickListener { run() }
    }

    private fun drawProfile() {
        val me = Account.profile
        val green = 0xFF1B5E3A.toInt()
        // Cover, with the back button floating over it.
        val cover = ImageView(c).apply {
            scaleType = ImageView.ScaleType.CENTER_CROP
            background = android.graphics.drawable.GradientDrawable(android.graphics.drawable.GradientDrawable.Orientation.TL_BR, intArrayOf(green, 0xFF0E2B1C.toInt()))
        }
        link(me, "banner_url")?.let { Images.load(cover, it, c.dp(480)) }
        val top = FrameLayout(c).apply {
            addView(cover, LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, c.dp(130)))
            addView(c.icon(R.drawable.ic_down, Color.WHITE, 40, 9).apply {
                rotation = 90f
                background = roundRect(0x99000000.toInt(), c.dp(20))
                setOnClickListener { back() }
            }, LayoutParams(c.dp(40), c.dp(40), Gravity.TOP or Gravity.START).apply { setMargins(c.dp(12), c.dp(12), 0, 0) })
        }
        body.addView(top, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT))

        // The picture sits half over the cover with a ring in the page colour, and a status dot like Discord's.
        val ring = FrameLayout(c).apply {
            background = roundRect(Ui.BG, c.dp(52))
            setPadding(c.dp(5), c.dp(5), c.dp(5), c.dp(5))
            addView(avatar(Account.name, link(me, "avatar_url"), 86), LayoutParams(c.dp(86), c.dp(86)))
            addView(View(c).apply {
                background = roundRect(if (Account.invisible) 0xFF80848E.toInt() else 0xFF23A55A.toInt(), c.dp(10)).also { it.setStroke(c.dp(3), Ui.BG) }
            }, LayoutParams(c.dp(24), c.dp(24), Gravity.BOTTOM or Gravity.END))
        }
        body.addView(ring, LinearLayout.LayoutParams(c.dp(96), c.dp(96)).apply { topMargin = -c.dp(46); marginStart = c.dp(14) })

        body.addView(c.label(25f, Color.WHITE, bold = true).apply { text = Account.name; setPadding(c.dp(18), c.dp(8), c.dp(18), 0) })
        body.addView(c.label(14f, Ui.MUTED).apply { text = "@${Account.username}"; setPadding(c.dp(18), c.dp(2), c.dp(18), 0) })
        body.addView(c.label(15f, Color.BLACK, bold = true).apply {
            text = "Edit profile"
            gravity = Gravity.CENTER
            background = roundRect(Color.WHITE, c.dp(24))
            setPadding(0, c.dp(13), 0, c.dp(13))
            setOnClickListener { editProfile() }
        }, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT).apply { setMargins(c.dp(16), c.dp(14), c.dp(16), 0) })

        if (!me.optBoolean("email_verified")) {
            card(
                c.label(14f, 0xFFFFB84D.toInt(), bold = true).apply { text = "Verify your email" },
                c.label(13f, Ui.MUTED, lines = 3).apply { text = "${me.optString("email")} is not verified yet."; ellipsize = null; setPadding(0, c.dp(4), 0, c.dp(4)) },
                action("Enter the code") { verifyEmail() },
                action("Send a new code") { Thread { val r = Account.call("POST", "/email/resend"); ui.post { if (!failed(r)) c.say("Code sent. Check Spam or Junk too.") } }.start() },
                action("Change email") { changeEmail() },
            )
        }

        // Orbs, like the balance card on Discord's profile.
        card(LinearLayout(c).apply {
            gravity = Gravity.CENTER_VERTICAL
            addView(c.label(15f, Color.WHITE, bold = true).apply { text = "Orbs balance" }, LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f))
            addView(c.label(16f, Color.WHITE, bold = true).apply {
                text = "◆  ${Account.orbs}"
                background = roundRect(Ui.RAISED, c.dp(14))
                setPadding(c.dp(14), c.dp(7), c.dp(14), c.dp(7))
            })
        }, c.label(12f, Ui.MUTED, lines = 2).apply { text = "Orbs are spent in the Roy Music shop, coming later."; ellipsize = null; setPadding(0, c.dp(8), 0, 0) })

        val bio = me.optString("bio")
        card(
            c.label(14f, Ui.MUTED, bold = true).apply { text = "About me" },
            c.label(15f, Color.WHITE, lines = 10).apply { text = bio.ifEmpty { "Nothing here yet. Tap Edit profile to add a bio." }; ellipsize = null; setPadding(0, c.dp(6), 0, c.dp(12)) },
            c.label(14f, Ui.MUTED, bold = true).apply { text = "Member since" },
            c.label(15f, Color.WHITE).apply {
                text = java.text.SimpleDateFormat("d MMMM yyyy", java.util.Locale.getDefault()).format(java.util.Date(me.optLong("created_at") * 1000))
                setPadding(0, c.dp(6), 0, 0)
            },
        )

        val online = c.label(14f, if (Account.invisible) Color.WHITE else Color.BLACK, bold = true).apply {
            text = "Online"; gravity = Gravity.CENTER
            background = roundRect(if (Account.invisible) Ui.RAISED else Color.WHITE, c.dp(20))
            setPadding(0, c.dp(10), 0, c.dp(10))
            setOnClickListener { setStatus(false) }
        }
        val hidden = c.label(14f, if (Account.invisible) Color.BLACK else Color.WHITE, bold = true).apply {
            text = "Invisible"; gravity = Gravity.CENTER
            background = roundRect(if (Account.invisible) Color.WHITE else Ui.RAISED, c.dp(20))
            setPadding(0, c.dp(10), 0, c.dp(10))
            setOnClickListener { setStatus(true) }
        }
        card(
            c.label(14f, Ui.MUTED, bold = true).apply { text = "Status" },
            LinearLayout(c).apply {
                addView(online, LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f).apply { marginEnd = c.dp(6) })
                addView(hidden, LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f).apply { marginStart = c.dp(6) })
                setPadding(0, c.dp(10), 0, 0)
            },
        )

        card(
            action("Change profile picture") { pick(11) },
            action("Change cover") { pick(12) },
            *listOfNotNull(
                if (link(me, "avatar_url") != null) action("Remove profile picture", 0xFFFF6B6B.toInt()) { removeImage("/me/avatar") } else null,
                if (link(me, "banner_url") != null) action("Remove cover", 0xFFFF6B6B.toInt()) { removeImage("/me/banner") } else null,
            ).toTypedArray(),
        )

        val share = android.widget.Switch(c).apply {
            isChecked = me.optBoolean("share_taste", true)
            setOnCheckedChangeListener { _, on -> Thread { Account.accept(Account.call("PATCH", "/me", JSONObject().put("share_taste", on))) }.start() }
        }
        card(
            LinearLayout(c).apply {
                gravity = Gravity.CENTER_VERTICAL
                setPadding(0, c.dp(6), 0, c.dp(6))
                addView(c.label(15f, Color.WHITE).apply { text = "Let friends Blend with my music" }, LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f))
                addView(share)
            },
            action(if (me.optBoolean("has_password")) "Change password" else "Set a password") { changePassword(me.optBoolean("has_password")) },
            c.label(12f, Ui.MUTED).apply { text = "Your library follows your account on every device. " + Sync.status; setPadding(0, c.dp(4), 0, 0) },
        )
        card(action("Sign out", 0xFFFF6B6B.toInt()) {
            Thread { Account.logout(); ui.post { mode = Mode.Friends; render() } }.start()
        })
        body.addView(View(c), LinearLayout.LayoutParams(1, c.dp(24)))
    }

    private fun setStatus(invisible: Boolean) {
        Account.setInvisible(invisible)
        // The player may only be asked from the main thread, so read it before going to the network.
        val song = Playback.song
        val position = Playback.controller?.currentPosition ?: 0
        val playing = Playback.controller?.isPlaying == true
        render()
        Thread { Account.presence(song, position, playing) }.start()
    }

    private fun editProfile() {
        val me = Account.profile
        val name = input("Display name", text = me.optString("display_name"))
        val user = input("Username", text = me.optString("username"))
        val bio = input("Bio", text = me.optString("bio"))
        var birthday = me.optString("birthday")
        val pick = button(if (birthday.isEmpty()) "Add your birthday" else "Birthday: $birthday") {}
        pick.setOnClickListener {
            val now = java.util.Calendar.getInstance()
            val old = birthday.split('-').mapNotNull { it.toIntOrNull() }
            val (y, m, d) = if (old.size == 3) old else listOf(now.get(java.util.Calendar.YEAR) - 20, now.get(java.util.Calendar.MONTH) + 1, now.get(java.util.Calendar.DAY_OF_MONTH))
            android.app.DatePickerDialog(c, { _, year, month, day ->
                birthday = "%04d-%02d-%02d".format(year, month + 1, day)
                pick.text = "Birthday: $birthday"
            }, y, m - 1, d).apply { datePicker.maxDate = System.currentTimeMillis() }.show()
        }
        val column = LinearLayout(c).apply {
            orientation = LinearLayout.VERTICAL
            addView(name, wide(0))
            addView(user, wide())
            addView(bio, wide())
            addView(pick, wide())
            addView(note("Only you see it."), wide(6))
        }
        dialog("Edit profile", column, "Save") {
            val patch = JSONObject().put("display_name", name.text.toString().trim()).put("bio", bio.text.toString().trim())
            if (birthday != me.optString("birthday")) patch.put("birthday", birthday)
            if (user.text.toString().trim().lowercase() != me.optString("username")) patch.put("username", user.text.toString().trim())
            Thread {
                val reply = Account.call("PATCH", "/me", patch)
                val error = Account.accept(reply)
                ui.post { if (error != null) c.say(error); render() }
            }.start()
        }
    }

    private fun changePassword(has: Boolean) {
        val current = input("Current password", password = true)
        val next = input("New password (6 or more)", password = true)
        val column = LinearLayout(c).apply {
            orientation = LinearLayout.VERTICAL
            if (has) addView(current, wide(0))
            addView(next, wide())
        }
        dialog(if (has) "Change password" else "Set a password", column, "Save") {
            Thread {
                val reply = Account.call("PUT", "/me/password", JSONObject().put("current", current.text.toString()).put("password", next.text.toString()))
                val error = Account.accept(reply)
                ui.post { c.say(error ?: "Password saved"); render() }
            }.start()
        }
    }

    private fun verifyEmail() {
        val code = input("6-digit code").apply { inputType = InputType.TYPE_CLASS_NUMBER }
        dialog("Verify your email", code, "Verify") {
            Thread {
                val error = Account.accept(Account.call("POST", "/email/verify", JSONObject().put("code", code.text.toString().trim())))
                ui.post { c.say(error ?: "Email verified"); render() }
            }.start()
        }
    }

    private fun changeEmail() {
        val field = input("New email")
        dialog("Change email", field, "Send code") {
            Thread {
                val error = Account.accept(Account.call("PUT", "/me/email", JSONObject().put("email", field.text.toString().trim())))
                ui.post { c.say(error ?: "We sent a code to the new address"); render() }
            }.start()
        }
    }

    private fun removeImage(path: String) {
        Thread {
            val error = Account.accept(Account.call("DELETE", path))
            ui.post { if (error != null) c.say(error); render() }
        }.start()
    }

    // ---- pictures ----

    private fun pick(code: Int) {
        picking = code
        (c as Activity).startActivityForResult(Intent(Intent.ACTION_GET_CONTENT).setType("image/*"), code)
    }

    /** The picture the user chose: it is cropped on screen first, then shrunk to the server's limits and sent. */
    fun picked(uri: Uri) {
        val avatar = picking == 11
        host.crop(uri, avatar, if (avatar) 384 else 960, if (avatar) 384 else 320) { cropped ->
            if (cropped == null) return@crop
            Thread {
                val bytes = compress(cropped, if (avatar) 140_000 else 380_000)
                val error = Account.accept(Account.upload(if (avatar) "/me/avatar" else "/me/banner", bytes, "image/jpeg"))
                ui.post { if (error != null) c.say(error); render() }
            }.start()
        }
    }

    private fun compress(bitmap: Bitmap, limit: Int): ByteArray {
        var quality = 90
        var out = ByteArrayOutputStream()
        bitmap.compress(Bitmap.CompressFormat.JPEG, quality, out)
        while (out.size() > limit && quality > 30) {
            quality -= 10
            out = ByteArrayOutputStream()
            bitmap.compress(Bitmap.CompressFormat.JPEG, quality, out)
        }
        return out.toByteArray()
    }
}
