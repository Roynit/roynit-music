package org.roynit.music

import android.app.Activity
import android.content.Context
import android.graphics.Color
import android.view.Gravity
import android.view.View
import android.view.ViewGroup
import android.widget.FrameLayout
import android.widget.HorizontalScrollView
import android.widget.ImageView
import android.widget.LinearLayout
import org.json.JSONObject

/**
 * The Orbs shop and the daily quests. Quests are counted by the server from what you listen to; the shop sells
 * avatar decorations, nameplates, profile frames and profile effects, each in a few colours.
 */
class ShopScreen(private val c: Context, private val onChange: () -> Unit, private val back: () -> Unit, private val openProfile: () -> Unit) {
    private var tab = "quests"
    private var quests: JSONObject? = null
    private var loading = false
    private val chosen = HashMap<String, Int>() // item id -> variant index

    private val tabs = listOf("quests" to "Quests", "decoration" to "Avatar", "nameplate" to "Nameplates", "frame" to "Frames", "effect" to "Effects")

    fun load() {
        if (loading) return
        loading = true
        Thread {
            val shop = Account.call("GET", "/shop")
            val daily = Account.call("GET", "/quests")
            (c as Activity).runOnUiThread {
                loading = false
                if (!shop.has("error")) Shop.apply(shop)
                quests = daily.takeIf { !it.has("error") }
                onChange()
            }
        }.start()
    }

    private fun act(path: String, body: JSONObject, done: (JSONObject) -> Unit = {}) {
        Thread {
            val reply = Account.call("POST", path, body)
            (c as Activity).runOnUiThread {
                if (reply.has("error")) c.say(reply.optString("error")) else { done(reply); onChange() }
            }
        }.start()
    }

    private fun pill(text: String, color: Int = Color.WHITE, fill: Int = Ui.RAISED) = c.label(13f, color, bold = true).apply {
        this.text = text
        gravity = Gravity.CENTER
        background = roundRect(fill, c.dp(14))
        setPadding(c.dp(12), c.dp(6), c.dp(12), c.dp(6))
    }

    private fun button(text: String, enabled: Boolean = true, primary: Boolean = true, run: () -> Unit) = c.label(14f, if (primary && enabled) Color.BLACK else if (enabled) Color.WHITE else Ui.MUTED, bold = true).apply {
        this.text = text
        gravity = Gravity.CENTER
        background = roundRect(if (primary && enabled) Color.WHITE else Ui.RAISED, c.dp(20))
        setPadding(c.dp(14), c.dp(10), c.dp(14), c.dp(10))
        if (enabled) { foreground = pressable(); setOnClickListener { run() } }
    }

