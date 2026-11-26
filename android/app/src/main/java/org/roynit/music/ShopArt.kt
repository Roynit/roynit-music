package org.roynit.music

import android.animation.Animator
import android.animation.ObjectAnimator
import android.animation.ValueAnimator
import android.content.Context
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.Paint
import android.graphics.PorterDuff
import android.graphics.PorterDuffColorFilter
import android.graphics.PorterDuffXfermode
import android.graphics.Rect
import android.graphics.RectF
import android.util.LruCache
import android.view.Gravity
import android.view.View
import android.view.animation.LinearInterpolator
import android.widget.FrameLayout
import android.widget.ImageView
import android.widget.LinearLayout
import org.json.JSONObject
import java.util.concurrent.Executors
import kotlin.math.PI
import kotlin.math.sin

/**
 * The Orbs shop's art and what each person wears: avatar decorations, nameplates, profile frames and profile effects.
 * The pictures ship inside the app (assets/shop, drawn by scripts/shop_assets.py); the server only knows ids and prices.
 */
object Shop {
    class Variant(val id: String, val label: String, val color: Int)
    class Item(
        val id: String, val kind: String, val name: String, val tier: String, var price: Int, val anim: String,
        val folder: String, val stem: String, val variants: List<Variant>, val sprites: List<String>, val emitter: JSONObject?, val particles: JSONObject? = null, val fraction: Float? = null, val layers: org.json.JSONArray? = null,
    )

    var items = emptyList<Item>()
        private set
    private var fraction = 0.625f
    var orbs = 0
    var owned = HashSet<String>()      // "item:variant"
    var equipped = JSONObject()        // kind -> "item:variant"

    private val pool = Executors.newFixedThreadPool(2)
    private val cache = object : LruCache<String, Bitmap>(8 * 1024 * 1024) {
        override fun sizeOf(key: String, value: Bitmap) = value.byteCount
    }

    fun load(context: Context) {
        if (items.isNotEmpty()) return
        val root = JSONObject(context.assets.open("shop/catalog.json").bufferedReader().use { it.readText() })
        fraction = root.optDouble("avatar_fraction", 0.625).toFloat()
        val list = root.getJSONArray("items")
        items = (0 until list.length()).map { i ->
            val o = list.getJSONObject(i)
            val vs = o.getJSONArray("variants")
            val sprites = o.optJSONArray("sprites")
            Item(
                o.getString("id"), o.getString("kind"), o.getString("name"), o.optString("tier"), o.getInt("price"), o.optString("anim", "none"),
                o.optString("folder"), o.optString("stem"),
                (0 until vs.length()).map { v -> vs.getJSONObject(v).let { Variant(it.getString("id"), it.optString("label"), Color.parseColor(it.optString("color", "#ffffff"))) } },
                (0 until (sprites?.length() ?: 0)).map { sprites!!.getString(it) }, o.optJSONObject("emitter"), o.optJSONObject("particles"), if (o.has("fraction")) o.getDouble("fraction").toFloat() else null, o.optJSONArray("layers"),
            )
        }
    }

    fun find(id: String) = items.firstOrNull { it.id == id }
    fun path(item: Item, variant: Variant) = "${item.folder}/${item.stem}_${variant.id}.webp"
    val avatarFraction get() = fraction

    /** What someone wears of one kind, from their `equipped` object ("item:variant" strings). */
    fun worn(equipped: JSONObject?, kind: String): Pair<Item, Variant>? {
        val text = equipped?.optString(kind).orEmpty()
        if (text.isEmpty()) return null
        val item = find(text.substringBefore(':')) ?: return null
        val variant = item.variants.firstOrNull { it.id == text.substringAfter(':') } ?: return null
        return item to variant
    }

    /** Takes a reply of GET /shop, POST /shop/buy or POST /me/equip. */
    fun apply(reply: JSONObject) {
        orbs = reply.optInt("orbs", orbs)
        reply.optJSONObject("equipped")?.let { equipped = it }
        reply.optJSONArray("owned")?.let { list -> owned = HashSet((0 until list.length()).map { list.getJSONObject(it).let { o -> o.optString("item") + ":" + o.optString("variant") } }) }
        reply.optJSONArray("items")?.let { list -> for (i in 0 until list.length()) list.getJSONObject(i).let { o -> find(o.optString("id"))?.price = o.optInt("price") } }
        if (Account.signedIn) runCatching { Account.profile.put("equipped", equipped).put("orbs", orbs) }
    }

