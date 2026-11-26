package org.roynit.music

import android.app.Activity
import android.app.Dialog
import android.graphics.Color
import android.graphics.drawable.ColorDrawable
import android.graphics.drawable.GradientDrawable
import android.view.Gravity
import android.view.View
import android.view.ViewGroup
import android.view.Window
import android.widget.ImageView
import android.widget.LinearLayout
import android.widget.ScrollView

/**
 * The menu that slides up from the bottom, like Spotify's: a rounded dark sheet with the song (or a title) on top and
 * large rows below. It replaces the small pop-up menus, which are hard to hit with a thumb.
 */
object Sheet {
    class Option(val label: String, val icon: Int? = null, val color: Int = Color.WHITE, val run: () -> Unit)

    fun show(activity: Activity, title: String, subtitle: String = "", cover: String? = null, options: List<Option>) {
        val dialog = Dialog(activity)
        dialog.requestWindowFeature(Window.FEATURE_NO_TITLE)
        val c = activity
        val column = LinearLayout(c).apply {
            orientation = LinearLayout.VERTICAL
            background = GradientDrawable().apply {
                setColor(0xFF282828.toInt())
                cornerRadii = floatArrayOf(c.dp(16).toFloat(), c.dp(16).toFloat(), c.dp(16).toFloat(), c.dp(16).toFloat(), 0f, 0f, 0f, 0f)
            }
            setPadding(0, c.dp(8), 0, c.dp(20))
        }
        // A short bar at the top says this can be dragged away or tapped outside of.
        column.addView(View(c).apply { background = roundRect(0x55FFFFFF, c.dp(2)) }, LinearLayout.LayoutParams(c.dp(36), c.dp(4)).apply { gravity = Gravity.CENTER_HORIZONTAL; bottomMargin = c.dp(10) })
        val head = LinearLayout(c).apply {
            gravity = Gravity.CENTER_VERTICAL
            setPadding(c.dp(20), c.dp(6), c.dp(20), c.dp(14))
            if (cover != null) {
                val art = ImageView(c).apply {
                    scaleType = ImageView.ScaleType.CENTER_CROP
                    background = roundRect(Ui.RAISED, c.dp(4))
                    clipRounded(c.dp(4))
                }
                Images.load(art, cover, c.dp(112))
                addView(art, LinearLayout.LayoutParams(c.dp(52), c.dp(52)).apply { marginEnd = c.dp(14) })
            }
            addView(
                LinearLayout(c).apply {
                    orientation = LinearLayout.VERTICAL
                    addView(c.label(17f, Color.WHITE, bold = true).apply { text = title })
                    if (subtitle.isNotEmpty()) addView(c.label(14f, Ui.MUTED).apply { text = subtitle; setPadding(0, c.dp(2), 0, 0) })
                },
                LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f),
            )
        }
        if (title.isNotEmpty()) {
            column.addView(head)
            column.addView(View(c).apply { setBackgroundColor(0x22FFFFFF) }, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, 1).apply { bottomMargin = c.dp(4) })
        }
        val rows = LinearLayout(c).apply { orientation = LinearLayout.VERTICAL }
        for (option in options) {
            rows.addView(
                LinearLayout(c).apply {
                    gravity = Gravity.CENTER_VERTICAL
                    background = pressable()
                    setPadding(c.dp(20), 0, c.dp(20), 0)
                    if (option.icon != null) addView(ImageView(c).apply { setImageResource(option.icon); setColorFilter(option.color) }, LinearLayout.LayoutParams(c.dp(24), c.dp(24)).apply { marginEnd = c.dp(18) })
                    addView(c.label(16f, option.color).apply { text = option.label })
                    setOnClickListener {
                        dialog.dismiss()
                        option.run()
                    }
                },
                LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, c.dp(54)),
            )
        }
        column.addView(ScrollView(c).apply { addView(rows) }, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT))
        dialog.setContentView(column)
        dialog.window?.apply {
            setBackgroundDrawable(ColorDrawable(Color.TRANSPARENT))
            setGravity(Gravity.BOTTOM)
            setLayout(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT)
            setWindowAnimations(android.R.style.Animation_InputMethod)
            setDimAmount(0.6f)
        }
        dialog.show()
    }
}

/** A drop-in for the system pop-up menu that opens as a [Sheet], so every menu in the app looks the same. */
class SheetMenu(private val context: android.content.Context, private val anchor: View) {
    class Item(val itemId: Int, val title: String)

    class Items {
        val list = ArrayList<Item>()
        fun add(group: Int, id: Int, order: Int, title: String) { list += Item(id, title) }
    }

    val menu = Items()
    private var listener: ((Item) -> Boolean)? = null
    fun setOnMenuItemClickListener(l: (Item) -> Boolean) { listener = l }

    fun show() {
        var base = context
        while (base is android.content.ContextWrapper && base !is Activity) base = base.baseContext
        val activity = base as? Activity ?: return
        Sheet.show(activity, "", "", null, menu.list.map { item ->
            val danger = item.title.startsWith("Delete") || item.title.startsWith("Remove")
            Sheet.Option(item.title, iconFor(item.title), if (danger) 0xFFFF6B6B.toInt() else Color.WHITE) { listener?.invoke(item) }
        })
    }

    /** A fitting icon for the usual menu entries; others go without. */
    private fun iconFor(title: String): Int? = when {
        title.startsWith("Delete") || title.startsWith("Remove") -> R.drawable.ic_close
        title.startsWith("Rename") -> R.drawable.ic_edit
        title.startsWith("Share") -> R.drawable.ic_share
        title.startsWith("Sleep") -> R.drawable.ic_moon
        title.startsWith("Blend") -> R.drawable.ic_blend
        title.startsWith("Import") -> R.drawable.ic_download
        title == "Playlist" -> R.drawable.ic_add
        title.contains("adio") -> R.drawable.ic_shuffle
        title.contains("liked") -> R.drawable.ic_heart_outline
        title == "Charts" -> R.drawable.ic_stats
        title.contains("home") -> R.drawable.ic_home_outline
        else -> null
    }
}
