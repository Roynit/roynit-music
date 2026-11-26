package org.roynit.music

import android.app.Activity
import android.content.Intent
import android.os.Build
import android.os.Bundle
import android.view.WindowInsets
import org.json.JSONObject

/** One friend's chat inside a floating bubble: the same conversation screen as in the app. */
class BubbleChatActivity : Activity() {
    private lateinit var chat: ChatView

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        Images.init(this)
        Account.load(this)
        Shop.load(this)
        val who = runCatching { JSONObject(intent.getStringExtra("person") ?: "") }.getOrNull()
        if (who == null) { finish(); return }
        chat = ChatView(this, { startActivity(Intent(this, MainActivity::class.java).putExtra("tab", 3).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)) }, { finish() })
        setContentView(chat)
        chat.setOnApplyWindowInsetsListener { view, insets ->
            val bars = if (Build.VERSION.SDK_INT >= 30) insets.getInsets(WindowInsets.Type.systemBars()) else null
            val keyboard = if (Build.VERSION.SDK_INT >= 30) insets.getInsets(WindowInsets.Type.ime()).bottom else 0
            view.setPadding(0, bars?.top ?: 0, 0, maxOf(bars?.bottom ?: 0, keyboard))
            insets
        }
        chat.open(who)
    }

    override fun onResume() { super.onResume(); Notifier.foreground = true }
    override fun onPause() { super.onPause(); Notifier.foreground = false }

    @Deprecated("Deprecated in Java")
    override fun onBackPressed() = chat.back()
}