    fun bitmap(context: Context, path: String, px: Int): Bitmap? {
        val key = "$path@$px"
        cache.get(key)?.let { return it }
        return runCatching {
            val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
            context.assets.open("shop/$path").use { BitmapFactory.decodeStream(it, null, bounds) }
            var sample = 1
            while (bounds.outWidth / (sample * 2) >= px) sample *= 2
            context.assets.open("shop/$path").use { BitmapFactory.decodeStream(it, null, BitmapFactory.Options().apply { inSampleSize = sample }) }
        }.getOrNull()?.also { cache.put(key, it) }
    }

    /** Loads a picture off the main thread and puts it in the view, unless the view moved on to something else. */
    fun into(view: ImageView, path: String, px: Int) {
        view.tag = path
        cache.get("$path@$px")?.let { view.setImageBitmap(it); return }
        view.setImageDrawable(null)
        val context = view.context.applicationContext
        pool.execute {
            val bitmap = bitmap(context, path, px)
            view.post { if (view.tag == path && bitmap != null) view.setImageBitmap(bitmap) }
        }
    }

    /** The avatar with its decoration drawn over it, or the avatar itself when there is none. */
    fun decorate(c: Context, avatar: View, sizeDp: Int, equipped: JSONObject?): View {
        val (item, variant) = worn(equipped, "decoration") ?: return avatar
        val big = (sizeDp / (item.fraction ?: fraction)).toInt()
        // The wrapper is as big as the decoration and sits over its neighbours with negative margins, so the layout
        // around the avatar stays exactly as it was and nothing has to draw outside its own bounds.
        val wrap = FrameLayout(c).apply { clipChildren = false; clipToPadding = false }
        wrap.addView(avatar, FrameLayout.LayoutParams(c.dp(sizeDp), c.dp(sizeDp), Gravity.CENTER))
        val art: View = if (item.layers != null) LayeredDeco(c, bitmap(c, path(item, variant), c.dp(big)), item.layers) else DecoView(c, item.anim).also { into(it, path(item, variant), c.dp(big)) }
        wrap.addView(art, FrameLayout.LayoutParams(c.dp(big), c.dp(big), Gravity.CENTER))
        if (item.particles != null) wrap.addView(RingSparkles(c).apply { show(item, variant) }, FrameLayout.LayoutParams(c.dp(big), c.dp(big), Gravity.CENTER))
        // Containers clip what sticks out of them (and out of their padding) unless told not to; tell the few above this one.
        wrap.addOnAttachStateChangeListener(object : View.OnAttachStateChangeListener {
            override fun onViewAttachedToWindow(v: View) {
                var p = v.parent
                var levels = 0
                while (p is android.view.ViewGroup && levels < 5) {
                    p.clipChildren = false
                    p.clipToPadding = false
                    p = p.parent
                    levels++
                }
            }
            override fun onViewDetachedFromWindow(v: View) {}
        })
        val over = c.dp(big) - c.dp(sizeDp)
        wrap.layoutParams = LinearLayout.LayoutParams(c.dp(big), c.dp(big)).apply { setMargins(-over / 2, -over / 2, -over / 2, -over / 2) }
        return wrap
    }

    /** A nameplate drawn behind a row, filling it like a cover. */
    fun plate(c: Context, equipped: JSONObject?): android.graphics.drawable.Drawable? {
        val (item, variant) = worn(equipped, "nameplate") ?: return null
        val bitmap = bitmap(c, path(item, variant), c.dp(360)) ?: return null
        return RoundDrawable(bitmap, c.dp(14).toFloat())
    }
}

/** A decoration. It spins, pulses or floats while it is on screen, and holds still when it is not. */
class DecoView(context: Context, private val anim: String) : ImageView(context) {
    private var animator: Animator? = null

    init {
        scaleType = ScaleType.FIT_XY
        if (anim != "none") setLayerType(LAYER_TYPE_HARDWARE, null)
    }

