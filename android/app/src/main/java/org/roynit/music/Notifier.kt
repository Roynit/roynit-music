package org.roynit.music

import android.app.Activity
import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.job.JobInfo
import android.app.job.JobParameters
import android.app.job.JobScheduler
import android.app.job.JobService
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.app.Person
import android.app.RemoteInput
import android.content.BroadcastReceiver
import android.content.LocusId
import android.content.pm.ShortcutInfo
import android.content.pm.ShortcutManager
import android.graphics.drawable.Icon
import android.os.Build
import org.json.JSONObject
import android.media.AudioAttributes
import android.net.Uri
import android.os.Handler
import android.os.Looper
import java.util.Calendar
import java.util.Locale

/**
 * Notifications with one short, bright "ting" (res/raw/roy_ting.wav): new messages from friends, a good-morning line,
 * and a happy-birthday wish. While the app's process is alive it checks every half minute; when it is not, Android
 * wakes [PushJob] about every quarter of an hour. Instant push would need a Google Firebase project, which this does not use.
 */
object Notifier {
    private const val MESSAGES = "messages_v2"
    private const val GREETINGS = "greetings_v1"

    /** True while the app is on screen, and the friend whose chat is open: no ping for a message you are reading. */
    @Volatile var foreground = false
    @Volatile var chatOpenWith: String? = null
    private var looping = false
    private val main = Handler(Looper.getMainLooper())

    private fun vi() = Locale.getDefault().language == "vi"

    private fun channels(context: Context) {
        val manager = context.getSystemService(NotificationManager::class.java)
        val sound = Uri.parse("android.resource://${context.packageName}/${R.raw.roy_ting}")
        val attributes = AudioAttributes.Builder().setUsage(AudioAttributes.USAGE_NOTIFICATION).setContentType(AudioAttributes.CONTENT_TYPE_SONIFICATION).build()
        manager.deleteNotificationChannel("messages_v1")
        fun make(id: String, name: String, importance: Int) {
            if (manager.getNotificationChannel(id) != null) return
            manager.createNotificationChannel(NotificationChannel(id, name, importance).apply {
                if (id == MESSAGES && Build.VERSION.SDK_INT >= 29) setAllowBubbles(true)
                setSound(sound, attributes)
                enableVibration(true)
                vibrationPattern = longArrayOf(0, 40, 60, 40)
            })
        }
        make(MESSAGES, "Messages", NotificationManager.IMPORTANCE_HIGH)
        make(GREETINGS, "Greetings and birthdays", NotificationManager.IMPORTANCE_DEFAULT)
    }

    private fun post(context: Context, channel: String, id: Int, title: String, text: String, icon: Int, tab: Int) {
        channels(context)
        val open = PendingIntent.getActivity(
            context, id,
            Intent(context, MainActivity::class.java).putExtra("tab", tab).addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP or Intent.FLAG_ACTIVITY_CLEAR_TOP),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
        val note = Notification.Builder(context, channel).setSmallIcon(icon).setContentTitle(title).setContentText(text)
            .setStyle(Notification.BigTextStyle().bigText(text)).setColor(Ui.GREEN).setContentIntent(open).setAutoCancel(true).build()
        runCatching { context.getSystemService(NotificationManager::class.java).notify(id, note) }
    }

    /** The quick icon you set for one friend (like Messenger's): the thumbs-up beside the message box and the button on their notification. */
    fun quickIcon(context: Context, friend: String) = context.getSharedPreferences("roy_notify", Context.MODE_PRIVATE).getString("quick_$friend", null) ?: "👍"

    /** Sets it for both of you: shown at once from the local copy, and saved on the server so the friend and your other devices get it. */
    fun setQuickIcon(context: Context, friend: String, icon: String) {
        syncQuick(context, friend, icon)
        Thread { Account.call("PUT", "/messages/$friend/quick", JSONObject().put("emoji", icon)) }.start()
    }

    /** Takes what the server says (null = the default) into the local copy that the button and the notification read. */
    fun syncQuick(context: Context, friend: String, icon: String?) {
        val prefs = context.getSharedPreferences("roy_notify", Context.MODE_PRIVATE)
        if (prefs.getString("quick_$friend", null) == icon) return
        prefs.edit().apply { if (icon == null) remove("quick_$friend") else putString("quick_$friend", icon) }.apply()
    }

