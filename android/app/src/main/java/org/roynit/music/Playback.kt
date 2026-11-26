package org.roynit.music

import androidx.media3.common.Player
import androidx.media3.session.MediaController

/** The screen's handle on the player that lives in the service. */
object Playback {
    var controller: MediaController? = null

    val song: Song? get() = controller?.currentMediaItem?.toSong()

    /** What the full player says under "Playing from": a playlist, a mix, liked songs. */
    var sourceName = ""

    /** Pages set this just before they start a list, so the player can say where it came from. */
    var nextSource = ""

    private val handler = android.os.Handler(android.os.Looper.getMainLooper())
    private val sleep = Runnable { controller?.pause() }

    /** The sleep timer: the music pauses after [minutes]; 0 turns it off. */
    fun sleepIn(minutes: Int) {
        handler.removeCallbacks(sleep)
        if (minutes > 0) handler.postDelayed(sleep, minutes * 60_000L)
    }

    fun play(songs: List<Song>, index: Int) {
        val c = controller ?: return
        sourceName = nextSource
        nextSource = ""
        c.setMediaItems(songs.map(Song::toItem), index, 0)
        c.prepare()
        c.play()
    }

    /** Right after the song that is playing. */
    fun playNext(song: Song) {
        val c = controller ?: return
        if (c.mediaItemCount == 0) play(listOf(song), 0) else c.addMediaItem(c.currentMediaItemIndex + 1, song.toItem())
    }

    fun enqueue(song: Song) {
        val c = controller ?: return
        if (c.mediaItemCount == 0) play(listOf(song), 0) else c.addMediaItem(song.toItem())
    }

    fun toggle() {
        val c = controller ?: return
        if (c.isPlaying) {
            c.pause()
        } else {
            // A queue brought back after the app was closed has not been prepared yet.
            if (c.playbackState == Player.STATE_IDLE) c.prepare()
            c.play()
        }
    }

    /** Off, then repeat the queue, then repeat one song. */
    fun cycleRepeat() {
        val c = controller ?: return
        c.repeatMode = when (c.repeatMode) {
            Player.REPEAT_MODE_OFF -> Player.REPEAT_MODE_ALL
            Player.REPEAT_MODE_ALL -> Player.REPEAT_MODE_ONE
            else -> Player.REPEAT_MODE_OFF
        }
    }
}
