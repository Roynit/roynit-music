package org.roynit.music

import android.content.Context
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.Paint
import android.graphics.RectF
import android.view.MotionEvent
import android.view.View

/** A thin seek bar like Spotify's: a 4 dp track, a white fill and a small thumb that grows while it is dragged. */
class Scrubber(context: Context) : View(context) {
    var max = 1
        set(value) {
            field = value.coerceAtLeast(1)
            invalidate()
        }
    var progress = 0
        set(value) {
            field = value.coerceIn(0, max)
            invalidate()
        }

    /** Called while dragging and once when the finger lifts. */
    var onScrub: ((value: Int, done: Boolean) -> Unit)? = null
    var dragging = false
        private set

    private val track = Paint(Paint.ANTI_ALIAS_FLAG).apply { color = 0x4DFFFFFF }
    private val fill = Paint(Paint.ANTI_ALIAS_FLAG).apply { color = Color.WHITE }
    private val rect = RectF()
    private val d = context.resources.displayMetrics.density

    override fun onMeasure(widthSpec: Int, heightSpec: Int) {
        setMeasuredDimension(MeasureSpec.getSize(widthSpec), (28 * d).toInt())
    }

    private fun fraction() = progress.toFloat() / max

    override fun onDraw(canvas: Canvas) {
        val pad = 8 * d
        val left = pad
        val right = width - pad
        val cy = height / 2f
        val half = 2 * d
        rect.set(left, cy - half, right, cy + half)
        canvas.drawRoundRect(rect, half, half, track)
        val x = left + (right - left) * fraction()
        rect.set(left, cy - half, x, cy + half)
        canvas.drawRoundRect(rect, half, half, fill)
        canvas.drawCircle(x, cy, (if (dragging) 8 else 6) * d, fill)
    }

    override fun onTouchEvent(event: MotionEvent): Boolean {
        val pad = 8 * d
        val value = (((event.x - pad) / (width - 2 * pad)).coerceIn(0f, 1f) * max).toInt()
        when (event.actionMasked) {
            MotionEvent.ACTION_DOWN -> {
                parent?.requestDisallowInterceptTouchEvent(true)
                dragging = true
                progress = value
                onScrub?.invoke(value, false)
            }
            MotionEvent.ACTION_MOVE -> if (dragging) {
                progress = value
                onScrub?.invoke(value, false)
            }
            MotionEvent.ACTION_UP, MotionEvent.ACTION_CANCEL -> if (dragging) {
                dragging = false
                progress = value
                onScrub?.invoke(value, true)
                invalidate()
            }
        }
        return true
    }
}