    override fun onVisibilityAggregated(isVisible: Boolean) {
        super.onVisibilityAggregated(isVisible)
        animator?.cancel()
        animator = null
        if (!isVisible) return
        animator = when (anim) {
            "spin" -> ObjectAnimator.ofFloat(this, ROTATION, 0f, 360f).apply { duration = 26000; interpolator = LinearInterpolator(); repeatCount = ValueAnimator.INFINITE }
            "pulse" -> ObjectAnimator.ofFloat(this, ALPHA, 0.62f, 1f).apply { duration = 1700; repeatMode = ValueAnimator.REVERSE; repeatCount = ValueAnimator.INFINITE }
            "float" -> ObjectAnimator.ofFloat(this, TRANSLATION_Y, 0f, -context.dp(3).toFloat()).apply { duration = 1900; repeatMode = ValueAnimator.REVERSE; repeatCount = ValueAnimator.INFINITE }
            else -> null
        }
        animator?.start()
    }
}

/** A profile frame: a nine-slice picture whose corners keep their size while the edges stretch. Touches pass through. */
class FrameOverlay(context: Context) : View(context) {
    private var bitmap: Bitmap? = null
    private val paint = Paint(Paint.FILTER_BITMAP_FLAG)
    private var edge = 0.28f
    private var cornerDp = 44

    init { isClickable = false; isFocusable = false }

    fun show(item: Shop.Item?, variant: Shop.Variant?, corner: Int = 44) {
        cornerDp = corner
        bitmap = if (item != null && variant != null) Shop.bitmap(context, Shop.path(item, variant), 256) else null
        visibility = if (bitmap == null) GONE else VISIBLE
        invalidate()
    }

    override fun onDraw(canvas: Canvas) {
        val b = bitmap ?: return
        val e = (b.width * edge).toInt()
        val d = context.dp(cornerDp)
        val w = width
        val h = height
        val xs = intArrayOf(0, e, b.width - e, b.width)
        val xd = intArrayOf(0, d, w - d, w)
        val ys = intArrayOf(0, e, b.height - e, b.height)
        val yd = intArrayOf(0, d, h - d, h)
        for (row in 0..2) for (col in 0..2) {
            if (col == 1 && row == 1) continue
            canvas.drawBitmap(b, Rect(xs[col], ys[row], xs[col + 1], ys[row + 1]), Rect(xd[col], yd[row], xd[col + 1], yd[row + 1]), paint)
        }
    }
}

/** A profile effect: a few sprites drifting over the page, moved by the catalog's numbers. Touches pass through. */
class EffectView(context: Context) : View(context) {
    private var sprites = emptyList<Bitmap>()
    private var emitter: JSONObject? = null
    private var tint = Color.WHITE
    private var started = 0L
    private val paint = Paint(Paint.FILTER_BITMAP_FLAG or Paint.ANTI_ALIAS_FLAG)
    private val src = Rect()
    private val dst = RectF()
    private var seeds = DoubleArray(0)

    init { isClickable = false; isFocusable = false }

    fun show(item: Shop.Item?, variant: Shop.Variant?) {
        val e = item?.emitter
        if (item == null || variant == null || e == null) { emitter = null; visibility = GONE; return }
        emitter = e
        tint = variant.color
        sprites = item.sprites.mapNotNull { Shop.bitmap(context, "sprites/$it.webp", 64) }
        val count = e.optInt("count", 12)
        val r = java.util.Random(item.id.hashCode().toLong())
        seeds = DoubleArray(count * 6) { r.nextDouble() }
        started = System.nanoTime()
        paint.xfermode = if (e.optString("blend") == "add") PorterDuffXfermode(PorterDuff.Mode.SCREEN) else null
        paint.colorFilter = if (e.optBoolean("tint")) PorterDuffColorFilter(tint, PorterDuff.Mode.SRC_ATOP) else null
        visibility = if (sprites.isEmpty()) GONE else VISIBLE
        invalidate()
    }

    override fun onVisibilityAggregated(isVisible: Boolean) {
        super.onVisibilityAggregated(isVisible)
        if (isVisible) postInvalidateOnAnimation()
    }

