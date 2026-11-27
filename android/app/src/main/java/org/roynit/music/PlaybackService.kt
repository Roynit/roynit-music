package org.roynit.music

import android.content.Intent
import android.net.Uri
import android.os.Handler
import android.os.Looper
import androidx.media3.common.AudioAttributes
import androidx.media3.common.C
import androidx.media3.common.MediaItem
import androidx.media3.common.PlaybackException
import androidx.media3.common.Player
import androidx.media3.datasource.DefaultHttpDataSource
import androidx.media3.datasource.ResolvingDataSource
import androidx.media3.exoplayer.DefaultLoadControl
import androidx.media3.exoplayer.ExoPlayer
import androidx.media3.exoplayer.RenderersFactory
import androidx.media3.exoplayer.audio.MediaCodecAudioRenderer
import androidx.media3.exoplayer.mediacodec.MediaCodecSelector
import androidx.media3.exoplayer.source.DefaultMediaSourceFactory
import androidx.media3.session.MediaSession
import androidx.media3.session.MediaSessionService
import com.google.common.util.concurrent.Futures
import com.google.common.util.concurrent.ListenableFuture
import org.json.JSONObject
import java.io.IOException

/**
 * Plays the music. It is a foreground service with a media notification, so the sound keeps going when the
 * screen turns off, when the app is closed from the recents list, and while other apps are in front.
 */
class PlaybackService : MediaSessionService() {
    private var session: MediaSession? = null
    private var loadingMore = false
    private val main = Handler(Looper.getMainLooper())

    override fun onCreate() {
        super.onCreate()
        Library.load(applicationContext)
        Account.load(applicationContext)
        Sync.load(applicationContext)
        Downloads.load(applicationContext)
        // A track is only turned into an address when the player is about to read it: the queue holds ids.
        val http = DefaultHttpDataSource.Factory()
            .setConnectTimeoutMs(8_000)
            .setReadTimeoutMs(15_000)
            .setUserAgent("RoyMusic")
        // A downloaded song is read from its file; anything else is looked up when the player reaches it.
        val source = ResolvingDataSource.Factory(androidx.media3.datasource.DefaultDataSource.Factory(this, http)) { spec ->
            val wanted = spec.uri.lastPathSegment.orEmpty()
            val saved = if (spec.uri.scheme == "roy") Downloads.file(wanted) else null
            if (saved != null) {
                spec.withUri(Uri.fromFile(saved))
            } else if (spec.uri.scheme == "roy") {
                val reply = JSONObject(
                    Core.resolveAny(
                        wanted,
                        spec.uri.getQueryParameter("t").orEmpty(),
                        spec.uri.getQueryParameter("a").orEmpty(),
                        spec.uri.getQueryParameter("d")?.toDoubleOrNull() ?: 0.0,
                    ),
                )
                val url = reply.optString("url")
                if (url.isEmpty()) throw IOException(reply.optString("error", "Could not open that song"))
                spec.withUri(Uri.parse(url))
            } else {
                spec
            }
        }
        // Audio only, and a small buffer: this has to run well on phones with little memory.
        val renderers = RenderersFactory { handler, _, audioListener, _, _ ->
            arrayOf(MediaCodecAudioRenderer(this, MediaCodecSelector.DEFAULT, handler, audioListener))
        }
        val load = DefaultLoadControl.Builder()
            .setBufferDurationsMs(20_000, 40_000, 1_500, 3_000)
            .setTargetBufferBytes(6 * 1024 * 1024)
            .setPrioritizeTimeOverSizeThresholds(false)
            .build()
        val player = ExoPlayer.Builder(this, renderers)
            .setMediaSourceFactory(DefaultMediaSourceFactory(source))
            .setLoadControl(load)
            .setAudioAttributes(
                AudioAttributes.Builder()
                    .setUsage(C.USAGE_MEDIA)
                    .setContentType(C.AUDIO_CONTENT_TYPE_MUSIC)
                    .build(),
                true,
            )
            .setHandleAudioBecomingNoisy(true)
            .setWakeMode(C.WAKE_MODE_NETWORK)
            .build()
        // A song that cannot be opened is skipped instead of stopping the music; songs played are remembered, and
        // when the queue is about to run out, similar songs are added so the music carries on.
        Widgets.player = player
        Library.widgetHook = { Widgets.refresh(applicationContext) }
        player.addListener(object : Player.Listener {
            override fun onMediaItemTransition(mediaItem: MediaItem?, reason: Int) {
                Widgets.refresh(applicationContext)
                mediaItem?.let { Library.played(it.toSong()) }
                saveQueue(player)
                announce(player)
                if (player.repeatMode == Player.REPEAT_MODE_OFF && player.currentMediaItemIndex >= player.mediaItemCount - 2) more(player)
            }

            override fun onIsPlayingChanged(isPlaying: Boolean) {
                Widgets.refresh(applicationContext)
                if (!isPlaying) saveQueue(player)
                announce(player)
                main.removeCallbacks(heartbeat)
                if (isPlaying) main.postDelayed(heartbeat, 30_000)
            }

            override fun onPlayerError(error: PlaybackException) {
                if (player.hasNextMediaItem()) {
                    player.seekToNextMediaItem()
                    player.prepare()
                    player.play()
                }
            }
        })
        restoreQueue(player)
        session = MediaSession.Builder(this, player)
            .setCallback(object : MediaSession.Callback {
                override fun onAddMediaItems(
                    mediaSession: MediaSession,
                    controller: MediaSession.ControllerInfo,
                    mediaItems: MutableList<MediaItem>,
                ): ListenableFuture<MutableList<MediaItem>> = Futures.immediateFuture(
                    mediaItems.map { it.withPlayUri() }.toMutableList(),
                )
            })
            .build()
    }

