package org.roynit.music

import android.app.Activity
import android.content.Context
import android.content.ContextWrapper
import android.graphics.Color
import android.view.Gravity
import android.view.View
import android.view.ViewGroup
import android.widget.FrameLayout
import android.widget.LinearLayout
import android.widget.TextView

/**
 * A short message in the app's own look: a dark rounded pill that floats above the player bar, slides up, waits and
 * fades. It replaces the system toast, which is a grey box that ignores the app's colours.
 */
private var current: View? = null

fun Context.say(text: String) {
    var base: Context = this
    while (base is ContextWrapper && base !is Activity) base = base.baseContext
    val activity = base as? Activity ?: return
    val host = activity.findViewById<ViewGroup>(android.R.id.content)
    current?.let { (it.parent as? ViewGroup)?.removeView(it) }
    val pill = TextView(activity).apply {
        this.text = text
        setTextColor(Color.WHITE)
        textSize = 14f
        maxLines = 4
        gravity = Gravity.CENTER_VERTICAL
        background = roundRect(0xFF3A3A3A.toInt(), activity.dp(12))
        setPadding(activity.dp(16), activity.dp(12), activity.dp(16), activity.dp(12))
        alpha = 0f
        translationY = activity.dp(16).toFloat()
        elevation = activity.dp(6).toFloat()
        isClickable = true
        setOnClickListener { (parent as? ViewGroup)?.removeView(this) }
    }
    current = pill
    host.addView(pill, FrameLayout.LayoutParams(ViewGroup.LayoutParams.WRAP_CONTENT, ViewGroup.LayoutParams.WRAP_CONTENT, Gravity.BOTTOM or Gravity.CENTER_HORIZONTAL).apply {
        setMargins(activity.dp(16), 0, activity.dp(16), activity.dp(150))
    })
    pill.animate().alpha(1f).translationY(0f).setDuration(180).withLayer().start()
    pill.postDelayed({
        pill.animate().alpha(0f).setDuration(220).withLayer().withEndAction { (pill.parent as? ViewGroup)?.removeView(pill) }.start()
    }, (1800 + text.length * 25L).coerceAtMost(5000))
}

/** A centred question or form card, in the same dark rounded style as the sheets, instead of the system dialog. */
fun Activity.ask(title: String, message: String = "", content: View? = null, ok: String, cancel: String = "Cancel", danger: Boolean = false, run: () -> Unit) {
    val dialog = android.app.Dialog(this)
    dialog.requestWindowFeature(android.view.Window.FEATURE_NO_TITLE)
    val card = LinearLayout(this).apply {
        orientation = LinearLayout.VERTICAL
        background = roundRect(0xFF282828.toInt(), dp(16))
        setPadding(dp(24), dp(24), dp(24), dp(16))
    }
    card.addView(label(20f, Color.WHITE, bold = true, lines = 3).apply { text = title })
    if (message.isNotEmpty()) card.addView(label(15f, Ui.MUTED, lines = 8).apply { text = message; setPadding(0, dp(10), 0, 0) })
    if (content != null) card.addView(content, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT).apply { topMargin = dp(16) })
    val buttons = LinearLayout(this).apply { gravity = Gravity.END or Gravity.CENTER_VERTICAL }
    buttons.addView(label(15f, Color.WHITE, bold = true).apply {
        text = cancel
        background = pressable()
        setPadding(dp(18), dp(12), dp(18), dp(12))
        setOnClickListener { dialog.dismiss() }
    })
    buttons.addView(label(15f, Color.BLACK, bold = true).apply {
        text = ok
        background = roundRect(if (danger) 0xFFFF6B6B.toInt() else Ui.GREEN, dp(22))
        setPadding(dp(22), dp(11), dp(22), dp(11))
        setOnClickListener { dialog.dismiss(); run() }
    }, LinearLayout.LayoutParams(ViewGroup.LayoutParams.WRAP_CONTENT, ViewGroup.LayoutParams.WRAP_CONTENT).apply { marginStart = dp(8) })
    card.addView(buttons, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT).apply { topMargin = dp(20) })
    dialog.setContentView(card)
    dialog.window?.apply {
        setBackgroundDrawable(android.graphics.drawable.ColorDrawable(Color.TRANSPARENT))
        setLayout((resources.displayMetrics.widthPixels * 0.88f).toInt(), ViewGroup.LayoutParams.WRAP_CONTENT)
        setWindowAnimations(android.R.style.Animation_Dialog)
        setDimAmount(0.7f)
        setSoftInputMode(android.view.WindowManager.LayoutParams.SOFT_INPUT_STATE_VISIBLE or android.view.WindowManager.LayoutParams.SOFT_INPUT_ADJUST_RESIZE)
    }
    dialog.show()
}

/** One text box in the app's style, for the dialogs that ask for a name. */
fun Activity.field(hint: String, text: String = ""): android.widget.EditText = android.widget.EditText(this).apply {
    this.hint = hint
    setText(text)
    setSelection(text.length)
    setSingleLine()
    setTextColor(Color.WHITE)
    setHintTextColor(Ui.MUTED)
    textSize = 16f
    background = roundRect(0xFF161616.toInt(), dp(8))
    setPadding(dp(14), dp(12), dp(14), dp(12))
    setBackgroundTintList(null)
}
