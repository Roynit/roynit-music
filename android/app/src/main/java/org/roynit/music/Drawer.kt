package org.roynit.music

import android.app.Activity
import android.graphics.Color
import android.view.Gravity
import android.view.View
import android.view.ViewGroup
import android.widget.FrameLayout
import android.widget.ImageView
import android.widget.LinearLayout
import android.widget.Toast

/**
 * The menu that slides in from the left when you tap your picture, like Spotify's: your profile, listening stats,
 * what you played lately, updates, and settings. Moving it is a translation only, so it costs the CPU nothing.
 */
class DrawerView(private val activity: Activity, private val go: (Int) -> Unit, private val recents: () -> Unit) : FrameLayout(activity) {
    private val scrim = View(activity).apply { setBackgroundColor(0xAA000000.toInt()); alpha = 0f; setOnClickListener { close() } }
    private val panel = LinearLayout(activity).apply {
        orientation = LinearLayout.VERTICAL
        setBackgroundColor(Ui.SURFACE)
        isClickable = true
        setPadding(activity.dp(12), activity.dp(16), activity.dp(12), activity.dp(16))
    }
    private val width = (activity.resources.displayMetrics.widthPixels * 0.85f).toInt()
    val isOpen get() = visibility == View.VISIBLE

    init {
        visibility = View.GONE
        addView(scrim, LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT))
        addView(panel, LayoutParams(width, ViewGroup.LayoutParams.MATCH_PARENT))
    }

    private fun row(icon: Int, text: String, run: () -> Unit) = LinearLayout(activity).apply {
        gravity = Gravity.CENTER_VERTICAL
        setPadding(activity.dp(8), activity.dp(16), activity.dp(8), activity.dp(16))
        background = pressable()
        addView(ImageView(activity).apply { setImageResource(icon); setColorFilter(Color.WHITE) }, LinearLayout.LayoutParams(activity.dp(26), activity.dp(26)))
        addView(activity.label(17f, Color.WHITE).apply { this.text = text; setPadding(activity.dp(18), 0, 0, 0) })
        setOnClickListener { close(); run() }
    }

    private fun build() {
        panel.removeAllViews()
        val avatar = ImageView(activity).apply {
            scaleType = ImageView.ScaleType.CENTER_CROP
            background = roundRect(Ui.GREEN, activity.dp(28))
            clipRounded(activity.dp(28))
        }
        val signed = Account.signedIn
        val mine = Account.profile.optString("avatar_url")
        if (signed && mine.isNotEmpty() && mine != "null") Images.load(avatar, mine, activity.dp(56))
        val name = if (signed) Account.name.ifEmpty { "You" } else "Not signed in"
        panel.addView(LinearLayout(activity).apply {
            gravity = Gravity.CENTER_VERTICAL
            background = roundRect(Ui.RAISED, activity.dp(12))
            setPadding(activity.dp(12), activity.dp(12), activity.dp(12), activity.dp(12))
            addView(avatar, LinearLayout.LayoutParams(activity.dp(56), activity.dp(56)))
            addView(LinearLayout(activity).apply {
                orientation = LinearLayout.VERTICAL
                addView(activity.label(19f, Color.WHITE, bold = true).apply { text = name })
                addView(activity.label(14f, Ui.MUTED).apply { text = if (signed) "View profile" else "Sign in" })
            }, LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f).apply { marginStart = activity.dp(14) })
            setOnClickListener { close(); go(3) }
        }, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT).apply { bottomMargin = activity.dp(8) })
        panel.addView(row(R.drawable.ic_add, "Add account") { go(3) })
        panel.addView(row(R.drawable.ic_stats, "Listening stats") { stats() })
        panel.addView(row(R.drawable.ic_recent, "Recents") { recents() })
        panel.addView(row(R.drawable.ic_megaphone, "What's new") { updates() })
        panel.addView(row(R.drawable.ic_settings, "Settings and privacy") { go(3) })
    }

    fun open() {
        build()
        visibility = View.VISIBLE
        panel.translationX = -width.toFloat()
        panel.animate().translationX(0f).setDuration(220).withLayer().start()
        scrim.animate().alpha(1f).setDuration(220).withLayer().start()
    }

    fun close() {
        if (!isOpen) return
        scrim.animate().alpha(0f).setDuration(160).withLayer().start()
        panel.animate().translationX(-width.toFloat()).setDuration(180).withLayer().withEndAction { visibility = View.GONE }.start()
    }

    /** Plays, hours and favourites worked out from the plays this phone knows (the same list the PC app syncs). */
    private fun stats() {
        val events = Library.events
        val seconds = events.sumOf { it.song.seconds }
        val artist = events.groupingBy { it.song.artist.substringBefore(",").trim() }.eachCount().filterKeys { it.isNotEmpty() }.maxByOrNull { it.value }
        val song = events.groupingBy { it.song.id }.eachCount().maxByOrNull { it.value }?.let { top -> events.first { it.song.id == top.key }.song to top.value }
        val rows = ArrayList<Sheet.Option>()
        rows += Sheet.Option("${events.size} plays, about ${seconds / 3600}h ${seconds / 60 % 60}m listened") {}
        rows += Sheet.Option("${Library.favorites.size} liked songs, ${Library.playlists.size} playlists") {}
        if (artist != null) rows += Sheet.Option("Top artist: ${artist.key} (${artist.value} plays)") {}
        if (song != null) rows += Sheet.Option("Top song: ${song.first.title} (${song.second} plays)") {}
        if (events.isEmpty()) rows += Sheet.Option("Play a few songs and your stats appear here.") {}
        Sheet.show(activity, "Listening stats", "From what you have played", null, rows)
    }

    private fun updates() {
        activity.say("Checking for updates...")
        Thread {
            val current = activity.packageManager.getPackageInfo(activity.packageName, 0).versionName.orEmpty()
            val release = Updater.check(current)
            activity.runOnUiThread {
                if (release == null) {
                    activity.say("You have the latest version ($current).")
                } else {
                    Sheet.show(activity, "Update ${release.version} is out", "You have $current", null, listOf(Sheet.Option("Download and install", R.drawable.ic_download) {
                        activity.say("Downloading...")
                        Thread {
                            val problem = Updater.install(activity, release)
                            if (problem != null) activity.runOnUiThread { activity.say(problem) }
                        }.start()
                    }))
                }
            }
        }.start()
    }
}