    /** Instant message notifications through a small always-on background service (on by default). */
    fun live(context: Context) = context.getSharedPreferences("roy_notify", Context.MODE_PRIVATE).getBoolean("live", true)

    fun setLive(context: Context, on: Boolean) {
        context.getSharedPreferences("roy_notify", Context.MODE_PRIVATE).edit().putBoolean("live", on).apply()
        if (on) LiveService.start(context) else context.stopService(Intent(context, LiveService::class.java))
    }

    /** Turn the floating chat bubbles on or off (on by default; Android's own settings can still switch them off). */
    fun bubbles(context: Context) = context.getSharedPreferences("roy_notify", Context.MODE_PRIVATE).getBoolean("bubbles", true)

    fun setBubbles(context: Context, on: Boolean) {
        context.getSharedPreferences("roy_notify", Context.MODE_PRIVATE).edit().putBoolean("bubbles", on).apply()
    }

    /** A friend's message: Reply and Like right on the notification, and (when allowed) a round floating bubble like Messenger's. */
    private fun postMessage(context: Context, friend: JSONObject, text: String) {
        channels(context)
        val id = friend.optString("id")
        val name = friend.optString("display_name").ifBlank { "New message" }
        val code = id.hashCode()
        val picture = Account.avatarUrl(id, friend.optInt("avatar_version"))?.let { Images.blocking(it, 192) }
        val icon = if (picture != null) Icon.createWithAdaptiveBitmap(picture) else Icon.createWithResource(context, R.mipmap.ic_launcher)
        val person = Person.Builder().setName(name).setKey(id).setIcon(icon).build()
        val me = Person.Builder().setName(if (vi()) "Bạn" else "You").build()
        val open = PendingIntent.getActivity(
            context, code,
            Intent(context, MainActivity::class.java).putExtra("tab", 3).addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP or Intent.FLAG_ACTIVITY_CLEAR_TOP),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
        fun receiver(action: String, mutable: Boolean) = PendingIntent.getBroadcast(
            context, code * 4 + action.length,
            Intent(context, NoteActions::class.java).setAction(action).putExtra("id", id).putExtra("name", name),
            PendingIntent.FLAG_UPDATE_CURRENT or (if (mutable) (if (Build.VERSION.SDK_INT >= 31) PendingIntent.FLAG_MUTABLE else 0) else PendingIntent.FLAG_IMMUTABLE),
        )
        val reply = Notification.Action.Builder(Icon.createWithResource(context, R.drawable.ic_reply), if (vi()) "Trả lời" else "Reply", receiver("reply", true))
            .addRemoteInput(RemoteInput.Builder("text").setLabel(if (vi()) "Trả lời $name" else "Reply to $name").build())
            .setSemanticAction(Notification.Action.SEMANTIC_ACTION_REPLY).setAllowGeneratedReplies(true).build()
        val like = Notification.Action.Builder(Icon.createWithResource(context, R.drawable.ic_heart), quickIcon(context, id), receiver("like", false)).build()
        val builder = Notification.Builder(context, MESSAGES).setSmallIcon(R.drawable.ic_send).setColor(Ui.GREEN).setContentIntent(open).setAutoCancel(true)
            .setStyle(Notification.MessagingStyle(me).addMessage(text, System.currentTimeMillis(), person))
            .setCategory(Notification.CATEGORY_MESSAGE).addAction(reply).addAction(like).setShortcutId("chat_$id")
        if (Build.VERSION.SDK_INT >= 29) builder.setLocusId(LocusId("chat_$id"))
        if (Build.VERSION.SDK_INT >= 30 && bubbles(context)) runCatching {
            val shortcuts = context.getSystemService(ShortcutManager::class.java)
            shortcuts.pushDynamicShortcut(
                ShortcutInfo.Builder(context, "chat_$id").setLongLived(true).setShortLabel(name).setIcon(icon).setPerson(person)
                    .setLocusId(LocusId("chat_$id")).setIntent(Intent(context, MainActivity::class.java).setAction(Intent.ACTION_VIEW)).build(),
            )
            val bubble = PendingIntent.getActivity(
                context, code,
                Intent(context, BubbleChatActivity::class.java).setAction("bubble_$id").putExtra("person", friend.toString()),
                PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_MUTABLE,
            )
            builder.setBubbleMetadata(Notification.BubbleMetadata.Builder(bubble, icon).setDesiredHeight(640).setSuppressNotification(false).build())
        }
        runCatching { context.getSystemService(NotificationManager::class.java).notify(code, builder.build()) }
    }

