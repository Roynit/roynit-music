package org.roynit.music

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.net.Uri
import android.os.Build
import android.os.IBinder
import android.os.PowerManager
import android.provider.Settings

/**
 * Keeps one quiet connection to the server open (a long poll that answers the moment a friend's message arrives), so
 * a notification shows at once even while the app is closed. Android requires a small status-bar notification for that.
 */
class LiveService : Service() {
    @Volatile private var running = false

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        val manager = getSystemService(NotificationManager::class.java)
        if (manager.getNotificationChannel(CHANNEL) == null) {
            manager.createNotificationChannel(NotificationChannel(CHANNEL, "Instant messages", NotificationManager.IMPORTANCE_MIN).apply { setShowBadge(false) })
        }
        val open = PendingIntent.getActivity(this, 0, Intent(this, MainActivity::class.java).putExtra("tab", 3), PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT)
        val note = Notification.Builder(this, CHANNEL).setSmallIcon(R.drawable.ic_send).setContentTitle("Roy Music")
            .setContentText("Ready for new messages").setContentIntent(open).setOngoing(true).setCategory(Notification.CATEGORY_SERVICE).build()
        if (Build.VERSION.SDK_INT >= 34) startForeground(ID, note, ServiceInfo.FOREGROUND_SERVICE_TYPE_SPECIAL_USE) else startForeground(ID, note)
        if (!running) {
            running = true
            Thread { loop() }.start()
        }
        return START_STICKY
    }

    private fun loop() {
        val app = applicationContext
        var after = -1L
        while (running) {
            Account.load(app)
            if (!Account.signedIn || !Notifier.live(app)) {
                // Signed out or switched off: nothing to wait for.
                running = false
                stopSelf()
                return
            }
            val reply = Account.call("GET", "/inbox/wait?after=$after")
            if (reply.has("error")) {
                Thread.sleep(15_000)
                continue
            }
            val id = reply.optLong("id")
            if (after >= 0 && id > after) runCatching { Notifier.check(app) }
            after = id
        }
    }

    override fun onDestroy() {
        running = false
        super.onDestroy()
    }

    companion object {
        private const val CHANNEL = "live_v1"
        private const val ID = 7100

        fun start(context: Context) {
            runCatching { context.startForegroundService(Intent(context, LiveService::class.java)) }
        }

        /** Without this, Android's Doze mode can cut the connection when the screen has been off for a while. */
        fun askBatteryExemption(context: Context) {
            val power = context.getSystemService(PowerManager::class.java)
            if (power.isIgnoringBatteryOptimizations(context.packageName)) return
            runCatching {
                context.startActivity(Intent(Settings.ACTION_REQUEST_IGNORE_BATTERY_OPTIMIZATIONS, Uri.parse("package:${context.packageName}")).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
            }
        }
    }
}

/** Starts the connection again after the phone restarts or the app is updated. */
class BootReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        Account.load(context)
        if (Account.signedIn && Notifier.live(context)) LiveService.start(context)
    }
}
