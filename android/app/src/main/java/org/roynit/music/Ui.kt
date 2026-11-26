package org.roynit.music

import android.content.Context
import android.content.res.ColorStateList
import android.graphics.Color
import android.graphics.Outline
import android.graphics.Typeface
import android.graphics.drawable.ColorDrawable
import android.graphics.drawable.GradientDrawable
import android.graphics.drawable.RippleDrawable
import android.text.TextUtils
import android.util.TypedValue
import android.view.Gravity
import android.view.View
import android.view.ViewGroup
import android.view.ViewOutlineProvider
import android.widget.ImageView
import android.widget.TextView

/** The few colours and building blocks every screen shares. Plain views: nothing here costs memory to keep. */
object Ui {
    const val BG = 0xFF121212.toInt()
    const val SURFACE = 0xFF1E1E1E.toInt()
    const val RAISED = 0xFF2A2A2A.toInt()
    const val MUTED = 0xFFB3B3B3.toInt()
    const val GREEN = 0xFF1ED760.toInt()
}

fun Context.dp(n: Int): Int = (n * resources.displayMetrics.density + 0.5f).toInt()

/** A soft press highlight, drawn by the system on the render thread. */
fun pressable(circle: Boolean = false): RippleDrawable {
    val mask = if (circle) GradientDrawable().apply { shape = GradientDrawable.OVAL; setColor(Color.WHITE) } else ColorDrawable(Color.WHITE)
    return RippleDrawable(ColorStateList.valueOf(0x33FFFFFF), null, mask)
}

fun roundRect(color: Int, radiusPx: Int): GradientDrawable = GradientDrawable().apply {
    setColor(color)
    cornerRadius = radiusPx.toFloat()
}

/**
 * Rounds the corners of a view. A picture is rounded by drawing it through a shader (see [RoundDrawable]), which is
 * far cheaper than clipping it to an outline: lists full of covers scroll smoothly. Other views use an outline clip.
 */
fun View.clipRounded(radiusPx: Int) {
    if (this is ImageView) {
        setTag(R.id.round_radius, radiusPx)
        scaleType = ImageView.ScaleType.FIT_XY
        return
    }
    outlineProvider = object : ViewOutlineProvider() {
        override fun getOutline(view: View, outline: Outline) = outline.setRoundRect(0, 0, view.width, view.height, radiusPx.toFloat())
    }
    clipToOutline = true
}

/** A picture with rounded corners, centre-cropped to whatever size it is given, drawn without clipping. */
class RoundDrawable(private val bitmap: android.graphics.Bitmap, private val radius: Float) : android.graphics.drawable.Drawable() {
    private val paint = android.graphics.Paint(android.graphics.Paint.ANTI_ALIAS_FLAG or android.graphics.Paint.FILTER_BITMAP_FLAG).apply {
        shader = android.graphics.BitmapShader(bitmap, android.graphics.Shader.TileMode.CLAMP, android.graphics.Shader.TileMode.CLAMP)
    }
    private val rect = android.graphics.RectF()

    override fun onBoundsChange(bounds: android.graphics.Rect) {
        rect.set(bounds)
        val scale = maxOf(bounds.width().toFloat() / bitmap.width, bounds.height().toFloat() / bitmap.height)
        val matrix = android.graphics.Matrix()
        matrix.setScale(scale, scale)
        matrix.postTranslate(bounds.left + (bounds.width() - bitmap.width * scale) / 2f, bounds.top + (bounds.height() - bitmap.height * scale) / 2f)
        paint.shader.setLocalMatrix(matrix)
    }

    override fun draw(canvas: android.graphics.Canvas) {
        canvas.drawRoundRect(rect, radius, radius, paint)
    }

    // No size of its own: the picture fills the view, and showing it never makes the list lay itself out again.
    override fun getIntrinsicWidth() = -1
    override fun getIntrinsicHeight() = -1
    override fun setAlpha(alpha: Int) { paint.alpha = alpha }
    override fun setColorFilter(filter: android.graphics.ColorFilter?) { paint.colorFilter = filter }
    @Deprecated("Required by Drawable")
    override fun getOpacity() = android.graphics.PixelFormat.TRANSLUCENT
}

fun Context.label(sp: Float, color: Int = Color.WHITE, bold: Boolean = false, lines: Int = 1): TextView = TextView(this).apply {
    setTextSize(TypedValue.COMPLEX_UNIT_SP, sp)
    setTextColor(color)
    if (bold) setTypeface(typeface, Typeface.BOLD)
    maxLines = lines
    ellipsize = TextUtils.TruncateAt.END
    includeFontPadding = false
}

fun Context.icon(res: Int, tint: Int = Color.WHITE, sizeDp: Int = 44, padDp: Int = 10): ImageView = ImageView(this).apply {
    setImageResource(res)
    setColorFilter(tint)
    setPadding(dp(padDp), dp(padDp), dp(padDp), dp(padDp))
    layoutParams = android.widget.LinearLayout.LayoutParams(dp(sizeDp), dp(sizeDp))
    background = pressable(circle = true)
    scaleType = ImageView.ScaleType.FIT_CENTER
    isClickable = true
}

fun center(): Int = Gravity.CENTER

/**
 * Grey placeholder rows that pulse softly while a page loads, so the screen never sits empty. The pulse is an alpha
 * animation, which the GPU does on its own, and it stops by itself when the rows leave the screen.
 */
fun Context.skeleton(rows: Int = 6, round: Boolean = false): android.widget.LinearLayout = android.widget.LinearLayout(this).apply {
    orientation = android.widget.LinearLayout.VERTICAL
    repeat(rows) {
        addView(
            android.widget.LinearLayout(context).apply {
                gravity = Gravity.CENTER_VERTICAL
                setPadding(dp(16), dp(8), dp(16), dp(8))
                addView(View(context).apply { background = roundRect(Ui.RAISED, if (round) dp(28) else dp(6)) }, android.widget.LinearLayout.LayoutParams(dp(56), dp(56)))
                addView(
                    android.widget.LinearLayout(context).apply {
                        orientation = android.widget.LinearLayout.VERTICAL
                        addView(View(context).apply { background = roundRect(Ui.RAISED, dp(4)) }, android.widget.LinearLayout.LayoutParams(dp(150 + (it * 37) % 90), dp(14)))
                        addView(View(context).apply { background = roundRect(Ui.RAISED, dp(4)) }, android.widget.LinearLayout.LayoutParams(dp(90 + (it * 23) % 60), dp(11)).apply { topMargin = dp(8) })
                    },
                    android.widget.LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f).apply { marginStart = dp(12) },
                )
            },
        )
    }
    val pulse = android.animation.ObjectAnimator.ofFloat(this, View.ALPHA, 0.45f, 1f).apply {
        duration = 800
        repeatMode = android.animation.ValueAnimator.REVERSE
        repeatCount = android.animation.ValueAnimator.INFINITE
    }
    setLayerType(View.LAYER_TYPE_HARDWARE, null)
    // Only while it is really on screen: a hidden (GONE) skeleton still counts as attached and would keep a frame going every vsync.
    viewTreeObserver.addOnGlobalLayoutListener { if (isShown) { if (!pulse.isStarted) pulse.start() } else if (pulse.isStarted) pulse.cancel() }
    addOnAttachStateChangeListener(object : View.OnAttachStateChangeListener {
        override fun onViewAttachedToWindow(v: View) {}
        override fun onViewDetachedFromWindow(v: View) = pulse.cancel()
    })
}