    /** A plain one-line notice that replaces a friend's notification (used when a reply from the notification could not be sent). */
    fun notice(context: Context, id: Int, title: String, text: String) = post(context, MESSAGES, id, title, text, R.drawable.ic_send, 3)

    /** Looks at friends' messages, the time of day and the birthday. Call from a worker thread. */
    fun check(context: Context) {
        Account.load(context)
        if (!Account.signedIn) return
        val prefs = context.getSharedPreferences("roy_notify", Context.MODE_PRIVATE)
        greet(context, prefs)
        val social = Account.call("GET", "/social")
        val unread = social.optJSONObject("unread")
        if (unread != null) {
            val latest = social.optJSONObject("latest")
            val friends = HashMap<String, JSONObject>()
            social.optJSONArray("friends")?.let { list ->
                for (i in 0 until list.length()) list.getJSONObject(i).let {
                    friends[it.optString("id")] = it
                    syncQuick(context, it.optString("id"), if (it.isNull("quick")) null else it.optString("quick"))
                }
            }
            val senders = unread.keys().asSequence().toSet()
            val edit = prefs.edit()
            for (sender in senders) {
                val count = unread.optInt(sender)
                if (count > prefs.getInt("unread_$sender", 0) && !(foreground && chatOpenWith == sender)) {
                    val text = latest?.optString(sender).orEmpty().ifBlank { "Sent you a message" }
                    postMessage(context, friends[sender] ?: JSONObject().put("id", sender), if (count > 1) "$text  (+${count - 1})" else text)
                }
                edit.putInt("unread_$sender", count)
            }
            // Once a friend's messages are read they leave the list; forget them so their next message rings again.
            prefs.all.keys.filter { it.startsWith("unread_") && it.removePrefix("unread_") !in senders }.forEach { edit.remove(it) }
            edit.apply()
        }
    }

    private fun today(): String = Calendar.getInstance().let { "${it.get(Calendar.YEAR)}-${it.get(Calendar.DAY_OF_YEAR)}" }

    private fun greet(context: Context, prefs: android.content.SharedPreferences) {
        val now = Calendar.getInstance()
        val name = Account.name
        val year = now.get(Calendar.YEAR)
        if (isBirthday() && prefs.getInt("birthday_push", 0) != year && now.get(Calendar.HOUR_OF_DAY) >= 7) {
            prefs.edit().putInt("birthday_push", year).apply()
            post(context, GREETINGS, 7001, birthdayTitle(name), birthdayText(), R.drawable.ic_heart, 0)
        } else if (now.get(Calendar.HOUR_OF_DAY) in 7..10 && prefs.getString("morning", "") != today()) {
            prefs.edit().putString("morning", today()).apply()
            val lines = if (vi()) listOf("Chúc buổi sáng tốt lành! Hôm nay bật một bài thật hay nhé", "Dậy rồi à? Có bài mới đang chờ bạn đó", "Ngày mới vui vẻ! Nhạc đã sẵn sàng cho bạn")
            else listOf("Good morning! Start the day with a great song", "Rise and shine, new music is waiting for you", "Have a lovely day, your music is ready")
            val comma = if (name.isNotEmpty()) ", $name" else ""
            post(context, GREETINGS, 7002, (if (vi()) "Chào buổi sáng" else "Good morning") + comma + " ☀️", lines[now.get(Calendar.DAY_OF_YEAR) % lines.size], R.drawable.ic_play, 0)
        }
    }

    fun birthdayTitle(name: String): String {
        val comma = if (name.isNotEmpty()) ", $name" else ""
        return (if (vi()) "Chúc mừng sinh nhật" else "Happy birthday") + comma + "! 🎂"
    }

