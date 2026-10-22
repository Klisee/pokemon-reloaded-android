package com.klisee.reloaded.discord

import android.util.Log
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.Response
import okhttp3.WebSocket
import okhttp3.WebSocketListener
import org.json.JSONObject
import java.util.Timer
import java.util.TimerTask
import java.util.concurrent.TimeUnit

/**
 * Conexión WebSocket con el Gateway de Discord.
 *
 * Se identifica (op 2) con el token de sesión de la cuenta y después publica la actividad. Mantiene
 * la sesión con heartbeats y reconecta cuando Discord lo pide.
 */
class DiscordGateway(
    private val deviceName: String,
    private val onReady: () -> Unit,
    private val onClosed: () -> Unit,
) {
    private companion object {
        const val TAG = "DiscordRpc"
        const val GATEWAY_URL = "wss://gateway.discord.gg/?v=10&encoding=json"

        const val OP_DISPATCH = 0
        const val OP_HEARTBEAT = 1
        const val OP_IDENTIFY = 2
        const val OP_RECONNECT = 7
        const val OP_INVALID_SESSION = 9
        const val OP_HELLO = 10
        const val OP_HEARTBEAT_ACK = 11
    }

    private val client = OkHttpClient.Builder()
        .connectTimeout(15, TimeUnit.SECONDS)
        .readTimeout(0, TimeUnit.MILLISECONDS)
        .pingInterval(0, TimeUnit.MILLISECONDS)
        .build()

    @Volatile private var socket: WebSocket? = null
    @Volatile private var token: String? = null
    @Volatile private var seq: Int = 0
    @Volatile private var isOpen = false
    @Volatile private var ready = false
    private var heartbeat: Timer? = null

    // A failed socket must not be retried in a tight loop: without this, a DNS outage produced
    // dozens of attempts per second and kept the presence from ever being published.
    private val reconnectExecutor = java.util.concurrent.Executors.newSingleThreadScheduledExecutor { runnable ->
        Thread(runnable, "discord-reconnect").apply { isDaemon = true }
    }
    private val reconnecting = java.util.concurrent.atomic.AtomicBoolean(false)
    @Volatile private var reconnectAttempts = 0
    @Volatile private var stopped = false

    fun setToken(value: String) {
        token = value
    }

    fun connect() {
        stopped = false
        teardownSocket()
        isOpen = false
        ready = false
        socket = client.newWebSocket(Request.Builder().url(GATEWAY_URL).build(), Listener())
    }

    val isReady: Boolean get() = ready

    fun updatePresence(frame: String) {
        val ws = socket
        if (ws == null || !isOpen) {
            Log.w(TAG, "presence skipped: gateway not open")
            return
        }
        ws.send(frame)
    }

    fun close() {
        stopped = true
        reconnecting.set(false)
        reconnectExecutor.shutdownNow()
        heartbeat?.cancel()
        heartbeat = null
        ready = false
        isOpen = false
        val ws = socket
        socket = null
        if (ws != null) {
            runCatching { ws.send(DiscordPresence.buildClearActivity()) }
            runCatching { ws.close(1000, "app closed") }
        }
    }

    private fun teardownSocket() {
        heartbeat?.cancel()
        heartbeat = null
        runCatching { socket?.cancel() }
        socket = null
    }

    private fun send(frame: String) {
        val ws = socket ?: return
        if (!ws.send(frame)) Log.w(TAG, "gateway send queue full")
    }

    private fun identify() {
        val value = token ?: return
        val properties = JSONObject()
            .put("os", "android")
            .put("browser", "Discord Android")
            .put("device", deviceName)
        val d = JSONObject()
            .put("token", value)
            .put("intents", 0)
            .put("properties", properties)
            .put("compress", false)
        send(JSONObject().put("op", OP_IDENTIFY).put("d", d).toString())
    }

    private fun startHeartbeat(intervalMs: Long) {
        heartbeat?.cancel()
        heartbeat = Timer("discord-heartbeat", true).apply {
            scheduleAtFixedRate(
                object : TimerTask() {
                    override fun run() = sendHeartbeat()
                },
                intervalMs,
                intervalMs,
            )
        }
    }

    private fun sendHeartbeat() {
        val d = if (seq > 0) seq else JSONObject.NULL
        send(JSONObject().put("op", OP_HEARTBEAT).put("d", d).toString())
    }

    private fun handleFrame(text: String) {
        val json = runCatching { JSONObject(text) }.getOrNull() ?: return
        if (json.has("s") && !json.isNull("s")) {
            val incoming = json.optInt("s", 0)
            if (incoming > 0) seq = incoming
        }

        when (json.optInt("op", -1)) {
            OP_HELLO -> {
                val interval = json.optJSONObject("d")?.optLong("heartbeat_interval", 41_250L) ?: 41_250L
                startHeartbeat(interval)
                identify()
            }
            OP_HEARTBEAT -> sendHeartbeat()
            OP_HEARTBEAT_ACK -> Unit
            OP_RECONNECT -> {
                Log.i(TAG, "gateway asked to reconnect")
                reconnect()
            }
            OP_INVALID_SESSION -> {
                Log.w(TAG, "invalid session, re-identifying")
                ready = false
                identify()
            }
            OP_DISPATCH -> {
                when (json.optString("t", "")) {
                    "READY", "RESUMED" -> {
                        reconnectAttempts = 0
                        if (!ready) {
                            ready = true
                            Log.i(TAG, "gateway ready")
                            onReady()
                        }
                    }
                }
            }
        }
    }

    private fun reconnect() {
        if (stopped) return
        // One pending attempt at a time, with a growing delay: 2, 4, 8 ... up to 60 seconds.
        if (!reconnecting.compareAndSet(false, true)) return
        val delay = (1L shl reconnectAttempts.coerceAtMost(5)).coerceIn(2L, 60L)
        reconnectAttempts++
        Log.i(TAG, "reconectando en ${delay}s (intento $reconnectAttempts)")
        reconnectExecutor.schedule({
            reconnecting.set(false)
            if (!stopped) {
                runCatching { connect() }.onFailure {
                    Log.w(TAG, "reconnect failed", it)
                    onClosed()
                }
            }
        }, delay, TimeUnit.SECONDS)
    }

    private inner class Listener : WebSocketListener() {
        override fun onOpen(webSocket: WebSocket, response: Response) {
            isOpen = true
            Log.i(TAG, "gateway socket open")
        }

        override fun onMessage(webSocket: WebSocket, text: String) {
            runCatching { handleFrame(text) }.onFailure { Log.w(TAG, "bad gateway frame", it) }
        }

        override fun onClosing(webSocket: WebSocket, code: Int, reason: String) {
            webSocket.close(1000, null)
        }

        override fun onClosed(webSocket: WebSocket, code: Int, reason: String) {
            Log.i(TAG, "gateway closed ($code $reason)")
            isOpen = false
            ready = false
            onClosed()
        }

        override fun onFailure(webSocket: WebSocket, t: Throwable, response: Response?) {
            Log.w(TAG, "gateway failure: ${t.message}")
            isOpen = false
            ready = false
            reconnect()
        }
    }
}
