package org.roynit.music

import android.app.AlertDialog
import android.content.Context
import android.graphics.Color
import android.text.InputType
import android.view.Gravity
import android.view.View
import android.view.ViewGroup
import android.view.inputmethod.EditorInfo
import android.view.inputmethod.InputMethodManager
import android.graphics.drawable.GradientDrawable
import android.widget.BaseAdapter
import android.widget.ScrollView
import android.widget.EditText
import android.widget.FrameLayout
import android.widget.HorizontalScrollView
import android.widget.ImageView
import android.widget.LinearLayout
import android.widget.ListView
import android.widget.TextView
import org.json.JSONObject
import java.util.Calendar

/** A screen of the app. Pages are views kept in one frame; the one in front is the one shown. */
abstract class Page(context: Context, protected val host: SongHost) : FrameLayout(context) {
    protected val c: Context = context
    protected val adapter = SongAdapter(context, host)

    fun nowPlaying(id: String?) {
        adapter.current = id
    }

    open fun libraryChanged() = adapter.notifyDataSetChanged()
}

private fun Context.status(): TextView = label(14f, Ui.MUTED, lines = 3).apply {
    gravity = Gravity.CENTER
    setPadding(dp(24), dp(12), dp(24), dp(12))
}

/**
 * Listen now, like the PC app: a greeting, a grid of quick picks, mixes made from what you play, what you played
 * lately, recommendations, and YouTube Music's own feed with Charts and radios.
 */
class HomePage(context: Context, host: SongHost) : Page(context, host) {
    private val list = ListView(context).apply { divider = null; setBackgroundColor(Ui.BG) }
    private val body = LinearLayout(context).apply { orientation = LinearLayout.VERTICAL }
    private val status = c.status()
    private val feedTitle = c.label(20f, Color.WHITE, bold = true).apply { text = "From YouTube Music" }
    private var mixes = emptyList<Pair<String, List<Song>>>()
    private var picks = emptyList<Song>()
    private var loaded = false
    private var retried = false
    private var mixing = false
    private var chip = "All"
    private var friendsNow = emptyList<JSONObject>()