    fun birthdayText(): String {
        val born = Account.profile.optString("birthday").take(4).toIntOrNull()
        val age = born?.let { Calendar.getInstance().get(Calendar.YEAR) - it }
        return if (vi()) "Chúc bạn một ngày thật vui và đầy những bài hát hay" + (if (age != null) " ở tuổi $age" else "") + "!"
        else "Wishing you a day full of great songs" + (if (age != null) " as you turn $age" else "") + "!"
    }

    /** True on the signed-in person's birthday (29 February is celebrated on 28 February in other years). */
    fun isBirthday(): Boolean {
        val day = Account.profile.optString("birthday")
        if (day.length != 10) return false
        val now = Calendar.getInstance()
        val month = day.substring(5, 7).toIntOrNull() ?: return false
        var date = day.substring(8, 10).toIntOrNull() ?: return false
        if (month == 2 && date == 29 && now.getActualMaximum(Calendar.DAY_OF_YEAR) != 366) date = 28
        return now.get(Calendar.MONTH) + 1 == month && now.get(Calendar.DAY_OF_MONTH) == date
    }

    /** The in-app wish, shown once a year when the app is open on the birthday. */
    fun wishInApp(activity: Activity) {
        val prefs = activity.getSharedPreferences("roy_notify", Context.MODE_PRIVATE)
        val year = Calendar.getInstance().get(Calendar.YEAR)
        if (!Account.signedIn || !isBirthday() || prefs.getInt("birthday_app", 0) == year) return
        prefs.edit().putInt("birthday_app", year).apply()
        Sheet.show(activity, birthdayTitle(Account.name), birthdayText(), null, listOf(Sheet.Option(if (vi()) "Cảm ơn!" else "Thank you!", R.drawable.ic_heart) {}))
    }

    /** Checks every 30 seconds while the app's process is alive, and asks Android to wake it now and then when it is not. */
    fun start(context: Context) {
        val app = context.applicationContext
        channels(app)
        if (live(app)) LiveService.start(app)
        if (!looping) {
            looping = true
            val run = object : Runnable {
                override fun run() {
                    Thread { runCatching { check(app) } }.start()
                    main.postDelayed(this, 30_000)
                }
            }
            main.postDelayed(run, 5_000)
        }
        val scheduler = app.getSystemService(JobScheduler::class.java)
        if (scheduler.getPendingJob(77) == null) {
            scheduler.schedule(JobInfo.Builder(77, ComponentName(app, PushJob::class.java)).setPeriodic(15 * 60_000L).setPersisted(true).setRequiredNetworkType(JobInfo.NETWORK_TYPE_ANY).build())
        }
    }
}

class PushJob : JobService() {
    override fun onStartJob(params: JobParameters): Boolean {
        Thread {
            runCatching { Notifier.check(applicationContext) }
            jobFinished(params, false)
        }.start()
        return true
    }

    override fun onStopJob(params: JobParameters) = false
}

/** Handles Reply (typed on the notification) and Like (sends 👍) without opening the app. */
class NoteActions : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        val id = intent.getStringExtra("id") ?: return
        val name = intent.getStringExtra("name").orEmpty()
        val text = if (intent.action == "like") Notifier.quickIcon(context, id) else RemoteInput.getResultsFromIntent(intent)?.getCharSequence("text")?.toString()?.trim().orEmpty()
        val pending = goAsync()
        val app = context.applicationContext
        Thread {
            var failed = ""
            try {
                Account.load(app)
                if (text.isNotEmpty()) failed = Account.call("POST", "/messages/$id", JSONObject().put("body", text)).optString("error")
                if (failed.isEmpty()) Account.call("GET", "/messages/$id?after=999999999999&read=1")
            } catch (e: Exception) {
                failed = e.message ?: "error"
            } finally {
                if (failed.isEmpty()) app.getSystemService(NotificationManager::class.java).cancel(id.hashCode())
                else Notifier.notice(app, id.hashCode(), name, "Message not sent. Open the app to try again.")
                pending.finish()
            }
        }.start()
    }
}
