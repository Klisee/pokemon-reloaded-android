package com.klisee.reloaded.discord

import android.content.Context
import android.content.SharedPreferences
import android.util.Log
import org.json.JSONArray
import org.json.JSONObject
import java.net.HttpURLConnection
import java.net.URL

/**
 * Registers a public image URL with the Discord application and returns the `mp:` path the Rich
 * Presence understands. This is how the character heads are shown without uploading hundreds of
 * assets to the Developer Portal (which is capped at 300): the URL is proxied by Discord and the
 * result is cached, so each head is registered once.
 */
object DiscordExternalAssets {
    private const val TAG = "DiscordRpc"
    private const val PREFS = "discord_external_assets"
    private const val MP = "mp:"

    private var cache: SharedPreferences? = null

    fun init(context: Context) {
        if (cache == null) {
            cache = context.applicationContext.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
        }
    }

    fun cached(imageUrl: String): String? = cache?.getString(imageUrl, null)

    fun remember(key: String, value: String) {
        cache?.edit()?.putString(key, value)?.apply()
    }

    fun register(token: String, imageUrl: String): Result<String> = runCatching {
        val body = JSONObject().put("urls", JSONArray().put(imageUrl)).toString()
        val endpoint = DiscordConfig.EXTERNAL_ASSETS_URL.format(DiscordConfig.APPLICATION_ID)
        val connection = (URL(endpoint).openConnection() as HttpURLConnection).apply {
            requestMethod = "POST"
            doOutput = true
            connectTimeout = 15_000
            readTimeout = 15_000
            setRequestProperty("Authorization", token)
            setRequestProperty("Content-Type", "application/json")
        }
        connection.outputStream.use { it.write(body.toByteArray()) }
        val status = connection.responseCode
        val text = (if (status in 200..299) connection.inputStream else connection.errorStream)
            ?.bufferedReader()?.use { it.readText() }.orEmpty()
        connection.disconnect()
        if (status !in 200..299) error("HTTP $status")
        val array = JSONArray(text)
        if (array.length() == 0) error("respuesta vacia")
        val path = array.getJSONObject(0).optString("external_asset_path")
        if (path.isEmpty()) error("sin external_asset_path")
        MP + path
    }.onSuccess { value ->
        cache?.edit()?.putString(imageUrl, value)?.apply()
    }.onFailure { error ->
        Log.w(TAG, "no pude registrar la imagen del personaje", error)
    }
}