    private fun range(e: JSONObject, key: String, t: Double): Double {
        val a = e.optJSONArray(key) ?: return 0.0
        return a.optDouble(0) + (a.optDouble(1) - a.optDouble(0)) * t
    }

    override fun onDraw(canvas: Canvas) {
        val e = emitter ?: return
        val time = (System.nanoTime() - started) / 1e9
        val count = e.optInt("count", 12)
        val fade = e.optDouble("fade", 0.15)
        val start = e.optString("start")
        for (i in 0 until count) {
            val s = i * 6
            val size = range(e, "size", seeds[s]) * width
            val vy = range(e, "vy", seeds[s + 1])
            val sway = range(e, "sway", seeds[s + 2]) * width
            val spin = range(e, "spin", seeds[s + 3])
            val x0 = seeds[s + 4] * width
            val phase = seeds[s + 5]
            var alpha = 1.0
            val y: Double
            if (start == "anywhere" || vy == 0.0) {
                y = (0.05 + 0.9 * seeds[s + 1]) * height
                e.optJSONArray("twinkle")?.let { alpha = 0.15 + 0.85 * (0.5 + 0.5 * sin(time * (it.optDouble(0) + (it.optDouble(1) - it.optDouble(0)) * seeds[s + 2]) + phase * 2 * PI)) }
            } else {
                // One lap down (or up) the page, then round again; it fades in at the start and out at the end.
                val lap = 1.0 + 2 * 0.08
                val progress = ((phase + time * Math.abs(vy) / lap) % 1.0)
                y = (if (vy > 0) progress * lap - 0.08 else (1 - progress) * lap - 0.08) * height
                alpha = (progress / fade).coerceIn(0.0, 1.0) * ((1 - progress) / fade).coerceIn(0.0, 1.0)
            }
            val x = x0 + sin(time * (0.6 + seeds[s + 2]) + phase * 2 * PI) * sway
            val bitmap = sprites[i % sprites.size]
            paint.alpha = (alpha * 255).toInt().coerceIn(0, 255)
            val save = canvas.save()
            canvas.rotate((spin * time * 57.3 + phase * 360).toFloat(), x.toFloat(), y.toFloat())
            src.set(0, 0, bitmap.width, bitmap.height)
            dst.set((x - size / 2).toFloat(), (y - size / 2).toFloat(), (x + size / 2).toFloat(), (y + size / 2).toFloat())
            canvas.drawBitmap(bitmap, src, dst, paint)
            canvas.restoreToCount(save)
        }
        if (isShown) postInvalidateOnAnimation()
    }
}


/** Sparkles, embers or bubbles drifting in the band around an avatar, drawn over its decoration. Touches pass through. */
class RingSparkles(context: Context) : View(context) {
    private var emitter: JSONObject? = null
    private var sprites = emptyList<Bitmap>()
    private var seeds = DoubleArray(0)
    private var started = 0L
    private val paint = Paint(Paint.FILTER_BITMAP_FLAG or Paint.ANTI_ALIAS_FLAG)
    private val src = Rect()
    private val dst = RectF()

    init { isClickable = false; isFocusable = false }

    fun show(item: Shop.Item, variant: Shop.Variant) {
        val e = item.particles ?: return
        emitter = e
        val names = e.optJSONArray("sprites")
        sprites = (0 until (names?.length() ?: 0)).mapNotNull { Shop.bitmap(context, "sprites/" + names!!.getString(it) + ".webp", 64) }
        val r = java.util.Random(item.id.hashCode().toLong())
        seeds = DoubleArray(e.optInt("count", 12) * 5) { r.nextDouble() }
        started = System.nanoTime()
        paint.xfermode = if (e.optString("blend") == "add") PorterDuffXfermode(PorterDuff.Mode.SCREEN) else null
        paint.colorFilter = if (e.optBoolean("tint")) PorterDuffColorFilter(variant.color, PorterDuff.Mode.SRC_ATOP) else null
    }

    override fun onVisibilityAggregated(isVisible: Boolean) {
        super.onVisibilityAggregated(isVisible)
        if (isVisible) postInvalidateOnAnimation()
    }