    fun draw(body: LinearLayout) {
        val top = LinearLayout(c).apply {
            gravity = Gravity.CENTER_VERTICAL
            addView(c.icon(R.drawable.ic_down, Color.WHITE, 44, 11).apply { rotation = 90f; setOnClickListener { back() } })
            addView(c.label(26f, Color.WHITE, bold = true).apply { text = "Shop"; setPadding(c.dp(6), 0, 0, 0) }, LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f))
            addView(pill("◆  ${Shop.orbs}", Color.WHITE, 0xFF2A2A2A.toInt()))
        }
        body.addView(top, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT))
        val chips = LinearLayout(c).apply {
            tabs.forEach { (key, label) ->
                addView(c.label(14f, if (tab == key) Color.BLACK else Color.WHITE, bold = true).apply {
                    text = label
                    gravity = Gravity.CENTER
                    background = roundRect(if (tab == key) Color.WHITE else Ui.RAISED, c.dp(18))
                    setPadding(c.dp(16), c.dp(9), c.dp(16), c.dp(9))
                    setOnClickListener { tab = key; onChange() }
                }, LinearLayout.LayoutParams(ViewGroup.LayoutParams.WRAP_CONTENT, ViewGroup.LayoutParams.WRAP_CONTENT).apply { marginEnd = c.dp(8) })
            }
        }
        body.addView(HorizontalScrollView(c).apply { isHorizontalScrollBarEnabled = false; addView(chips) },
            LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT).apply { topMargin = c.dp(10); bottomMargin = c.dp(6) })
        if (tab == "quests") drawQuests(body) else drawItems(body, tab)
        body.addView(View(c), LinearLayout.LayoutParams(1, c.dp(40)))
    }

    private fun drawQuests(body: LinearLayout) {
        val q = quests
        if (q == null) {
            body.addView(c.label(14f, Ui.MUTED).apply { text = if (loading) "Loading..." else "Could not load your quests. Pull back and try again."; setPadding(0, c.dp(20), 0, 0) })
            return
        }
        val left = q.optLong("resets_in")
        body.addView(c.label(20f, Color.WHITE, bold = true).apply { text = "Daily quests"; setPadding(0, c.dp(12), 0, 0) })
        body.addView(c.label(13f, Ui.MUTED, lines = 2).apply {
            text = "Listen with Roy Music to make progress. New quests in ${left / 3600}h ${left / 60 % 60}m."
            ellipsize = null
            setPadding(0, c.dp(4), 0, c.dp(8))
        })
        val list = q.optJSONArray("quests") ?: return
        for (i in 0 until list.length()) {
            val quest = list.getJSONObject(i)
            val tier = quest.optString("tier")
            val accent = when (tier) { "easy" -> Ui.GREEN; "medium" -> 0xFFFFB84D.toInt(); else -> 0xFFFF6B6B.toInt() }
            val progress = quest.optInt("progress")
            val target = quest.optInt("target").coerceAtLeast(1)
            val done = quest.optBoolean("done")
            val claimed = quest.optBoolean("claimed")
            val bar = LinearLayout(c).apply {
                background = roundRect(Ui.RAISED, c.dp(4))
                addView(View(c).apply { background = roundRect(accent, c.dp(4)) }, LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.MATCH_PARENT, progress.toFloat()))
                addView(View(c), LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.MATCH_PARENT, (target - progress).toFloat().coerceAtLeast(0f)))
            }
            val card = LinearLayout(c).apply {
                orientation = LinearLayout.VERTICAL
                background = roundRect(Ui.SURFACE, c.dp(16))
                setPadding(c.dp(16), c.dp(14), c.dp(16), c.dp(14))
                addView(LinearLayout(c).apply {
                    gravity = Gravity.CENTER_VERTICAL
                    addView(c.label(11f, accent, bold = true).apply { text = tier.uppercase() }, LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f))
                    addView(pill("◆  ${quest.optInt("orbs")}", Color.WHITE, Ui.RAISED))
                })
                addView(c.label(16f, Color.WHITE, bold = true, lines = 2).apply { text = quest.optString("title"); setPadding(0, c.dp(6), 0, c.dp(10)) })
                addView(bar, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, c.dp(8)))
                addView(LinearLayout(c).apply {
                    gravity = Gravity.CENTER_VERTICAL
                    addView(c.label(13f, Ui.MUTED).apply { text = "$progress / $target" }, LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f))
                    addView(
                        when {
                            claimed -> button("Claimed", enabled = false) {}
                            done -> button("Claim ◆ ${quest.optInt("orbs")}") { act("/quests/claim", JSONObject().put("id", quest.optString("id"))) { r -> quests = r; Shop.orbs = r.optInt("orbs", Shop.orbs); c.say("Orbs added") } }
                            else -> button("Keep listening", enabled = false) {}
                        },
                    )
                }, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT).apply { topMargin = c.dp(10) })
            }
            body.addView(card, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT).apply { topMargin = c.dp(10) })
        }
        body.addView(c.label(12f, Ui.MUTED, lines = 4).apply {
            text = "Each quest pays once a day, and days start at midnight in Vietnam. Orbs are earned only by listening, and they cannot be turned into money."
            ellipsize = null
            setPadding(0, c.dp(16), 0, 0)
        })
    }

    private fun drawItems(body: LinearLayout, kind: String) {
        val list = Shop.items.filter { it.kind == kind }
        var row: LinearLayout? = null
        list.forEachIndexed { i, item ->
            if (i % 2 == 0) {
                row = LinearLayout(c).apply { clipChildren = false }
                body.addView(row, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT).apply { topMargin = c.dp(10) })
            }
            val holder = FrameLayout(c)
            fill(holder, item)
            row!!.addView(holder, LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f).apply { if (i % 2 == 0) marginEnd = c.dp(5) else marginStart = c.dp(5) })
        }
    }

    /** One shop card. Picking a colour redraws just this card. */
    private fun fill(holder: FrameLayout, item: Shop.Item) {
        holder.removeAllViews()
        val index = chosen[item.id] ?: 0
        val variant = item.variants[index]
        val key = item.id + ":" + variant.id
        val owns = key in Shop.owned
        val wearing = Shop.equipped.optString(item.kind) == key

        val stage = FrameLayout(c).apply {
            background = roundRect(0xFF17171C.toInt(), c.dp(12))
            clipChildren = false
        }
        val h = if (item.kind == "nameplate") 84 else 150
        when (item.kind) {
            "decoration" -> {
                val avatar = FrameLayout(c).apply {
                    background = roundRect(0xFF5C6EAA.toInt(), c.dp(40))
                    addView(c.label(28f, Color.WHITE, bold = true).apply { text = "R"; gravity = Gravity.CENTER }, FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT))
                }
                val box = JSONObject().put("decoration", key)
                stage.addView(Shop.decorate(c, avatar, 64, box).also { it.layoutParams = FrameLayout.LayoutParams(c.dp(64), c.dp(64), Gravity.CENTER) })
            }
            "nameplate" -> {
                Shop.plate(c, JSONObject().put("nameplate", key))?.let { d ->
                    stage.addView(View(c).apply { background = d }, FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, c.dp(64), Gravity.CENTER).apply { setMargins(c.dp(8), 0, c.dp(8), 0) })
                    stage.addView(c.label(15f, Color.WHITE, bold = true).apply { text = Account.name.ifEmpty { "Your name" } }, FrameLayout.LayoutParams(ViewGroup.LayoutParams.WRAP_CONTENT, ViewGroup.LayoutParams.WRAP_CONTENT, Gravity.CENTER_VERTICAL or Gravity.START).apply { marginStart = c.dp(24) })
                }
            }
            "frame" -> {
                stage.addView(View(c).apply { background = roundRect(0xFF22222A.toInt(), c.dp(8)) }, FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT).apply { setMargins(c.dp(16), c.dp(16), c.dp(16), c.dp(16)) })
                stage.addView(FrameOverlay(c).apply { show(item, variant, 28) }, FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT).apply { setMargins(c.dp(8), c.dp(8), c.dp(8), c.dp(8)) })
            }
            else -> {
                stage.addView(View(c).apply { background = roundRect(0xFF22222A.toInt(), c.dp(8)) }, FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT).apply { setMargins(c.dp(8), c.dp(8), c.dp(8), c.dp(8)) })
                stage.addView(EffectView(c).apply { show(item, variant) }, FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT))
            }
        }

        val swatches = LinearLayout(c).apply {
            gravity = Gravity.CENTER_VERTICAL
            item.variants.forEachIndexed { i, v ->
                addView(View(c).apply {
                    background = roundRect(v.color, c.dp(11)).also { if (i == index) it.setStroke(c.dp(2), Color.WHITE) }
                    setOnClickListener { chosen[item.id] = i; fill(holder, item) }
                }, LinearLayout.LayoutParams(c.dp(22), c.dp(22)).apply { marginEnd = c.dp(8) })
            }
        }
        val need = item.price - Shop.orbs
        val action = when {
            wearing -> button("Wearing", primary = false) { act("/me/equip", JSONObject().put("kind", item.kind).put("item", JSONObject.NULL)) { Shop.apply(it) } }
            owns -> button("Wear") { act("/me/equip", JSONObject().put("kind", item.kind).put("item", item.id).put("variant", variant.id)) { Shop.apply(it); c.say("${item.name} is on") } }
            need > 0 -> button("◆ ${item.price}", enabled = false) {}
            else -> button("Buy  ◆ ${item.price}") {
                (c as Activity).ask("Buy ${item.name}?", "${variant.label} for ${item.price} Orbs. You have ${Shop.orbs}.", ok = "Buy") {
                    act("/shop/buy", JSONObject().put("item", item.id).put("variant", variant.id)) { Shop.apply(it); c.say("${item.name} is yours") }
                }
            }
        }
        val card = LinearLayout(c).apply {
            orientation = LinearLayout.VERTICAL
            background = roundRect(Ui.SURFACE, c.dp(16))
            setPadding(c.dp(10), c.dp(10), c.dp(10), c.dp(12))
            clipChildren = false
            addView(stage, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, c.dp(h)))
            addView(c.label(15f, Color.WHITE, bold = true).apply { text = item.name; setPadding(c.dp(2), c.dp(10), 0, 0) })
            addView(c.label(12f, Ui.MUTED).apply { text = item.tier.replaceFirstChar { it.uppercase() } + " · " + variant.label; setPadding(c.dp(2), c.dp(2), 0, c.dp(8)) })
            addView(swatches, LinearLayout.LayoutParams(ViewGroup.LayoutParams.WRAP_CONTENT, ViewGroup.LayoutParams.WRAP_CONTENT).apply { setMargins(c.dp(2), 0, 0, c.dp(10)) })
            addView(action, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT))
        }
        holder.addView(card, FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT))
    }
}
