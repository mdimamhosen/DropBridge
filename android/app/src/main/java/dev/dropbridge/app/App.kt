package dev.dropbridge.app

import android.app.Application
import android.app.NotificationChannel
import android.app.NotificationManager

class App : Application() {
    override fun onCreate() {
        super.onCreate()
        val nm = getSystemService(NotificationManager::class.java)
        nm.createNotificationChannel(NotificationChannel(CH_TRANSFER, "Transfers", NotificationManager.IMPORTANCE_LOW))
        nm.createNotificationChannel(NotificationChannel(CH_INCOMING, "Incoming requests", NotificationManager.IMPORTANCE_HIGH))
        Core.start(this)
    }

    companion object {
        const val CH_TRANSFER = "transfer"
        const val CH_INCOMING = "incoming"
    }
}