    private fun span(e: JSONObject, key: String, t: Double): Double {
        val a = e.optJSONArray(key) ?: return 0.0
        return a.optDouble(0) + (a.optDouble(1) - a.optDouble(0)) * t
    }

    override fun onDraw(canvas: Canvas) {
        val e = emitter ?: return
        if (sprites.isEmpty()) return
        val time = (System.nanoTime() - started) / 1e9
        val cx = width / 2f
        val cy = height / 2f
        val rise = e.optDouble("rise", 0.0)
        val speed = e.optDouble("speed", 0.2)
        for (i in 0 until e.optInt("count", 12)) {
            val s = i * 5
            val angle = seeds[s] * 2 * PI
            val rho = 0.6 + 0.36 * seeds[s + 1]
            val size = span(e, "size", seeds[s + 2]) * width
            val phase = seeds[s + 3]
            var alpha = 1.0
            var lift = 0.0
            if (rise != 0.0) {
                val p = (phase + time * speed) % 1.0
                lift = -p * rise * width
                alpha = sin(PI * p)
            }
            if (e.has("twinkle")) alpha = 0.12 + 0.88 * (0.5 + 0.5 * sin(time * span(e, "twinkle", seeds[s + 4]) + phase * 2 * PI))
            val x = cx + Math.cos(angle) * rho * (width / 2f)
            val y = cy + Math.sin(angle) * rho * (height / 2f) + lift
            val bitmap = sprites[i % sprites.size]
            paint.alpha = (alpha * 255).toInt().coerceIn(0, 255)
            src.set(0, 0, bitmap.width, bitmap.height)
            dst.set((x - size / 2).toFloat(), (y - size / 2).toFloat(), (x + size / 2).toFloat(), (y + size / 2).toFloat())
            canvas.drawBitmap(bitmap, src, dst, paint)
        }
        if (isShown) postInvalidateOnAnimation()
    }
}

/**
 * A decoration whose parts move on their own: each layer is a slice of the same picture that sways, bobs or flaps
 * about a pivot (ears twitching, paws bobbing, wings beating). Drawn in one pass, and only while on screen.
 */
class LayeredDeco(context: Context, private val bitmap: Bitmap?, private val layers: org.json.JSONArray) : View(context) {
    private val paint = Paint(Paint.FILTER_BITMAP_FLAG or Paint.ANTI_ALIAS_FLAG)
    private val started = System.nanoTime()
    private val src = Rect()
    private val dst = Rect()

    override fun onVisibilityAggregated(isVisible: Boolean) {
        super.onVisibilityAggregated(isVisible)
        if (isVisible) postInvalidateOnAnimation()
    }

    override fun onDraw(canvas: Canvas) {
        val b = bitmap ?: return
        val time = (System.nanoTime() - started) / 1e9
        val w = width.toFloat()
        val h = height.toFloat()
        for (i in 0 until layers.length()) {
            val l = layers.getJSONObject(i)
            val x0 = l.getJSONArray("x").getDouble(0).toFloat()
            val x1 = l.getJSONArray("x").getDouble(1).toFloat()
            val y0 = l.getJSONArray("y").getDouble(0).toFloat()
            val y1 = l.getJSONArray("y").getDouble(1).toFloat()
            val period = l.optDouble("period", 3.0)
            val phase = l.optDouble("phase", 0.0)
            val turn = l.optDouble("sign", 1.0) * l.optDouble("rotate", 0.0) * sin(2 * PI * (time / period + phase))
            val lift = l.optDouble("bob", 0.0) * w * sin(2 * PI * time / l.optDouble("bob_period", 2.6))
            val pivot = l.getJSONArray("pivot")
            val save = canvas.save()
            canvas.clipRect(x0 * w, y0 * h, x1 * w, y1 * h)
            canvas.translate(0f, lift.toFloat())
            canvas.rotate(turn.toFloat(), pivot.getDouble(0).toFloat() * w, pivot.getDouble(1).toFloat() * h)
            src.set(0, 0, b.width, b.height)
            dst.set(0, 0, width, height)
            canvas.drawBitmap(b, src, dst, paint)
            canvas.restoreToCount(save)
        }
        if (isShown) postInvalidateOnAnimation()
    }
}