    init {
        val head = LinearLayout(c).apply {
            gravity = Gravity.CENTER_VERTICAL
            addView(feedTitle, LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f))
            addView(c.icon(R.drawable.ic_more, Ui.MUTED, 40, 9).apply { setOnClickListener { feedMenu(it) } })
            setPadding(c.dp(16), c.dp(22), c.dp(8), c.dp(4))
        }
        val header = LinearLayout(c).apply {
            orientation = LinearLayout.VERTICAL
            addView(body)
            addView(head)
            addView(status)
        }
        list.addHeaderView(header, null, false)
        list.adapter = adapter
        addView(list, LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT))
        status.setOnClickListener { load(force = true) }
        draw()
    }

    private var drawn = ""

    /** What the page shows, as one string: when it has not changed there is nothing to rebuild. */
    private fun signature() = listOf(chip, Library.favorites.size, Library.history.take(12).joinToString(",") { it.id }, mixes.size, picks.size, friendsNow.size, Account.signedIn).joinToString("|")

    private val redraw = Runnable { if (signature() != drawn) draw() }

    /** The library changes in bursts (a play, a sync); the page is rebuilt once the burst is over, not for each change. */
    override fun libraryChanged() {
        super.libraryChanged()
        removeCallbacks(redraw)
        postDelayed(redraw, 350)
    }

    private fun heading(text: String, extra: String? = null, action: (() -> Unit)? = null) = LinearLayout(c).apply {
        gravity = Gravity.CENTER_VERTICAL
        setPadding(c.dp(16), c.dp(22), c.dp(16), c.dp(10))
        addView(c.label(20f, Color.WHITE, bold = true).apply { this.text = text }, LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f))
        if (extra != null) addView(c.label(13f, Ui.MUTED).apply { this.text = extra; setOnClickListener { action?.invoke() } })
    }

    private fun cover(song: Song?, size: Int, radius: Int = 8) = ImageView(c).apply {
        scaleType = ImageView.ScaleType.CENTER_CROP
        background = roundRect(Ui.RAISED, c.dp(radius))
        clipRounded(c.dp(radius))
        if (song != null) Images.load(this, song.cover, c.dp(size))
    }

    /** The whole page above the YouTube feed; cheap to rebuild whenever the library or the mixes change. */
    private fun draw() {
        drawn = signature()
        body.removeAllViews()
        // Spotify's top row: your picture, then chips that choose what to show.
        val avatar = ImageView(c).apply {
            scaleType = ImageView.ScaleType.CENTER_CROP
            background = roundRect(Ui.GREEN, c.dp(16))
            clipRounded(c.dp(16))
            setOnClickListener { host.drawer() }
        }
        val mine = Account.profile.optString("avatar_url")
        if (Account.signedIn && mine.isNotEmpty() && mine != "null") Images.load(avatar, mine, c.dp(40))
        val top = LinearLayout(c).apply {
            gravity = Gravity.CENTER_VERTICAL
            setPadding(c.dp(16), c.dp(18), c.dp(8), c.dp(10))
            addView(avatar, LinearLayout.LayoutParams(c.dp(32), c.dp(32)))
            val row = LinearLayout(c).apply { setPadding(c.dp(12), 0, c.dp(8), 0) }
            for (name in listOf("All", "Mixes", "Recent", "Friends")) {
                row.addView(c.label(14f, if (chip == name) Color.BLACK else Color.WHITE, bold = true).apply {
                    text = name
                    background = roundRect(if (chip == name) Ui.GREEN else Ui.SURFACE, c.dp(18))
                    setPadding(c.dp(16), c.dp(8), c.dp(16), c.dp(8))
                    setOnClickListener { chip = name; if (name == "Friends") loadFriendsNow(); draw() }
                }, LinearLayout.LayoutParams(ViewGroup.LayoutParams.WRAP_CONTENT, ViewGroup.LayoutParams.WRAP_CONTENT).apply { marginEnd = c.dp(8) })
            }
            addView(HorizontalScrollView(c).apply { isHorizontalScrollBarEnabled = false; addView(row) })
        }
        body.addView(top)
        when (chip) {
            "Mixes" -> {
                if (mixes.isNotEmpty()) mixStrip() else body.addView(note("Your mixes appear after you like or play a few songs."))
                if (picks.isNotEmpty()) strip("Recommended for you", picks)
            }
            "Recent" -> {
                quickPicks()
                recent()
            }
            "Friends" -> friendActivity()
            else -> {
                quickPicks()
                if (mixes.isNotEmpty()) mixStrip()
                recent()
                if (picks.isNotEmpty()) strip("Recommended for you", picks)
            }
        }
    }

    private fun note(text: String) = c.label(14f, Ui.MUTED, lines = 3).apply { this.text = text; setPadding(c.dp(16), c.dp(16), c.dp(16), c.dp(8)) }

    /** What friends are listening to right now: tap one to listen along. */
    private fun loadFriendsNow() {
        if (!Account.signedIn) return
        Thread {
            val reply = Account.call("GET", "/social")
            val friends = reply.optJSONArray("friends")
            val now = (0 until (friends?.length() ?: 0)).map { friends!!.getJSONObject(it) }.filter { it.optJSONObject("listening") != null }
            post {
                friendsNow = now
                if (chip == "Friends") draw()
            }
        }.start()
    }

    private fun friendActivity() {
        body.addView(heading("Friend activity"))
        if (!Account.signedIn) {
            body.addView(note("Sign in from the Friends tab to see what your friends are listening to."))
            return
        }
        if (friendsNow.isEmpty()) {
            body.addView(note("Nobody is listening right now."))
            return
        }
        for (friend in friendsNow) {
            val song = friend.getJSONObject("listening")
            val name = friend.optString("display_name")
            val avatar = ImageView(c).apply {
                scaleType = ImageView.ScaleType.CENTER_CROP
                background = roundRect(Ui.RAISED, c.dp(22))
                clipRounded(c.dp(22))
            }
            Account.avatarUrl(friend.optString("id"), friend.optInt("avatar_version"))?.let { Images.load(avatar, it, c.dp(44)) }
            val row = LinearLayout(c).apply {
                gravity = Gravity.CENTER_VERTICAL
                setPadding(c.dp(16), c.dp(8), c.dp(16), c.dp(8))
                background = pressable()
                addView(avatar, LinearLayout.LayoutParams(c.dp(44), c.dp(44)))
                addView(
                    LinearLayout(c).apply {
                        orientation = LinearLayout.VERTICAL
                        addView(c.label(15f, Color.WHITE, bold = true).apply { text = name })
                        addView(c.label(13f, Ui.MUTED).apply { text = song.optString("title") + " \u2022 " + song.optString("artist") })
                    },
                    LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f).apply { marginStart = c.dp(12) },
                )
                addView(c.label(12f, Color.BLACK, bold = true).apply {
                    text = "Listen along"
                    background = roundRect(Ui.GREEN, c.dp(14))
                    setPadding(c.dp(12), c.dp(6), c.dp(12), c.dp(6))
                })
                setOnClickListener { listenAlong(name, song.optString("title"), song.optString("artist")) }
            }
            body.addView(row)
        }
    }

    /** Plays what a friend is playing, then carries on with songs like it. */
    private fun listenAlong(name: String, title: String, artist: String) {
        c.say("Listening along with $name...")
        Thread {
            val first = songsOf(JSONObject(Core.search("$title $artist"))).firstOrNull()
            val rest = if (first == null) emptyList() else suggestionsOf(JSONObject(Core.radio(first.id, 30))).filter { it.id != first.id }
            post {
                if (first == null) {
                    c.say("Could not find that song")
                } else {
                    Playback.nextSource = "$name's session"
                    host.play(listOf(first) + rest, 0)
                }
            }
        }.start()
    }

    private fun quickPicks() {
        class Pick(val title: String, val song: Song?, val go: () -> Unit)
        val tiles = ArrayList<Pick>()
        if (Library.favorites.isNotEmpty()) tiles += Pick("Liked songs", Library.favorites.first()) { host.play(Library.favorites.toList(), 0) }
        mixes.take(3).forEach { (name, songs) -> tiles += Pick(name, songs.firstOrNull()) { host.play(songs, 0) } }
        val seen = HashSet<String>()
        val history = Library.history
        history.forEachIndexed { i, song -> if (tiles.size < 8 && seen.add(song.id)) tiles += Pick(song.title, song) { host.play(history, i) } }
        if (tiles.isEmpty()) return
        tiles.chunked(2).forEach { pair ->
            val row = LinearLayout(c).apply { setPadding(c.dp(12), c.dp(4), c.dp(12), c.dp(4)) }
            pair.forEach { tile ->
                row.addView(LinearLayout(c).apply {
                    gravity = Gravity.CENTER_VERTICAL
                    background = roundRect(Ui.SURFACE, c.dp(8))
                    foreground = pressable()
                    addView(cover(tile.song, 48, 8), LinearLayout.LayoutParams(c.dp(52), c.dp(52)))
                    addView(c.label(13f, Color.WHITE, bold = true, lines = 2).apply { text = tile.title; setPadding(c.dp(10), 0, c.dp(8), 0) }, LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f))
                    setOnClickListener { tile.go() }
                }, LinearLayout.LayoutParams(0, c.dp(52), 1f).apply { marginStart = c.dp(4); marginEnd = c.dp(4) })
            }
            if (pair.size == 1) row.addView(View(c), LinearLayout.LayoutParams(0, 1, 1f))
            body.addView(row)
        }
    }

    private fun mixStrip() {
        body.addView(heading("Mixes for you", "Refresh") { loadMixes(force = true) })
        val row = LinearLayout(c).apply { setPadding(c.dp(16), 0, c.dp(4), 0) }
        for ((name, songs) in mixes) {
            val artists = songs.map { it.artist }.filter { it.isNotEmpty() }.distinct().take(3).joinToString(", ")
            val saved = Library.playlists.any { it.name == name }
            val card = FrameLayout(c).apply {
                layoutParams = LinearLayout.LayoutParams(c.dp(150), ViewGroup.LayoutParams.WRAP_CONTENT).apply { marginEnd = c.dp(12) }
                addView(LinearLayout(c).apply {
                    orientation = LinearLayout.VERTICAL
                    addView(cover(songs.firstOrNull(), 150, 10), LinearLayout.LayoutParams(c.dp(150), c.dp(150)))
                    addView(c.label(14f, Color.WHITE, bold = true).apply { text = name; setPadding(0, c.dp(8), 0, 0) })
                    addView(c.label(12f, Ui.MUTED, lines = 2).apply { text = "${songs.size} songs - $artists"; setPadding(0, c.dp(2), 0, 0) })
                    setOnClickListener { host.play(songs, 0) }
                })
                // A visible way to keep the mix as a playlist, like the + on the PC app's cards.
                addView(c.label(18f, Color.WHITE, bold = true).apply {
                    text = if (saved) "✓" else "+"
                    gravity = Gravity.CENTER
                    background = roundRect(0xCC000000.toInt(), c.dp(8))
                    setOnClickListener {
                        if (Library.playlists.none { it.name == name }) {
                            val p = Library.createPlaylist(name)
                            songs.forEach { Library.addTo(p, it) }
                            c.say("Saved \"$name\" to your playlists")
                        }
                    }
                }, LayoutParams(c.dp(34), c.dp(30), Gravity.END or Gravity.TOP).apply { setMargins(0, c.dp(8), c.dp(8), 0) })
            }
            row.addView(card)
        }
        body.addView(HorizontalScrollView(c).apply { isHorizontalScrollBarEnabled = false; addView(row) })
    }

    private fun recent() {
        val history = Library.history.take(12)
        if (history.isNotEmpty()) strip("Recently played", history)
    }

    private fun strip(title: String, songs: List<Song>) {
        body.addView(heading(title))
        val row = LinearLayout(c).apply { setPadding(c.dp(16), 0, c.dp(4), 0) }
        songs.forEachIndexed { index, song ->
            row.addView(LinearLayout(c).apply {
                orientation = LinearLayout.VERTICAL
                layoutParams = LinearLayout.LayoutParams(c.dp(132), ViewGroup.LayoutParams.WRAP_CONTENT).apply { marginEnd = c.dp(12) }
                addView(cover(song, 120), LinearLayout.LayoutParams(c.dp(132), c.dp(132)))
                addView(c.label(13f, Color.WHITE, lines = 2).apply { text = song.title; setPadding(0, c.dp(6), 0, 0) })
                addView(c.label(12f, Ui.MUTED).apply { text = song.artist })
                setOnClickListener { host.play(songs, index) }
            })
        }
        body.addView(HorizontalScrollView(c).apply { isHorizontalScrollBarEnabled = false; addView(row) })
    }

    private fun feedMenu(anchor: View) {
        val menu = SheetMenu(c, anchor)
        menu.menu.add(0, 1, 0, "YouTube Music home")
        menu.menu.add(0, 2, 1, "Charts")
        menu.menu.add(0, 3, 2, "Radio from favorites")
        menu.menu.add(0, 4, 3, "Radio from listening history")
        menu.setOnMenuItemClickListener {
            when (it.itemId) {
                1 -> { feedTitle.text = "From YouTube Music"; load(force = true) }
                2 -> feed("Charts") { Core.charts() }
                3 -> radioFrom(Library.favorites.firstOrNull(), "Like a song first, then this makes a radio from your favorites.")
                4 -> radioFrom(Library.history.firstOrNull(), "Play something first, then this makes a radio from your history.")
            }
            true
        }
        menu.show()
    }

    private fun radioFrom(seed: Song?, none: String) {
        if (seed == null) {
            c.say(none)
            return
        }
        c.say("Starting a radio from ${seed.title}...")
        Thread {
            val songs = listOf(seed) + suggestionsOf(JSONObject(Core.radio(seed.id, 40))).filter { it.id != seed.id }
            post { if (songs.size > 1) host.play(songs, 0) else c.say("Could not make a radio right now") }
        }.start()
    }

    private fun feed(title: String, fetch: () -> String) {
        feedTitle.text = title
        status.text = "Loading..."
        status.visibility = VISIBLE
        Thread {
            val reply = JSONObject(fetch())
            val songs = songsOf(reply)
            post {
                adapter.songs = songs
                if (songs.isEmpty()) status.text = reply.optString("error", "Nothing to show yet") else status.visibility = GONE
            }
        }.start()
    }

    /** Mixes made from the library; they need the network and a little time, so they appear when ready. */
    fun loadMixes(force: Boolean = false) {
        if (mixing || (mixes.isNotEmpty() && !force)) return
        if (Library.favorites.isEmpty() && Library.events.isEmpty()) return
        mixing = true
        val library = Sync.snapshotNow().toString()
        Thread {
            val reply = runCatching { JSONObject(Core.mixes(library)) }.getOrNull()
            val made = reply?.optJSONArray("mixes")?.let { array ->
                (0 until array.length()).mapNotNull { i ->
                    val m = array.getJSONObject(i)
                    val songs = songsOf(m)
                    if (songs.size >= 4) m.optString("name") to songs else null
                }
            }.orEmpty()
            val recommended = reply?.let { songsOf(JSONObject().put("tracks", it.optJSONArray("picks") ?: org.json.JSONArray())) }.orEmpty()
            post {
                mixing = false
                if (made.isNotEmpty()) {
                    mixes = made
                    picks = recommended
                    draw()
                }
            }
        }.start()
    }

    /** Fetches the feed once; tapping the message tries again after a failure. */
    fun load(force: Boolean = false) {
        loadMixes()
        if (chip == "Friends") loadFriendsNow()
        if (loaded && !force) return
        status.text = "Loading..."
        status.visibility = VISIBLE
        Thread {
            val reply = JSONObject(Core.home())
            val songs = songsOf(reply)
            post {
                if (songs.isEmpty() && !retried) {
                    // The connection is often not ready in the first second after the app opens: try once more.
                    retried = true
                    postDelayed({ load(force = true) }, 1500)
                } else if (songs.isEmpty()) {
                    status.text = reply.optString("error", "Nothing to show yet") + "\nTap to try again"
                } else {
                    loaded = true
                    adapter.songs = songs
                    status.visibility = GONE
                }
            }
        }.start()
    }
}