    private var current: Player? = null
    private val heartbeat = object : Runnable {
        override fun run() {
            current?.let { announce(it) }
            main.postDelayed(this, 30_000)
        }
    }

    /** Lets friends see what is playing; refreshed every 30 seconds so they also see you are online. */
    private fun announce(player: Player) {
        current = player
        if (!Account.signedIn) return
        val song = if (player.mediaItemCount > 0) player.currentMediaItem?.toSong() else null
        val (position, playing) = player.currentPosition to player.isPlaying
        Thread { Account.presence(song, position, playing) }.start()
    }

    private val queueFile get() = java.io.File(filesDir, "queue.json")

    /** The queue and the place in it, so a phone that closed the app for memory gives it back as it was. */
    private fun saveQueue(player: Player) {
        if (player.mediaItemCount == 0) return
        val json = JSONObject()
            .put("index", player.currentMediaItemIndex)
            .put("position", player.currentPosition)
            .put("songs", org.json.JSONArray((0 until player.mediaItemCount).map { player.getMediaItemAt(it).toSong().toJson() }))
        runCatching { queueFile.writeText(json.toString()) }
    }

    /** Loaded paused and not prepared: nothing is downloaded until the user presses play. */
    private fun restoreQueue(player: Player) {
        runCatching {
            val json = JSONObject(queueFile.readText())
            val array = json.getJSONArray("songs")
            val songs = (0 until array.length()).map { Song.fromJson(array.getJSONObject(it)) }
            if (songs.isEmpty()) return
            val items = songs.map { it.toItem().withPlayUri() }
            player.setMediaItems(items, json.optInt("index").coerceIn(0, items.size - 1), json.optLong("position"))
        }
    }

    /** Asks for songs like the last one in the queue and adds the ones that are new. */
    private fun more(player: Player) {
        if (loadingMore || player.mediaItemCount == 0) return
        loadingMore = true
        val last = player.getMediaItemAt(player.mediaItemCount - 1).mediaId
        Thread {
            val found = runCatching { suggestionsOf(JSONObject(Core.radio(last, 25))) }.getOrDefault(emptyList())
            main.post {
                val queued = HashSet<String>()
                for (i in 0 until player.mediaItemCount) queued += player.getMediaItemAt(i).mediaId
                val fresh = found.filter { queued.add(it.id) }
                if (fresh.isNotEmpty()) player.addMediaItems(fresh.map { it.toItem().withPlayUri() })
                loadingMore = false
            }
        }.start()
    }

    override fun onGetSession(controllerInfo: MediaSession.ControllerInfo): MediaSession? = session

    /** The home-screen widgets' buttons arrive here as plain intents; anything else is the media library's own. */
    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        val action = intent?.action.orEmpty()
        if (action.startsWith("org.roynit.music.widget.")) {
            val player = session?.player
            if (player != null) when (action) {
                Widgets.PLAY -> if (player.isPlaying) player.pause() else {
                    if (player.playbackState == Player.STATE_IDLE) player.prepare()
                    player.play()
                }
                Widgets.NEXT -> player.seekToNextMediaItem()
                Widgets.PREV -> player.seekToPreviousMediaItem()
                Widgets.LIKE -> player.currentMediaItem?.toSong()?.let(Library::toggleFavorite)
                Widgets.PLAY_LIST -> {
                    // A list started from a widget: liked songs play shuffled, a playlist plays in order.
                    val id = intent?.getStringExtra("list").orEmpty()
                    val (name, songs) = Widgets.songsOf(id) ?: ("" to emptyList())
                    if (intent?.getBooleanExtra("feature", false) == true) Widgets.setFeatured(applicationContext, id)
                    if (songs.isNotEmpty()) {
                        Playback.sourceName = name
                        val ordered = if (id == "liked") songs.shuffled() else songs
                        player.setMediaItems(ordered.map { it.toItem().withPlayUri() }, 0, 0)
                        player.prepare()
                        player.play()
                    }
                }
            }
            Widgets.refresh(applicationContext)
            return START_NOT_STICKY
        }
        return super.onStartCommand(intent, flags, startId)
    }

    /** Swiping the app away must not cut the music; only an idle service goes away. */
    override fun onTaskRemoved(rootIntent: Intent?) {
        val player = session?.player
        if (player == null || !player.playWhenReady || player.mediaItemCount == 0) stopSelf()
    }

    override fun onDestroy() {
        Widgets.player = null
        Library.widgetHook = null
        session?.run {
            player.release()
            release()
        }
        session = null
        super.onDestroy()
    }
}
