package dev.dropbridge.app

import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.net.wifi.WifiManager
import android.os.IBinder
import android.os.PowerManager
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat
import androidx.core.app.ServiceCompat
import uniffi.dropbridge_ffi.humanBytes

/**
 * Keeps the process, CPU and Wi-Fi radio awake while any transfer is active, so transfers
 * survive the screen turning off. Stops itself when nothing is running.
 */
class TransferService : Service() {
    private var wifi: WifiManager.WifiLock? = null
    private var wake: PowerManager.WakeLock? = null

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onCreate() {
        super.onCreate()
        @Suppress("DEPRECATION") // LOW_LATENCY (API 29+) keeps the radio out of power save during bursts
        wifi = (getSystemService(WIFI_SERVICE) as WifiManager).createWifiLock(WifiManager.WIFI_MODE_FULL_LOW_LATENCY, "dropbridge").apply { acquire() }
        wake = (getSystemService(POWER_SERVICE) as PowerManager).newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "dropbridge:transfer").apply { acquire(6 * 60 * 60 * 1000L) }
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        try {
            ServiceCompat.startForeground(this, 1, notification(this), ServiceInfo.FOREGROUND_SERVICE_TYPE_DATA_SYNC)
        } catch (e: Exception) {
            stopSelf() // the OS refused (e.g. started from background); transfer continues while the app is open
        }
        if (active() == 0) stopSelf()
        return START_NOT_STICKY
    }

    override fun onDestroy() {
        wifi?.release()
        wake?.takeIf { it.isHeld }?.release()
        super.onDestroy()
    }

    companion object {
        private var running = false

        fun active() = Core.transfers.values.count { it.status != "done" }

        /** Called on every transfer state change. */
        fun sync(ctx: Context) {
            val n = active()
            if (n > 0 && !running) {
                running = true
                try {
                    ctx.startForegroundService(Intent(ctx, TransferService::class.java))
                } catch (_: Exception) {
                    running = false
                }
            } else if (n == 0 && running) {
                running = false
                ctx.stopService(Intent(ctx, TransferService::class.java))
            } else if (n > 0 && System.currentTimeMillis() - lastNote > 1000) {
                try {
                    NotificationManagerCompat.from(ctx).notify(1, notification(ctx))
                } catch (_: SecurityException) {
                }
            }
        }

        private var lastNote = 0L

        fun notification(ctx: Context): android.app.Notification {
            val live = Core.transfers.values.filter { it.status != "done" }
            val done = live.sumOf { it.done }
            val total = live.sumOf { it.total }.coerceAtLeast(1)
            val speed = live.sumOf { it.speed }
            val pi = PendingIntent.getActivity(ctx, 0, Intent(ctx, MainActivity::class.java), PendingIntent.FLAG_IMMUTABLE)
            lastNote = System.currentTimeMillis()
            return NotificationCompat.Builder(ctx, App.CH_TRANSFER)
                .setSmallIcon(android.R.drawable.stat_sys_upload)
                .setContentTitle(if (live.size == 1) "${if (live[0].outgoing) "Sending to" else "Receiving from"} ${live[0].peer}" else "${live.size} transfers")
                .setContentText("${humanBytes(done.toULong())} of ${humanBytes(total.toULong())} · ${humanBytes(speed.toULong())}/s")
                .setProgress(1000, (done * 1000 / total).toInt(), total <= 1)
                .setOngoing(true).setOnlyAlertOnce(true).setContentIntent(pi)
                .build()
        }
    }
}
