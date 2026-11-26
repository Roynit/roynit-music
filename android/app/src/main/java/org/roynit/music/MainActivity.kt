package org.roynit.music

import android.Manifest
import android.app.Activity
import android.app.AlertDialog
import android.content.ComponentName
import android.graphics.Color
import android.os.Build
import android.content.Intent
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.view.Gravity
import android.view.View
import android.view.ViewGroup
import android.view.WindowInsets
import android.widget.FrameLayout
import android.widget.ImageView
import android.widget.LinearLayout
import android.widget.PopupMenu
import android.widget.TextView
import androidx.media3.common.C
import androidx.media3.common.Player
import androidx.media3.session.MediaController
import androidx.media3.session.SessionToken
import org.json.JSONObject

/**
 * The app's one screen: Home, Search and Library pages, a mini player, and the full player that slides up
 * over them. Closing the app does not stop the music; that lives in PlaybackService.
 */
class MainActivity : Activity(), SongHost {
    private lateinit var shell: LinearLayout
    private lateinit var frame: FrameLayout
    private lateinit var home: HomePage
    private lateinit var search: SearchPage
    private lateinit var library: LibraryPage
    private lateinit var friends: FriendsPage
    private lateinit var chat: ChatView
    private lateinit var pages: List<Page>
    private lateinit var nowPlaying: NowPlayingView
    private lateinit var cropper: CropView
    private lateinit var drawer: DrawerView
    private lateinit var detail: DetailView
    private var cropDone: ((android.graphics.Bitmap?) -> Unit)? = null
    private lateinit var mini: LinearLayout
    private lateinit var miniCover: ImageView
    private lateinit var miniTitle: TextView
    private lateinit var miniArtist: TextView
    private lateinit var miniPlay: ImageView
    private lateinit var miniHeart: ImageView
    private lateinit var miniProgress: View
    private val navItems = ArrayList<Pair<ImageView, TextView>>()
    private var selected = 0
    private var contextPlaylist: Playlist? = null
    private val ticker = Handler(Looper.getMainLooper())
    private val tick = object : Runnable {
        override fun run() {
            progress()
            ticker.postDelayed(this, 500)
        }
    }

    override fun onResume() {
        super.onResume()
        Sync.resume()
        Sync.watching(true)
        Notifier.foreground = true
        window.decorView.postDelayed({ Notifier.wishInApp(this) }, 1500)
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        setIntent(intent)
        openTabFromIntent()
    }

    private fun openTabFromIntent() {
        val tab = intent.getIntExtra("tab", -1)
        if (tab in 1..3) select(tab, false)
        intent.removeExtra("tab")
    }

    override fun onPause() {
        super.onPause()
        Sync.watching(false)
        Notifier.foreground = false
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        Images.init(this)
        Library.load(this)
        Account.load(this)
        Sync.load(this)
        Downloads.load(this)
        Sync.now()
        Notifier.start(this)
        if (Build.VERSION.SDK_INT >= 33) requestPermissions(arrayOf(Manifest.permission.POST_NOTIFICATIONS), 1)
        setContentView(build())
        openTabFromIntent()
        Library.onChange = {
            pages.forEach(Page::libraryChanged)
            nowPlaying.refresh()
            refresh()
        }
        val token = SessionToken(this, ComponentName(this, PlaybackService::class.java))
        val future = MediaController.Builder(this, token).buildAsync()
        future.addListener({
            val controller = future.get()
            Playback.controller = controller
            controller.addListener(object : Player.Listener {
                override fun onEvents(player: Player, events: Player.Events) {
                    refresh()
                    nowPlaying.refresh()
                }
            })
            refresh()
        }, mainExecutor)
        home.load()
    }

    override fun onStart() {
        super.onStart()
        ticker.post(tick)
    }

    override fun onStop() {
        ticker.removeCallbacks(tick)
        super.onStop()
    }

    override fun onDestroy() {
        Library.onChange = null
        Playback.controller?.release()
        Playback.controller = null
        super.onDestroy()
    }