/** Search like Spotify: a white bar, then "Browse all" as colourful cards; a search shows songs, artists, albums and more. */
class SearchPage(context: Context, host: SongHost) : Page(context, host) {
    private val field = EditText(context)
    private val status = c.status()
    private val browse = ScrollView(context).apply { isVerticalScrollBarEnabled = false }
    private val results = ListView(context).apply { divider = null; visibility = GONE }
    private val clear = c.icon(R.drawable.ic_close, Color.BLACK, 36, 9).apply { visibility = GONE }
    private val chips = LinearLayout(context)
    private val chipRow = HorizontalScrollView(context).apply { isHorizontalScrollBarEnabled = false; visibility = GONE }
    private var kind = "All"
    private var source = 0
    private val sources = listOf("All sources", "YouTube Music", "Spotify", "SoundCloud", "YouTube")
    private var current = ""
    private var token = 0
    private val resultsAdapter = ResultsAdapter()
    private val loading = c.skeleton(8).apply { visibility = GONE }

    private val categories = listOf(
        "Pop" to 0xFFDC148C, "Hip-Hop" to 0xFFBC5900, "Rock" to 0xFFE91429, "Chill" to 0xFF1E3264,
        "Workout" to 0xFF777777, "Party" to 0xFF8D67AB, "Focus" to 0xFF503750, "Sleep" to 0xFF477D95,
        "V-Pop" to 0xFFE8115B, "K-Pop" to 0xFF148A08, "R&B" to 0xFF1E3264, "Indie" to 0xFFE13300,
        "Electronic" to 0xFF0D73EC, "Latin" to 0xFFBA5D07, "Jazz" to 0xFF477D95, "Classical" to 0xFF7D4B32,
        "Romance" to 0xFFAF2896, "Gaming" to 0xFF5179A1, "Acoustic" to 0xFF006450, "Throwback" to 0xFF8C1932,
        "Podcasts" to 0xFF006450,
    )

