package org.roynit.music

import android.content.Context
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.Paint
import android.graphics.Path
import android.graphics.RectF
import android.net.Uri
import android.view.Gravity
import android.view.MotionEvent
import android.view.ScaleGestureDetector
import android.view.View
import android.view.ViewGroup
import android.widget.FrameLayout
import android.widget.LinearLayout
import android.widget.SeekBar

/**
 * Pick the part of a picture to keep. Drag to move, pinch or use the slider to zoom; what shows inside the
 * window (a circle for a profile picture, a wide strip for a cover) is exactly what is saved.
 */
class CropView(context: Context, private val onDone: (Bitmap?) -> Unit) : FrameLayout(context) {
    private val c = context
    private val canvasView = Surface(context)
    private val zoom = SeekBar(context)

    init {
        setBackgroundColor(0xFF000000.toInt())
        visibility = GONE
        isClickable = true
        addView(canvasView, LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT))
        zoom.apply {
            max = 100
            progressTintList = android.content.res.ColorStateList.valueOf(Color.WHITE)
            progressBackgroundTintList = android.content.res.ColorStateList.valueOf(0x55FFFFFF)
            thumbTintList = android.content.res.ColorStateList.valueOf(Color.WHITE)
            setOnSeekBarChangeListener(object : SeekBar.OnSeekBarChangeListener {
                override fun onProgressChanged(bar: SeekBar, progress: Int, fromUser: Boolean) {
                    if (fromUser) canvasView.zoomTo(progress / 100f)
                }

                override fun onStartTrackingTouch(bar: SeekBar) = Unit
                override fun onStopTrackingTouch(bar: SeekBar) = Unit
            })
        }
        val cancel = c.label(15f, Color.WHITE, bold = true).apply {
            text = "Cancel"
            gravity = Gravity.CENTER
            background = roundRect(0xFF2A2A2A.toInt(), c.dp(24))
            setPadding(c.dp(28), c.dp(12), c.dp(28), c.dp(12))
            setOnClickListener { finish(null) }
        }
        val apply = c.label(15f, Color.BLACK, bold = true).apply {
            text = "Apply"
            gravity = Gravity.CENTER
            background = roundRect(Color.WHITE, c.dp(24))
            setPadding(c.dp(36), c.dp(12), c.dp(36), c.dp(12))
            setOnClickListener { finish(canvasView.result()) }
        }
        val bar = LinearLayout(c).apply {
            orientation = LinearLayout.VERTICAL
            gravity = Gravity.CENTER_HORIZONTAL
            setPadding(c.dp(24), c.dp(8), c.dp(24), c.dp(24))
            addView(zoom, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT))
            addView(LinearLayout(c).apply {
                gravity = Gravity.CENTER
                addView(cancel)
                addView(apply, LinearLayout.LayoutParams(ViewGroup.LayoutParams.WRAP_CONTENT, ViewGroup.LayoutParams.WRAP_CONTENT).apply { leftMargin = c.dp(14) })
            }, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT).apply { topMargin = c.dp(16) })
        }
        addView(bar, LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT, Gravity.BOTTOM))
        addView(c.label(16f, Color.WHITE, bold = true).apply { text = "Move and zoom"; gravity = Gravity.CENTER; setPadding(0, c.dp(20), 0, 0) },
            LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT, Gravity.TOP))
    }

    private fun finish(bitmap: Bitmap?) {
        visibility = GONE
        canvasView.clear()
        onDone(bitmap)
    }

    /** Shows the cropper for a picture. [circle] draws a round window; the window's shape is [outW] by [outH]. */
    fun open(uri: Uri, circle: Boolean, outW: Int, outH: Int): Boolean {
        val bitmap = runCatching {
            val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
            c.contentResolver.openInputStream(uri)!!.use { BitmapFactory.decodeStream(it, null, bounds) }
            var sample = 1
            while (maxOf(bounds.outWidth, bounds.outHeight) / (sample * 2) >= 2048) sample *= 2
            c.contentResolver.openInputStream(uri)!!.use { BitmapFactory.decodeStream(it, null, BitmapFactory.Options().apply { inSampleSize = sample }) }!!
        }.getOrNull() ?: return false
        canvasView.set(bitmap, circle, outW, outH)
        zoom.progress = 0
        visibility = VISIBLE
        return true
    }

    private inner class Surface(context: Context) : View(context) {
        private var image: Bitmap? = null
        private var round = false
        private var outW = 1
        private var outH = 1
        private var scale = 1f
        private var left = 0f
        private var top = 0f
        private val window = RectF()
        private val dim = Paint().apply { color = 0xB3000000.toInt() }
        private val ring = Paint(Paint.ANTI_ALIAS_FLAG).apply { color = Color.WHITE; style = Paint.Style.STROKE; strokeWidth = c.dp(2).toFloat() }
        private val guide = Paint(Paint.ANTI_ALIAS_FLAG).apply { color = 0x66FFFFFF; style = Paint.Style.STROKE; strokeWidth = c.dp(1).toFloat() }
        private val paint = Paint(Paint.FILTER_BITMAP_FLAG or Paint.ANTI_ALIAS_FLAG)
        private var lastX = 0f
        private var lastY = 0f
        private var dragging = false
        private val pinch = ScaleGestureDetector(context, object : ScaleGestureDetector.SimpleOnScaleGestureListener() {
            override fun onScale(d: ScaleGestureDetector): Boolean {
                zoomBy(d.scaleFactor, d.focusX, d.focusY)
                return true
            }
        })

        fun set(bitmap: Bitmap, circle: Boolean, w: Int, h: Int) {
            image = bitmap
            round = circle
            outW = w
            outH = h
            layoutWindow()
            reset()
        }

        fun clear() {
            image = null
        }

        private fun layoutWindow() {
            if (width == 0) return
            val pad = c.dp(24).toFloat()
            val ratio = outW.toFloat() / outH
            var w = width - 2 * pad
            var h = w / ratio
            val maxH = height * 0.62f
            if (h > maxH) {
                h = maxH
                w = h * ratio
            }
            window.set((width - w) / 2, (height - h) / 2 - c.dp(16), (width + w) / 2, (height + h) / 2 - c.dp(16))
        }

        private fun minScale() = image?.let { maxOf(window.width() / it.width, window.height() / it.height) } ?: 1f

        private fun reset() {
            val img = image ?: return
            scale = minScale()
            left = window.centerX() - img.width * scale / 2
            top = window.centerY() - img.height * scale / 2
            invalidate()
        }

        override fun onSizeChanged(w: Int, h: Int, oldw: Int, oldh: Int) {
            layoutWindow()
            reset()
        }

        /** The slider's 0..1: from "fits the window" to four times that, around the window's centre. */
        fun zoomTo(fraction: Float) = zoomBy((minScale() * (1 + fraction * 4)) / scale, window.centerX(), window.centerY())

        private fun zoomBy(factor: Float, px: Float, py: Float) {
            val min = minScale()
            val next = (scale * factor).coerceIn(min, min * 5)
            val f = next / scale
            left = px - (px - left) * f
            top = py - (py - top) * f
            scale = next
            keepCovering()
            zoom.progress = (((scale / min) - 1) / 4 * 100).toInt().coerceIn(0, 100)
            invalidate()
        }

        private fun keepCovering() {
            val img = image ?: return
            left = left.coerceIn(window.right - img.width * scale, window.left)
            top = top.coerceIn(window.bottom - img.height * scale, window.top)
        }

        override fun onTouchEvent(event: MotionEvent): Boolean {
            pinch.onTouchEvent(event)
            if (pinch.isInProgress) {
                dragging = false
                return true
            }
            when (event.actionMasked) {
                MotionEvent.ACTION_DOWN -> {
                    lastX = event.x
                    lastY = event.y
                    dragging = true
                }
                MotionEvent.ACTION_MOVE -> if (dragging) {
                    left += event.x - lastX
                    top += event.y - lastY
                    lastX = event.x
                    lastY = event.y
                    keepCovering()
                    invalidate()
                }
                MotionEvent.ACTION_POINTER_UP -> {
                    // Carry on dragging with the finger that is left.
                    val i = if (event.actionIndex == 0) 1 else 0
                    lastX = event.getX(i)
                    lastY = event.getY(i)
                    dragging = true
                }
                MotionEvent.ACTION_UP, MotionEvent.ACTION_CANCEL -> dragging = false
            }
            return true
        }

        override fun onDraw(canvas: Canvas) {
            val img = image ?: return
            canvas.save()
            canvas.translate(left, top)
            canvas.scale(scale, scale)
            canvas.drawBitmap(img, 0f, 0f, paint)
            canvas.restore()
            // Everything outside the window is dimmed.
            val hole = Path().apply {
                if (round) addOval(window, Path.Direction.CW) else addRoundRect(window, c.dp(10).toFloat(), c.dp(10).toFloat(), Path.Direction.CW)
            }
            canvas.save()
            canvas.clipOutPath(hole)
            canvas.drawRect(0f, 0f, width.toFloat(), height.toFloat(), dim)
            canvas.restore()
            if (round) canvas.drawOval(window, ring) else canvas.drawRoundRect(window, c.dp(10).toFloat(), c.dp(10).toFloat(), ring)
            // Thirds help line a cover up.
            if (!round) {
                for (i in 1..2) {
                    canvas.drawLine(window.left + window.width() * i / 3, window.top, window.left + window.width() * i / 3, window.bottom, guide)
                    canvas.drawLine(window.left, window.top + window.height() * i / 3, window.right, window.top + window.height() * i / 3, guide)
                }
            }
        }

        /** What is inside the window, at the saved size. */
        fun result(): Bitmap? {
            val img = image ?: return null
            val out = Bitmap.createBitmap(outW, outH, Bitmap.Config.ARGB_8888)
            val canvas = Canvas(out)
            canvas.drawColor(Color.BLACK)
            val k = outW / window.width()
            canvas.scale(k, k)
            canvas.translate(-window.left, -window.top)
            canvas.translate(left, top)
            canvas.scale(scale, scale)
            canvas.drawBitmap(img, 0f, 0f, paint)
            return out
        }
    }
}