    @Deprecated("Back is handled by hand so the full player and playlists close first")
    override fun onBackPressed() {
        when {
            chat.isOpen -> chat.back()
            drawer.isOpen -> drawer.close()
            cropper.visibility == View.VISIBLE -> { cropper.visibility = View.GONE; cropDone?.invoke(null); cropDone = null }
            nowPlaying.isOpen -> nowPlaying.close()
            detail.isOpen -> detail.back()
            selected == 2 && library.back() -> Unit
            selected == 3 && friends.back() -> Unit
            else -> @Suppress("DEPRECATION") super.onBackPressed()
        }
    }

    // ---- screen ----

    private fun build(): View {
        val root = FrameLayout(this).apply { setBackgroundColor(Ui.BG) }
        home = HomePage(this, this)
        search = SearchPage(this, this)
        library = LibraryPage(this, this) { contextPlaylist = it }
        friends = FriendsPage(this, this)
        pages = listOf(home, search, library, friends)
        frame = FrameLayout(this)
        pages.forEachIndexed { index, page ->
            page.visibility = if (index == 0) View.VISIBLE else View.GONE
            frame.addView(page, FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT))
        }
        mini = buildMini()
        shell = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            addView(updateBanner())
            addView(frame, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, 0, 1f))
            addView(mini, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT).apply { setMargins(dp(8), 0, dp(8), dp(4)) })
            addView(buildNav())
        }
        nowPlaying = NowPlayingView(this, this) { }
        root.addView(shell, FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT))
        root.addView(nowPlaying, FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT))
        detail = DetailView(this, this)
        // Inside the page area, so the player bar and the tabs stay in reach while a page is open.
        frame.addView(detail, FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT))
        drawer = DrawerView(this, { select(it) }, { select(2); library.showRecents() }, { select(3); friends.openShop() })
        root.addView(drawer, FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT))
        chat = ChatView(this, { person -> chat.close(); select(3); friends.openPerson(person) }, { })
        root.addView(chat, FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT))
        cropper = CropView(this) { bitmap -> cropDone?.invoke(bitmap); cropDone = null }
        root.addView(cropper, FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT))
        // The system bars sit over the app on recent Android; keep the content clear of them.
        root.setOnApplyWindowInsetsListener { _, insets ->
            val (top, bottom) = if (Build.VERSION.SDK_INT >= 30) {
                val bars = insets.getInsets(WindowInsets.Type.systemBars())
                bars.top to bars.bottom
            } else {
                @Suppress("DEPRECATION")
                insets.systemWindowInsetTop to insets.systemWindowInsetBottom
            }
            shell.setPadding(0, top, 0, bottom)
            nowPlaying.setPadding(0, top, 0, bottom)
            cropper.setPadding(0, top, 0, bottom)
            drawer.setPadding(0, top, 0, bottom)
            // From Android 15 the window no longer shrinks for the keyboard; it reports the keyboard's height instead, and the
            // chat lifts its message box by that. On older Android the window shrinks by itself and reports nothing here.
            val keyboard = if (Build.VERSION.SDK_INT >= 30) insets.getInsets(WindowInsets.Type.ime()).bottom else 0
            chat.setPadding(0, top, 0, maxOf(bottom, keyboard))
            insets
        }
        return root
    }

    /** A slim bar that appears when a newer version is published; tapping it downloads, checks and installs. */
    private fun updateBanner(): View {
        val bar = label(13f, Color.BLACK, bold = true).apply {
            gravity = Gravity.CENTER
            background = roundRect(Ui.GREEN, 0)
            setPadding(dp(12), dp(9), dp(12), dp(9))
            visibility = View.GONE
        }
        // Every time the app opens: look for a newer version, fetch and check it quietly, then offer it in one tap.
        Thread {
            val current = packageManager.getPackageInfo(packageName, 0).versionName.orEmpty()
            val release = Updater.check(current) ?: return@Thread
            val file = Updater.download(this, release) ?: return@Thread
            bar.post {
                bar.text = "Update ${release.version} is ready. Tap to install."
                bar.visibility = View.VISIBLE
                bar.setOnClickListener { Updater.launch(this, file) }
                ask("Update ${release.version} is ready", Updater.summary(release.notes), ok = "Update now", cancel = "Later") { Updater.launch(this, file) }
            }
        }.start()
        return bar
    }

    private fun buildNav(): View {
        val bar = LinearLayout(this).apply {
            setBackgroundColor(Ui.BG)
            setPadding(0, dp(8), 0, dp(6))
        }
        listOf(Triple("Home", R.drawable.ic_home_outline, 0), Triple("Search", R.drawable.ic_search_thin, 1), Triple("Your Library", R.drawable.ic_library_outline, 2), Triple("Friends", R.drawable.ic_people_outline, 3)).forEach { (name, res, index) ->
            val image = ImageView(this).apply {
                setImageResource(res)
                setColorFilter(Ui.MUTED)
            }
            val text = label(10f, Ui.MUTED, bold = true)
            val item = LinearLayout(this).apply {
                orientation = LinearLayout.VERTICAL
                gravity = Gravity.CENTER
                addView(image, LinearLayout.LayoutParams(dp(26), dp(26)))
                addView(text.apply { this.text = name; gravity = Gravity.CENTER_HORIZONTAL; setPadding(0, dp(4), 0, 0) })
                setOnClickListener { select(index) }
                setPadding(0, dp(2), 0, dp(2))
            }
            navItems += image to text
            bar.addView(item, LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f))
        }
        select(0, animate = false)
        return bar
    }

    /** Like Spotify: the page you are on is drawn filled and white, the others are thin outlines in grey. */
    private val navActive = intArrayOf(R.drawable.ic_home, R.drawable.ic_search_bold, R.drawable.ic_library, R.drawable.ic_people)
    private val navIdle = intArrayOf(R.drawable.ic_home_outline, R.drawable.ic_search_thin, R.drawable.ic_library_outline, R.drawable.ic_people_outline)

    private fun select(index: Int, animate: Boolean = true) {
        if (index == 1 && selected == 1) search.focusField()
        navItems.forEachIndexed { i, (image, text) ->
            val color = if (i == index) Color.WHITE else Ui.MUTED
            image.setImageResource(if (i == index) navActive[i] else navIdle[i])
            image.setColorFilter(color)
            text.setTextColor(color)
        }
        if (index == selected && animate) return
        val old = pages[selected]
        val new = pages[index]
        selected = index
        if (!animate) {
            pages.forEachIndexed { i, p -> p.visibility = if (i == index) View.VISIBLE else View.GONE }
            return
        }
        // A short cross-fade: alpha only, so it costs the CPU nothing.
        new.alpha = 0f
        new.visibility = View.VISIBLE
        new.animate().alpha(1f).setDuration(160).withLayer().start()
        old.animate().alpha(0f).setDuration(120).withLayer().withEndAction { if (pages[selected] !== old) old.visibility = View.GONE; old.alpha = 1f }.start()
        if (index == 0) home.load()
        friends.visible(index == 3)
    }

    @Deprecated("The picture picker answers here")
    override fun onActivityResult(requestCode: Int, resultCode: Int, data: android.content.Intent?) {
        @Suppress("DEPRECATION") super.onActivityResult(requestCode, resultCode, data)
        if ((requestCode == 11 || requestCode == 12) && resultCode == RESULT_OK) data?.data?.let(friends::picked)
    }

    private lateinit var miniFill: View
    private lateinit var miniTrack: FrameLayout

    /** The player bar above the tabs: a rounded card tinted with the cover's colour, as on Spotify. */
    private fun buildMini(): LinearLayout {
        miniFill = View(this).apply { setBackgroundColor(Color.WHITE) }
        miniProgress = miniFill
        miniTrack = FrameLayout(this).apply {
            background = roundRect(0x55FFFFFF, dp(2))
            addView(miniFill, FrameLayout.LayoutParams(0, ViewGroup.LayoutParams.MATCH_PARENT))
            clipRounded(dp(2))
        }
        miniCover = ImageView(this).apply {
            scaleType = ImageView.ScaleType.CENTER_CROP
            background = roundRect(Ui.RAISED, dp(4))
            clipRounded(dp(4))
        }
        miniTitle = label(14f, Color.WHITE, bold = true)
        miniArtist = label(13f, 0xCCFFFFFF.toInt()).apply { setPadding(0, dp(1), 0, 0) }
        miniHeart = icon(R.drawable.ic_heart_outline, Color.WHITE, 40, 9).apply { setOnClickListener { Playback.song?.let(Library::toggleFavorite) } }
        miniPlay = icon(R.drawable.ic_play, Color.WHITE, 44, 8).apply { setOnClickListener { Playback.toggle() } }
        val row = LinearLayout(this).apply {
            gravity = Gravity.CENTER_VERTICAL
            setPadding(dp(8), dp(8), dp(4), dp(6))
            addView(miniCover, LinearLayout.LayoutParams(dp(40), dp(40)))
            addView(
                LinearLayout(context).apply {
                    orientation = LinearLayout.VERTICAL
                    setPadding(dp(10), 0, dp(4), 0)
                    addView(miniTitle)
                    addView(miniArtist)
                },
                LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f),
            )
            addView(miniHeart)
            addView(miniPlay)
            setOnClickListener { nowPlaying.open() }
        }
        return LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            visibility = View.GONE
            background = roundRect(0xFF3A3A3A.toInt(), dp(8))
            addView(row)
            addView(miniTrack, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, dp(2)).apply { setMargins(dp(8), 0, dp(8), dp(6)) })
        }
    }

    /** A dark version of the cover's main colour, for the player bar and the full player. */
    private fun deep(color: Int): Int {
        val hsv = FloatArray(3)
        Color.colorToHSV(color, hsv)
        hsv[1] = (hsv[1] * 1.15f).coerceAtMost(1f)
        hsv[2] = (hsv[2] * 0.6f).coerceIn(0.18f, 0.5f)
        return Color.HSVToColor(hsv)
    }

    /** Mini player text, picture and buttons, whenever the player changes. */
    private fun refresh() {
        val controller = Playback.controller
        val song = Playback.song
        val id = song?.id
        pages.forEach { it.nowPlaying(id) }
        detail.nowPlaying(id)
        if (controller == null || song == null) {
            mini.visibility = View.GONE
            return
        }
        if (mini.visibility != View.VISIBLE) mini.visibility = View.VISIBLE
        miniTitle.text = song.title
        miniArtist.text = song.artist
        if (miniCover.tag != song.cover) Images.load(miniCover, song.cover, dp(44)) { bitmap -> mini.background = roundRect(deep(Images.average(bitmap)), dp(8)) }
        miniPlay.setImageResource(if (controller.isPlaying || controller.playbackState == Player.STATE_BUFFERING && controller.playWhenReady) R.drawable.ic_pause else R.drawable.ic_play)
        val liked = Library.isFavorite(song.id)
        miniHeart.setImageResource(if (liked) R.drawable.ic_heart else R.drawable.ic_heart_outline)
        miniHeart.setColorFilter(if (liked) Ui.GREEN else Color.WHITE)
        progress()
    }

    private fun progress() {
        val controller = Playback.controller ?: return
        val total = controller.duration.takeIf { it != C.TIME_UNSET && it > 0 } ?: return
        val fraction = (controller.currentPosition.toFloat() / total).coerceIn(0f, 1f)
        val width = (miniTrack.width * fraction).toInt()
        val params = miniFill.layoutParams
        if (params.width != width) {
            params.width = width
            miniFill.layoutParams = params
        }
        if (nowPlaying.isOpen) nowPlaying.progress()
    }

    // ---- what the song rows ask for ----

    override fun crop(uri: android.net.Uri, circle: Boolean, outW: Int, outH: Int, done: (android.graphics.Bitmap?) -> Unit) {
        cropDone = done
        if (!cropper.open(uri, circle, outW, outH)) {
            cropDone = null
            this.say("Could not read that picture")
        }
    }

    override fun drawer() = drawer.open()

    override fun open(item: Item) {
        // The full player sits above the pages, so it steps aside first.
        if (nowPlaying.isOpen) nowPlaying.close()
        detail.open(item)
    }

    override fun openChat(person: org.json.JSONObject) {
        chat.open(person)
    }

    override fun back() {
        detail.back()
    }

    override fun openArtist(name: String) {
        Thread {
            val first = JSONObject(Core.searchKind(name, "artists")).optJSONArray("items")?.let { array ->
                (0 until array.length()).mapNotNull { Item.fromJson(array.getJSONObject(it)) }.firstOrNull { it.kind == "artist" }
            }
            runOnUiThread {
                if (first != null) open(first) else this.say("Could not find that artist")
            }
        }.start()
    }

    override fun search(query: String) {
        select(1)
        search.search(query)
    }

    override fun play(songs: List<Song>, index: Int) {
        Playback.play(songs, index)
        batteryHint()
    }

    /** Some phone makers stop background apps to save battery; ask once to exempt the player. */
    private fun batteryHint() {
        val prefs = getSharedPreferences("roy", MODE_PRIVATE)
        val power = getSystemService(android.os.PowerManager::class.java)
        if (prefs.getBoolean("battery_asked", false) || power.isIgnoringBatteryOptimizations(packageName)) return
        prefs.edit().putBoolean("battery_asked", true).apply()
        ask("Keep music playing", "Some phones stop apps in the background to save battery. Allow Roy Music to run without limits so the music does not cut out when the screen is off.", ok = "Open settings", cancel = "Not now") {
            startActivity(android.content.Intent(android.provider.Settings.ACTION_IGNORE_BATTERY_OPTIMIZATION_SETTINGS))
        }
    }

    override fun menu(anchor: View, song: Song) {
        val liked = Library.isFavorite(song.id)
        val options = ArrayList<Sheet.Option>()
        options += Sheet.Option(if (liked) "Remove from liked songs" else "Add to liked songs", if (liked) R.drawable.ic_heart else R.drawable.ic_heart_outline, if (liked) Ui.GREEN else Color.WHITE) { Library.toggleFavorite(song) }
        val rejected = Library.isDisliked(song.id)
        options += Sheet.Option(if (rejected) "Remove dislike" else "Dislike (do not suggest)", R.drawable.ic_dislike, if (rejected) 0xFFFF6B6B.toInt() else Color.WHITE) {
            say(if (Playback.dislike(song)) "Got it. We will not suggest this song again" else "Dislike removed")
        }
        options += Sheet.Option("Add to playlist", R.drawable.ic_add) { pickPlaylist(song) }
        options += Sheet.Option("Add to queue", R.drawable.ic_queue) { Playback.enqueue(song) }
        options += Sheet.Option("Play next", R.drawable.ic_next) { Playback.playNext(song) }
        options += Sheet.Option("Start radio", R.drawable.ic_shuffle) { radio(song) }
        if (!song.id.startsWith("sc:")) {
            options += Sheet.Option(if (Downloads.has(song.id)) "Remove download" else "Download", R.drawable.ic_download) {
                if (Downloads.has(song.id)) {
                    Downloads.remove(song)
                } else {
                    Downloads.queue(this, listOf(song))
                    this.say("Downloading...")
                }
            }
        }
        options += Sheet.Option("Go to artist", R.drawable.ic_people_outline) { openArtist(song.artist.substringBefore(",")) }
        contextPlaylist?.let { list -> options += Sheet.Option("Remove from this playlist", R.drawable.ic_close, 0xFFFF6B6B.toInt()) { Library.removeFrom(list, song) } }
        Sheet.show(this, song.title, song.artist, song.cover, options)
    }

    private fun radio(song: Song) {
        Thread {
            val songs = suggestionsOf(JSONObject(Core.radio(song.id, 30))).filter { it.id != song.id }
            runOnUiThread { if (songs.isNotEmpty()) Playback.play(listOf(song) + songs, 0) }
        }.start()
    }

    private fun pickPlaylist(song: Song) {
        val options = ArrayList<Sheet.Option>()
        options += Sheet.Option("New playlist", R.drawable.ic_add) {
            val input = field("Playlist name")
            ask("Give your playlist a name", content = input, ok = "Create") { Library.addTo(Library.createPlaylist(input.text.toString()), song) }
        }
        Library.playlists.forEach { list ->
            options += Sheet.Option(list.name + if (list.songs.any { it.id == song.id }) "  (already in)" else "", R.drawable.ic_library) { Library.addTo(list, song) }
        }
        Sheet.show(this, "Add to playlist", song.title, song.cover, options)
    }
}