    init {
        field.apply {
            hint = "What do you want to play?"
            setHintTextColor(0xFF535353.toInt())
            setTextColor(Color.BLACK)
            setSingleLine()
            background = null
            inputType = InputType.TYPE_CLASS_TEXT
            imeOptions = EditorInfo.IME_ACTION_SEARCH
            setPadding(0, 0, 0, 0)
            setTypeface(typeface, android.graphics.Typeface.BOLD)
            setOnEditorActionListener { view, _, _ ->
                (c.getSystemService(Context.INPUT_METHOD_SERVICE) as InputMethodManager).hideSoftInputFromWindow(view.windowToken, 0)
                kind = "All"
                search(text.toString())
                true
            }
            addTextChangedListener(object : android.text.TextWatcher {
                override fun afterTextChanged(s: android.text.Editable?) {
                    clear.visibility = if (s.isNullOrEmpty()) GONE else VISIBLE
                    if (s.isNullOrEmpty()) showBrowse()
                }

                override fun beforeTextChanged(s: CharSequence?, start: Int, count: Int, after: Int) = Unit
                override fun onTextChanged(s: CharSequence?, start: Int, before: Int, count: Int) = Unit
            })
        }
        clear.setOnClickListener { field.setText("") }
        val bar = LinearLayout(c).apply {
            gravity = Gravity.CENTER_VERTICAL
            background = roundRect(Color.WHITE, c.dp(8))
            setPadding(c.dp(12), 0, c.dp(4), 0)
            addView(c.icon(R.drawable.ic_search_thin, Color.BLACK, 28, 2).apply { background = null; isClickable = false })
            addView(field, LinearLayout.LayoutParams(0, c.dp(46), 1f).apply { marginStart = c.dp(10) })
            addView(clear)
        }
        val head = LinearLayout(c).apply {
            gravity = Gravity.CENTER_VERTICAL
            setPadding(c.dp(16), c.dp(18), c.dp(16), c.dp(12))
            addView(c.label(26f, Color.WHITE, bold = true).apply { text = "Search" }, LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f))
        }
        val grid = LinearLayout(c).apply { orientation = LinearLayout.VERTICAL; setPadding(c.dp(12), 0, c.dp(12), c.dp(24)) }
        grid.addView(c.label(16f, Color.WHITE, bold = true).apply { text = "Browse all"; setPadding(c.dp(4), c.dp(18), 0, c.dp(12)) })
        categories.chunked(2).forEach { pair ->
            val row = LinearLayout(c)
            pair.forEach { (name, color) ->
                row.addView(category(name, color.toInt()), LinearLayout.LayoutParams(0, c.dp(100), 1f).apply { setMargins(c.dp(4), c.dp(4), c.dp(4), c.dp(4)) })
            }
            if (pair.size == 1) row.addView(View(c), LinearLayout.LayoutParams(0, 1, 1f))
            grid.addView(row)
        }
        browse.addView(grid)
        chipRow.addView(chips.apply { setPadding(c.dp(12), c.dp(4), c.dp(12), c.dp(8)) })
        results.adapter = resultsAdapter
        adapter.showSource = true
        val column = LinearLayout(c).apply {
            orientation = LinearLayout.VERTICAL
            addView(head)
            addView(bar, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT).apply { setMargins(c.dp(16), 0, c.dp(16), c.dp(4)) })
            addView(chipRow)
            addView(status)
            addView(loading)
            addView(browse, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, 0, 1f))
            addView(results, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, 0, 1f))
        }
        addView(column, LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT))
        status.visibility = GONE
    }

    /** A card with the genre's name and a tilted block in the corner, the way Spotify draws them. */
    private fun category(name: String, color: Int) = FrameLayout(c).apply {
        background = roundRect(color, c.dp(8))
        clipRounded(c.dp(8))
        addView(View(c).apply {
            background = roundRect(0x33000000, c.dp(6))
            rotation = 25f
        }, LayoutParams(c.dp(64), c.dp(64), Gravity.BOTTOM or Gravity.END).apply { setMargins(0, 0, -c.dp(10), -c.dp(6)) })
        addView(c.label(17f, Color.WHITE, bold = true, lines = 2).apply { text = name; setPadding(c.dp(14), c.dp(12), c.dp(14), 0) })
        setOnClickListener {
            kind = if (name == "Podcasts") "Podcasts" else "All"
            search(if (name == "Podcasts") "podcast" else "$name music")
        }
    }

    private fun showBrowse() {
        token++
        loading.visibility = GONE
        status.visibility = GONE
        results.visibility = GONE
        chipRow.visibility = GONE
        browse.visibility = VISIBLE
    }

    private fun drawChips() {
        chips.removeAllViews()
        // Where to look, as on the PC app: everything at once, or one place.
        chips.addView(c.label(14f, Color.WHITE, bold = true).apply {
            text = sources[source] + "  \u25BE"
            background = android.graphics.drawable.GradientDrawable().apply { cornerRadius = c.dp(18).toFloat(); setStroke(c.dp(1), 0x99FFFFFF.toInt()); setColor(Color.TRANSPARENT) }
            setPadding(c.dp(16), c.dp(8), c.dp(16), c.dp(8))
            setOnClickListener {
                val menu = SheetMenu(c, this)
                sources.forEachIndexed { i, name -> menu.menu.add(0, i, i, name) }
                menu.setOnMenuItemClickListener { item -> source = item.itemId; search(current, keepField = true); true }
                menu.show()
            }
        }, LinearLayout.LayoutParams(ViewGroup.LayoutParams.WRAP_CONTENT, ViewGroup.LayoutParams.WRAP_CONTENT).apply { marginEnd = c.dp(8) })
        for (name in listOf("All", "Songs", "Artists", "Albums", "Playlists", "Podcasts")) {
            chips.addView(c.label(14f, if (kind == name) Color.BLACK else Color.WHITE, bold = true).apply {
                text = name
                background = roundRect(if (kind == name) Ui.GREEN else Ui.SURFACE, c.dp(18))
                setPadding(c.dp(16), c.dp(8), c.dp(16), c.dp(8))
                setOnClickListener { kind = name; search(current, keepField = true) }
            }, LinearLayout.LayoutParams(ViewGroup.LayoutParams.WRAP_CONTENT, ViewGroup.LayoutParams.WRAP_CONTENT).apply { marginEnd = c.dp(8) })
        }
    }

    fun focusField() {
        field.requestFocus()
        (c.getSystemService(Context.INPUT_METHOD_SERVICE) as InputMethodManager).showSoftInput(field, 0)
    }

    fun search(query: String, keepField: Boolean = false) {
        if (query.isBlank()) return
        current = query
        if (!keepField && field.text.toString() != query) {
            field.setText(query)
            field.setSelection(field.text.length)
        }
        browse.visibility = GONE
        results.visibility = VISIBLE
        chipRow.visibility = VISIBLE
        drawChips()
        status.visibility = GONE
        loading.visibility = VISIBLE
        resultsAdapter.set(emptyList(), emptyList())
        val mine = ++token
        val want = kind
        Thread {
            val entries = ArrayList<Entry>()
            var error = ""
            val songs = ArrayList<Song>()
            fun items(reply: JSONObject) = reply.optJSONArray("items")?.let { a -> (0 until a.length()).mapNotNull { Item.fromJson(a.getJSONObject(it)) } }.orEmpty()
            try {
                when (want) {
                    "All" -> {
                        // Songs from the chosen sources, and artists, albums and playlists from YouTube Music, all at once.
                        val replies = arrayOfNulls<JSONObject>(3)
                        var found: List<Song> = emptyList()
                        val threads = ArrayList<Thread>()
                        threads += Thread { found = songsFor(query) }
                        if (source == 0 || source == 1) {
                            listOf("artists", "albums", "playlists").forEachIndexed { i, k -> threads += Thread { replies[i] = runCatching { JSONObject(Core.searchKind(query, k)) }.getOrNull() } }
                        }
                        threads.forEach { it.start() }
                        threads.forEach { it.join() }
                        songs += found
                        val artists = replies[0]?.let(::items).orEmpty().filter { it.kind == "artist" }.take(3)
                        val albums = replies[1]?.let(::items).orEmpty().filter { it.kind == "album" }.take(3)
                        val lists = replies[2]?.let(::items).orEmpty().filter { it.kind != "podcast" }.take(3)
                        if (artists.isNotEmpty()) entries += listOf<Entry>(Entry.Title("Artists")) + artists.map { Entry.Thing(it) }
                        if (songs.isNotEmpty()) entries += listOf<Entry>(Entry.Title("Songs")) + songs.take(10).mapIndexed { i, s -> Entry.Track(s, i) }
                        if (albums.isNotEmpty()) entries += listOf<Entry>(Entry.Title("Albums")) + albums.map { Entry.Thing(it) }
                        if (lists.isNotEmpty()) entries += listOf<Entry>(Entry.Title("Playlists")) + lists.map { Entry.Thing(it) }
                        if (songs.size > 10) entries += songs.drop(10).mapIndexed { i, s -> Entry.Track(s, i + 10) }
                    }
                    "Songs" -> {
                        songs += songsFor(query)
                        entries += songs.mapIndexed { i, s -> Entry.Track(s, i) }
                    }
                    "Podcasts" -> {
                        val reply = JSONObject(Core.searchKind(if (query.contains("podcast", true)) query else "$query podcast", "playlists"))
                        error = reply.optString("error")
                        entries += items(reply).filter { it.kind == "podcast" }.map { Entry.Thing(it) }
                    }
                    else -> {
                        val reply = JSONObject(Core.searchKind(query, want.lowercase()))
                        error = reply.optString("error")
                        entries += items(reply).filter { it.kind != "podcast" }.map { Entry.Thing(it) }
                    }
                }
            } catch (e: Exception) {
                error = "Could not search right now"
            }
            post {
                if (token != mine) return@post
                loading.visibility = GONE
                resultsAdapter.set(entries, songs)
                adapter.songs = songs
                if (entries.isEmpty()) {
                    status.text = error.ifEmpty { "Nothing found" }
                } else {
                    status.visibility = GONE
                }
            }
        }.start()
    }

    /** Songs from the chosen source, or from every source at once, taken in turns so no one place crowds the rest. */
    private fun songsFor(query: String): List<Song> {
        val wanted = when (source) {
            0 -> listOf("ytmusic", "spotify", "soundcloud")
            1 -> listOf("ytmusic")
            2 -> listOf("spotify")
            3 -> listOf("soundcloud")
            else -> listOf("youtube")
        }
        val lists = arrayOfNulls<List<Song>>(wanted.size)
        wanted.mapIndexed { i, name ->
            Thread { lists[i] = runCatching { songsOf(JSONObject(Core.searchSource(query, name))) }.getOrDefault(emptyList()) }.also { it.start() }
        }.forEach { it.join() }
        val seen = HashSet<String>()
        val out = ArrayList<Song>()
        val longest = lists.maxOf { it?.size ?: 0 }
        for (rank in 0 until longest) for (list in lists) list?.getOrNull(rank)?.let { if (seen.add(it.id)) out += it }
        return out
    }

    private class ThingRow(val art: ImageView, val name: TextView, val sub: TextView)

    private sealed class Entry {
        class Title(val text: String) : Entry()
        class Track(val song: Song, val index: Int) : Entry()
        class Thing(val item: Item) : Entry()
    }

    /** Section titles, songs and things with their own page (artists, albums...), all in one list. */
    private inner class ResultsAdapter : BaseAdapter() {
        private var entries: List<Entry> = emptyList()
        private var songs: List<Song> = emptyList()

        fun set(list: List<Entry>, all: List<Song>) {
            entries = list
            songs = all
            notifyDataSetChanged()
        }

        override fun getCount() = entries.size
        override fun getItem(position: Int) = entries[position]
        override fun getItemId(position: Int) = position.toLong()
        override fun getViewTypeCount() = 3
        override fun getItemViewType(position: Int) = when (entries[position]) {
            is Entry.Title -> 0
            is Entry.Track -> 1
            is Entry.Thing -> 2
        }

        override fun getView(position: Int, convert: View?, parent: ViewGroup): View = when (val entry = entries[position]) {
            is Entry.Title -> ((convert as? TextView) ?: c.label(20f, Color.WHITE, bold = true).apply { setPadding(c.dp(16), c.dp(18), c.dp(16), c.dp(6)) }).also { it.text = entry.text }
            is Entry.Track -> this@SearchPage.adapter.getView(entry.index, convert, parent)
            is Entry.Thing -> bindThing(convert, entry.item)
        }

        /** A row for an artist, album, playlist or podcast; built once and reused as the list scrolls. */
        private fun bindThing(convert: View?, item: Item): View {
            val view = convert?.takeIf { it.tag is ThingRow } ?: run {
                val art = ImageView(c).apply { scaleType = ImageView.ScaleType.CENTER_CROP }
                val name = c.label(16f, Color.WHITE, bold = true)
                val sub = c.label(13f, Ui.MUTED)
                LinearLayout(c).apply {
                    gravity = Gravity.CENTER_VERTICAL
                    setPadding(c.dp(16), c.dp(8), c.dp(16), c.dp(8))
                    background = pressable()
                    addView(art, LinearLayout.LayoutParams(c.dp(56), c.dp(56)))
                    addView(LinearLayout(c).apply { orientation = LinearLayout.VERTICAL; addView(name); addView(sub) }, LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f).apply { marginStart = c.dp(12) })
                    tag = ThingRow(art, name, sub)
                }
            }
            val row = view.tag as ThingRow
            val round = item.kind == "artist"
            row.art.background = roundRect(Ui.RAISED, if (round) c.dp(28) else c.dp(4))
            row.art.clipRounded(if (round) c.dp(28) else c.dp(4))
            Images.load(row.art, item.cover, c.dp(112))
            val kindName = when (item.kind) { "artist" -> "Artist"; "album" -> "Album"; "podcast" -> "Podcast"; else -> "Playlist" }
            row.name.text = item.title
            row.sub.text = if (item.subtitle.startsWith(kindName)) item.subtitle else "$kindName • ${item.subtitle}"
            view.setOnClickListener { host.open(item) }
            return view
        }
    }
}

