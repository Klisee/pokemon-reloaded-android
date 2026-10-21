package com.klisee.reloaded.discord

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.os.IBinder
import androidx.core.app.NotificationCompat
import com.klisee.reloaded.MainActivity
import com.klisee.reloaded.R

/**
 * Mantiene viva la conexión con Discord mientras el juego corre y muestra la notificación fija.
 *
 * La actividad del juego la arranca al empezar la sesión y la para al salir; la conexión vive en
 * [DiscordRpcManager].
 */
class DiscordRpcService : Service() {

    override fun onCreate() {
        super.onCreate()
        startForeground(NOTIFICATION_ID, buildNotification())
        DiscordRpcManager.init(applicationContext)
        DiscordRpcManager.startIfEnabled()
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int = START_STICKY

    override fun onDestroy() {
        DiscordRpcManager.disconnect()
        super.onDestroy()
    }

    override fun onBind(intent: Intent?): IBinder? = null

    private fun buildNotification(): Notification {
        val manager = getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
        if (manager.getNotificationChannel(CHANNEL_ID) == null) {
            manager.createNotificationChannel(
                NotificationChannel(
                    CHANNEL_ID,
                    getString(R.string.app_name),
                    NotificationManager.IMPORTANCE_LOW,
                ).apply { description = "Muestra que estás jugando a Pokémon Reloaded en Discord" },
            )
        }

        val open = PendingIntent.getActivity(
            this,
            0,
            Intent(this, MainActivity::class.java),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )

        return NotificationCompat.Builder(this, CHANNEL_ID)
            .setSmallIcon(R.drawable.ic_hero_ball)
            .setContentTitle(getString(R.string.app_name))
            .setContentText("Compartiendo tu partida en Discord")
            .setContentIntent(open)
            .setOngoing(true)
            .setShowWhen(false)
            .build()
    }

    companion object {
        private const val CHANNEL_ID = "discord_rpc"
        private const val NOTIFICATION_ID = 4711

        fun start(context: Context) {
            context.startForegroundService(Intent(context, DiscordRpcService::class.java))
        }

        fun stop(context: Context) {
            context.stopService(Intent(context, DiscordRpcService::class.java))
        }
    }
}