/**
 * Your Library, like Spotify's: your picture and "Your Library" with search and +, chips to filter, Liked Songs
 * pinned first, then playlists. A playlist opens its own page with a big cover and a green play button.
 */
class LibraryPage(context: Context, host: SongHost, private val onContext: (Playlist?) -> Unit) : Page(context, host) {
    private enum class Tab { Playlists, Artists, Downloaded, Recents }

    private val chips = LinearLayout(c)
    private val list = ListView(c).apply { divider = null }
    private val grid = android.widget.GridView(c).apply { numColumns = 3; visibility = GONE; horizontalSpacing = c.dp(8); verticalSpacing = c.dp(14); setPadding(c.dp(12), c.dp(4), c.dp(12), c.dp(8)); selector = android.graphics.drawable.ColorDrawable(Color.TRANSPARENT) }
    private var gridMode = false
    private val sortLabel = c.label(13f, Color.WHITE, bold = true)
    private val toggle = c.icon(R.drawable.ic_grid, Color.WHITE, 36, 8)
    private val sortRow = LinearLayout(c).apply {
        gravity = Gravity.CENTER_VERTICAL
        setPadding(c.dp(16), 0, c.dp(8), c.dp(4))
        addView(c.icon(R.drawable.ic_sort, Color.WHITE, 28, 5).apply { isClickable = false; background = null })
        addView(sortLabel.apply { setPadding(c.dp(6), 0, 0, 0) }, LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f))
        addView(toggle)
    }
    private val status = c.status()
    private val filter = EditText(c)
    private val avatarView = ImageView(c)
    private val listPage = LinearLayout(c).apply { orientation = LinearLayout.VERTICAL }
    private val detail = FrameLayout(c).apply { visibility = GONE; setBackgroundColor(Ui.BG); isClickable = true }
    private val detailList = ListView(c).apply { divider = null }
    private val rowsAdapter = RowsAdapter()
    private var tab = Tab.Playlists
    private var open: Playlist? = null
    private var openLiked = false
    private var query = ""

    init {
        avatarView.apply {
            scaleType = ImageView.ScaleType.CENTER_CROP
            background = roundRect(Ui.GREEN, c.dp(16))
            clipRounded(c.dp(16))
            setOnClickListener { host.drawer() }
        }
        val search = c.icon(R.drawable.ic_search_thin, Color.WHITE, 40, 8).apply { setOnClickListener { toggleFilter() } }
        val plus = c.icon(R.drawable.ic_add, Color.WHITE, 40, 8).apply { setOnClickListener { createMenu(it) } }
        val head = LinearLayout(c).apply {
            gravity = Gravity.CENTER_VERTICAL
            setPadding(c.dp(16), c.dp(16), c.dp(8), c.dp(8))
            addView(avatarView, LinearLayout.LayoutParams(c.dp(32), c.dp(32)))
            addView(c.label(22f, Color.WHITE, bold = true).apply { text = "Your Library"; setPadding(c.dp(12), 0, 0, 0) }, LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f))
            addView(search)
            addView(plus)
        }
        filter.apply {
            hint = "Find in Your Library"
            setHintTextColor(Ui.MUTED)
            setTextColor(Color.WHITE)
            setSingleLine()
            visibility = GONE
            background = roundRect(Ui.RAISED, c.dp(6))
            setPadding(c.dp(14), c.dp(10), c.dp(14), c.dp(10))
            addTextChangedListener(object : android.text.TextWatcher {
                override fun afterTextChanged(s: android.text.Editable?) {
                    query = s?.toString().orEmpty().trim().lowercase()
                    show()
                }

                override fun beforeTextChanged(s: CharSequence?, start: Int, count: Int, after: Int) = Unit
                override fun onTextChanged(s: CharSequence?, start: Int, before: Int, count: Int) = Unit
            })
        }
        listPage.apply {
            addView(head)
            addView(filter, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT).apply { setMargins(c.dp(16), 0, c.dp(16), c.dp(8)) })
            addView(HorizontalScrollView(c).apply { isHorizontalScrollBarEnabled = false; addView(chips.apply { setPadding(c.dp(12), c.dp(4), c.dp(12), c.dp(10)) }) })
            addView(sortRow)
            addView(status)
            addView(list, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, 0, 1f))
            addView(grid, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, 0, 1f))
        }
        addView(listPage, LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT))
        detailList.adapter = adapter
        detail.addView(detailList, LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT))
        addView(detail, LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT))
        list.adapter = rowsAdapter
        grid.adapter = rowsAdapter
        toggle.setOnClickListener { gridMode = !gridMode; show() }
        Downloads.onChange = { show() }
        show()
    }

    private fun toggleFilter() {
        val on = filter.visibility != VISIBLE
        filter.visibility = if (on) VISIBLE else GONE
        if (on) {
            filter.requestFocus()
            (c.getSystemService(Context.INPUT_METHOD_SERVICE) as InputMethodManager).showSoftInput(filter, 0)
        } else {
            filter.setText("")
        }
    }

    private fun createMenu(anchor: View) {
        val menu = SheetMenu(c, anchor)
        menu.menu.add(0, 1, 0, "Playlist")
        menu.menu.add(0, 2, 1, "Blend with a friend")
        menu.menu.add(0, 3, 2, "Import playlist")
        menu.setOnMenuItemClickListener {
            when (it.itemId) {
                1 -> newPlaylist()
                2 -> Blend.choose(c as android.app.Activity, host)
                else -> importPlaylist()
            }
            true
        }
        menu.show()
    }

    /** Back inside the library: leaves an open playlist. True when it handled the press. */
    fun back(): Boolean {
        if (detail.visibility != VISIBLE) return false
        closeDetail()
        return true
    }

    override fun libraryChanged() {
        val mine = Account.profile.optString("avatar_url")
        if (Account.signedIn && mine.isNotEmpty() && mine != "null") Images.load(avatarView, mine, c.dp(40)) else avatarView.setImageDrawable(null)
        if (detail.visibility == VISIBLE) drawDetail()
        show()
    }

    private fun chip(label: String, selected: Boolean, action: () -> Unit) = c.label(14f, if (selected) Color.BLACK else Color.WHITE, bold = true).apply {
        text = label
        background = roundRect(if (selected) Ui.GREEN else Ui.SURFACE, c.dp(18))
        setPadding(c.dp(16), c.dp(8), c.dp(16), c.dp(8))
        setOnClickListener { action() }
        layoutParams = LinearLayout.LayoutParams(ViewGroup.LayoutParams.WRAP_CONTENT, ViewGroup.LayoutParams.WRAP_CONTENT).apply { marginEnd = c.dp(8) }
    }

    /** One line in the library list: a cover (or a mosaic), a name and "Playlist - 12 songs". */
    private class Row(val title: String, val subtitle: String, val covers: List<String?>, val round: Boolean = false, val liked: Boolean = false, val pinned: Boolean = false, val playlist: Playlist? = null, val artist: String? = null, val song: Song? = null)

    private fun rows(): List<Row> {
        val out = ArrayList<Row>()
        val q = query
        when (tab) {
            Tab.Playlists -> {
                if (q.isEmpty() || "liked songs".contains(q)) out += Row("Liked Songs", "Playlist • ${Library.favorites.size} songs", emptyList(), liked = true, pinned = true)
                Library.playlists.filter { q.isEmpty() || it.name.lowercase().contains(q) }.forEach {
                    out += Row(it.name, "Playlist • ${it.songs.size} " + if (it.songs.size == 1) "song" else "songs", it.songs.take(4).map(Song::cover), playlist = it)
                }
            }
            Tab.Artists -> {
                val songs = (Library.favorites + Library.history).distinctBy { it.id }
                songs.groupBy { it.artist.ifEmpty { "Unknown artist" } }.entries.sortedByDescending { it.value.size }
                    .filter { q.isEmpty() || it.key.lowercase().contains(q) }
                    .forEach { out += Row(it.key, "Artist • ${it.value.size} " + if (it.value.size == 1) "song" else "songs", listOf(it.value.first().cover), round = true, artist = it.key) }
            }
            Tab.Downloaded -> (listOfNotNull(Downloads.activeSong) + Downloads.songs).filter { q.isEmpty() || it.title.lowercase().contains(q) || it.artist.lowercase().contains(q) }.forEach {
                val state = if (it.id == Downloads.activeId) "Downloading ${Downloads.percent}%" else "Downloaded"
                out += Row(it.title, "$state \u2022 ${it.artist}", listOf(it.cover), song = it)
            }
            Tab.Recents -> Library.history.filter { q.isEmpty() || it.title.lowercase().contains(q) || it.artist.lowercase().contains(q) }.forEach {
                out += Row(it.title, "Song • ${it.artist}", listOf(it.cover))
            }
        }
        return out
    }

    fun showRecents() {
        tab = Tab.Recents
        show()
    }

    private fun show() {
        chips.removeAllViews()
        for (t in Tab.entries) chips.addView(chip(t.name, tab == t) { tab = t; show() })
        val data = rows()
        rowsAdapter.rows = data
        status.text = when (tab) {
            Tab.Playlists -> "Make your first playlist with the + above, or save a mix from Home."
            Tab.Artists -> "The artists of songs you like and play show up here."
            Tab.Downloaded -> if (Downloads.activeId != null) "Downloading for offline..." else "Songs you download play with no connection. Use \u22EF on a song, or the arrow on a playlist."
            Tab.Recents -> "What you play shows up here."
        }
        status.visibility = if (data.isEmpty() || (tab == Tab.Playlists && Library.playlists.isEmpty() && query.isEmpty())) VISIBLE else GONE
        sortLabel.text = if (tab == Tab.Downloaded) {
            (if (Downloads.activeId != null) "Downloading ${Downloads.percent}%" + (if (Downloads.waitingCount > 0) " (${Downloads.waitingCount} waiting)" else "") + " \u2022 " else "") +
                "${Downloads.songs.size} songs \u2022 " + android.text.format.Formatter.formatShortFileSize(c, Downloads.bytes())
        } else {
            "Recents"
        }
        toggle.setImageResource(if (gridMode) R.drawable.ic_list else R.drawable.ic_grid)
        list.visibility = if (gridMode) GONE else VISIBLE
        grid.visibility = if (gridMode) VISIBLE else GONE
    }

    // ---- a playlist's own page ----

    private fun openPlaylist(playlist: Playlist?, liked: Boolean) {
        open = playlist
        openLiked = liked
        onContext(playlist)
        drawDetail()
        detail.alpha = 0f
        detail.visibility = VISIBLE
        detail.animate().alpha(1f).setDuration(180).withLayer().start()
    }

    private fun closeDetail() {
        open = null
        openLiked = false
        onContext(null)
        detail.animate().alpha(0f).setDuration(140).withLayer().withEndAction { detail.visibility = GONE }.start()
    }

    private fun drawDetail() {
        val songs = if (openLiked) Library.favorites.toList() else open?.songs?.toList().orEmpty()
        val name = if (openLiked) "Liked Songs" else open?.name.orEmpty()
        adapter.songs = songs
        val header = LinearLayout(c).apply { orientation = LinearLayout.VERTICAL }
        val cover = cover(if (openLiked) emptyList() else songs.take(4).map(Song::cover), openLiked, 190, 6)
        val top = FrameLayout(c).apply {
            addView(cover, LayoutParams(c.dp(190), c.dp(190), Gravity.CENTER_HORIZONTAL or Gravity.TOP).apply { topMargin = c.dp(56) })
            addView(c.icon(R.drawable.ic_arrow_back, Color.WHITE, 44, 10).apply { setOnClickListener { back() } }, LayoutParams(c.dp(44), c.dp(44), Gravity.TOP or Gravity.START).apply { setMargins(c.dp(8), c.dp(8), 0, 0) })
        }
        header.addView(top, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, c.dp(270)))
        header.addView(c.label(24f, Color.WHITE, bold = true, lines = 2).apply { text = name; setPadding(c.dp(16), c.dp(8), c.dp(16), 0) })
        val owner = LinearLayout(c).apply {
            gravity = Gravity.CENTER_VERTICAL
            setPadding(c.dp(16), c.dp(8), c.dp(16), 0)
            addView(c.label(13f, Color.WHITE, bold = true).apply { text = Account.name.ifEmpty { "You" } })
        }
        header.addView(owner)
        val minutes = songs.sumOf { it.seconds } / 60
        header.addView(c.label(13f, Ui.MUTED).apply { text = "${songs.size} songs" + if (minutes > 0) " • ${minutes / 60}h ${minutes % 60}m".replace("0h ", "") else ""; setPadding(c.dp(16), c.dp(4), c.dp(16), c.dp(4)) })
        val shuffleIcon = c.icon(R.drawable.ic_shuffle, Color.WHITE, 44, 10)
        val playButton = ImageView(c).apply {
            setImageResource(R.drawable.ic_play)
            setColorFilter(Color.BLACK)
            setPadding(c.dp(14), c.dp(14), c.dp(14), c.dp(14))
            background = GradientDrawable().apply { shape = GradientDrawable.OVAL; setColor(Ui.GREEN) }
            isClickable = true
        }
        shuffleIcon.setOnClickListener {
            if (songs.isNotEmpty()) {
                Playback.nextSource = name
                host.play(songs.shuffled(), 0)
            }
        }
        playButton.setOnClickListener {
            if (songs.isNotEmpty()) {
                Playback.nextSource = name
                host.play(songs, 0)
            }
        }
        val actions = LinearLayout(c).apply {
            gravity = Gravity.CENTER_VERTICAL
            setPadding(c.dp(8), c.dp(8), c.dp(16), c.dp(8))
            addView(c.icon(R.drawable.ic_more, 0xCCFFFFFF.toInt(), 44, 10).apply { setOnClickListener { playlistMenu(it, name) } })
            addView(View(c), LinearLayout.LayoutParams(0, 1, 1f))
            addView(shuffleIcon)
            addView(playButton, LinearLayout.LayoutParams(c.dp(56), c.dp(56)))
        }
        header.addView(actions)
        fun paint(tint: Int) {
            header.background = GradientDrawable(GradientDrawable.Orientation.TOP_BOTTOM, intArrayOf(tint, Ui.BG))
        }
        paint(if (openLiked) 0xFF3B2E8A.toInt() else 0xFF2A2A2A.toInt())
        // The page takes the cover's colour at the top and fades to the app's black.
        if (!openLiked && songs.isNotEmpty()) Images.load(ImageView(c), songs.first().cover, c.dp(40)) { bitmap -> paint(deepTone(Images.average(bitmap))) }
        val old = detailHeader
        if (old != null) detailList.removeHeaderView(old)
        detailHeader = header
        detailList.addHeaderView(header, null, false)
        detailList.adapter = adapter
    }

    private var detailHeader: View? = null

    private fun deepTone(color: Int): Int {
        val hsv = FloatArray(3)
        Color.colorToHSV(color, hsv)
        hsv[1] = (hsv[1] * 1.1f).coerceAtMost(1f)
        hsv[2] = (hsv[2] * 0.55f).coerceIn(0.18f, 0.5f)
        return Color.HSVToColor(hsv)
    }

    private fun playlistMenu(anchor: View, name: String) {
        val menu = SheetMenu(c, anchor)
        val playlist = open
        if (playlist != null) {
            menu.menu.add(0, 1, 0, "Rename")
            menu.menu.add(0, 2, 1, "Delete playlist")
        }
        menu.menu.add(0, 3, 2, "Share")
        menu.setOnMenuItemClickListener {
            when (it.itemId) {
                1 -> rename(playlist!!)
                2 -> { Library.delete(playlist!!); closeDetail() }
                3 -> {
                    val songs = adapter.songs
                    val text = "$name\n" + songs.take(30).joinToString("\n") { s -> "${s.title} - ${s.artist}" }
                    val send = android.content.Intent(android.content.Intent.ACTION_SEND).setType("text/plain").putExtra(android.content.Intent.EXTRA_TEXT, text)
                    c.startActivity(android.content.Intent.createChooser(send, "Share").addFlags(android.content.Intent.FLAG_ACTIVITY_NEW_TASK))
                }
            }
            true
        }
        menu.show()
    }

    private fun rename(playlist: Playlist) {
        val input = (c as android.app.Activity).field("Playlist name", playlist.name)
        c.ask("Rename playlist", content = input, ok = "Save") { Library.rename(playlist, input.text.toString()); drawDetail() }
    }

    // ---- creating ----

    private fun newPlaylist() {
        val input = (c as android.app.Activity).field("Playlist name")
        c.ask("Give your playlist a name", content = input, ok = "Create") { Library.createPlaylist(input.text.toString()) }
    }

    /** Brings a playlist in from a link: the songs land in a new playlist of your own. */
    private fun importPlaylist() {
        val activity = c as android.app.Activity
        val clip = (c.getSystemService(android.content.Context.CLIPBOARD_SERVICE) as android.content.ClipboardManager).primaryClip
            ?.takeIf { it.itemCount > 0 }?.getItemAt(0)?.text?.toString().orEmpty().trim().takeIf { it.startsWith("http") }.orEmpty()
        val field = activity.field("Playlist link", clip)
        activity.ask("Import a playlist", "Paste a link from Spotify, YouTube, YouTube Music, Deezer or Apple Music.", content = field, ok = "Import") {
            val link = field.text.toString().trim()
            if (link.isEmpty()) return@ask
            c.say("Importing... this can take a moment")
            Thread {
                val reply = runCatching { JSONObject(Core.importPlaylist(link)) }.getOrDefault(JSONObject().put("error", "Could not import that link"))
                val songs = songsOf(reply)
                post {
                    when {
                        reply.has("error") -> c.say(reply.optString("error"))
                        songs.isEmpty() -> c.say("No songs could be found in that playlist")
                        else -> {
                            val list = Library.createPlaylist(reply.optString("name").ifBlank { "Imported playlist" })
                            songs.forEach { Library.addTo(list, it) }
                            c.say("Imported ${songs.size} songs into \"${list.name}\"")
                        }
                    }
                }
            }.start()
        }
    }

    /** A square cover: a mosaic of four, one picture, or Spotify's purple heart for Liked Songs. */
    private fun cover(urls: List<String?>, liked: Boolean, size: Int, radius: Int): View {
        val box = FrameLayout(c).apply {
            background = roundRect(Ui.RAISED, c.dp(radius))
            clipRounded(c.dp(radius))
        }
        when {
            liked -> {
                box.background = GradientDrawable(GradientDrawable.Orientation.TL_BR, intArrayOf(0xFF450AF5.toInt(), 0xFF8E8EE5.toInt())).apply { cornerRadius = c.dp(radius).toFloat() }
                box.addView(ImageView(c).apply { setImageResource(R.drawable.ic_heart); setColorFilter(Color.WHITE) }, FrameLayout.LayoutParams(c.dp(size / 2), c.dp(size / 2), Gravity.CENTER))
            }
            urls.size >= 4 -> {
                val half = c.dp(size) / 2
                for (i in 0 until 4) {
                    val tile = ImageView(c).apply { scaleType = ImageView.ScaleType.CENTER_CROP }
                    Images.load(tile, urls[i], c.dp(size / 2))
                    box.addView(tile, FrameLayout.LayoutParams(half, half).apply { leftMargin = (i % 2) * half; topMargin = (i / 2) * half })
                }
            }
            else -> {
                val tile = ImageView(c).apply { scaleType = ImageView.ScaleType.CENTER_CROP }
                Images.load(tile, urls.firstOrNull(), c.dp(size))
                box.addView(tile, FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT))
            }
        }
        return box
    }

    private inner class RowsAdapter : BaseAdapter() {
        var rows: List<Row> = emptyList()
            set(value) {
                field = value
                notifyDataSetChanged()
            }

        override fun getCount() = rows.size
        override fun getItem(position: Int) = rows[position]
        override fun getItemId(position: Int) = position.toLong()
        override fun getView(position: Int, convert: View?, parent: ViewGroup): View {
            val row = rows[position]
            if (gridMode) return tile(row)
            val art = cover(row.covers, row.liked, 56, if (row.round) 28 else 4)
            val item = LinearLayout(c).apply {
                gravity = Gravity.CENTER_VERTICAL
                setPadding(c.dp(16), c.dp(8), c.dp(16), c.dp(8))
                background = pressable()
                addView(art, LinearLayout.LayoutParams(c.dp(56), c.dp(56)))
                addView(
                    LinearLayout(c).apply {
                        orientation = LinearLayout.VERTICAL
                        addView(c.label(16f, if (row.liked) Color.WHITE else Color.WHITE, bold = false).apply { text = row.title })
                        addView(LinearLayout(c).apply {
                            gravity = Gravity.CENTER_VERTICAL
                            setPadding(0, c.dp(2), 0, 0)
                            if (row.pinned) addView(ImageView(c).apply { setImageResource(R.drawable.ic_pin); setColorFilter(Ui.GREEN) }, LinearLayout.LayoutParams(c.dp(14), c.dp(14)).apply { marginEnd = c.dp(6) })
                            addView(c.label(13f, Ui.MUTED).apply { text = row.subtitle })
                        })
                    },
                    LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f).apply { marginStart = c.dp(12) },
                )
                setOnClickListener {
                    when {
                        row.liked -> openPlaylist(null, true)
                        row.playlist != null -> openPlaylist(row.playlist, false)
                        row.artist != null -> host.openArtist(row.artist)
                        row.song != null -> {
                            Playback.nextSource = "Downloaded"
                            host.play(Downloads.songs.toList(), Downloads.songs.indexOfFirst { it.id == row.song.id }.coerceAtLeast(0))
                        }
                        else -> host.search(row.title)
                    }
                }
                if (row.song != null) setOnLongClickListener { host.menu(this, row.song); true }
            }
            return item
        }

        /** The grid layout: a big cover with the name under it. */
        private fun tile(row: Row): View {
            val art = cover(row.covers, row.liked, 110, if (row.round) 55 else 6)
            return LinearLayout(c).apply {
                orientation = LinearLayout.VERTICAL
                addView(art, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, 0).apply { }.also { it.height = ((c.resources.displayMetrics.widthPixels - c.dp(40)) / 3) })
                addView(c.label(13f, Color.WHITE, bold = true, lines = 2).apply { text = row.title; setPadding(0, c.dp(6), 0, 0); if (row.round) gravity = Gravity.CENTER_HORIZONTAL })
                addView(c.label(12f, Ui.MUTED).apply { text = row.subtitle.substringBefore(" \u2022 "); if (row.round) gravity = Gravity.CENTER_HORIZONTAL })
                setOnClickListener {
                    when {
                        row.liked -> openPlaylist(null, true)
                        row.playlist != null -> openPlaylist(row.playlist, false)
                        row.artist != null -> host.openArtist(row.artist)
                        row.song != null -> { Playback.nextSource = "Downloaded"; host.play(Downloads.songs.toList(), Downloads.songs.indexOfFirst { it.id == row.song.id }.coerceAtLeast(0)) }
                        else -> host.search(row.title)
                    }
                }
            }
        }
    }
}
