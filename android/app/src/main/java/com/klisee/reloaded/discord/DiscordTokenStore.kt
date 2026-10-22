package com.klisee.reloaded.discord

import android.content.Context
import android.content.SharedPreferences
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Base64
import android.util.Log
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/**
 * Guarda el token de Discord entre ejecuciones, cifrado con una clave que no sale del Keystore de
 * Android. Es lo que permite reconectar sin volver a iniciar sesión.
 */
object DiscordTokenStore {
    private const val TAG = "DiscordRpc"
    private const val PREFS = "discord_auth"

    private const val KEY_TOKEN = "token"
    private const val KEY_USER_ID = "user_id"
    private const val KEY_USERNAME = "username"
    private const val KEY_GLOBAL_NAME = "global_name"
    private const val KEY_AVATAR = "avatar"

    private var prefs: SharedPreferences? = null

    fun init(context: Context) {
        if (prefs == null) {
            prefs = context.applicationContext.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
        }
    }

    data class User(val id: String, val username: String, val globalName: String?, val avatar: String?)

    fun storeToken(token: String) {
        prefs?.edit()?.putString(KEY_TOKEN, encrypt(token))?.apply()
    }

    fun getToken(): String? = prefs?.getString(KEY_TOKEN, null)?.let(::decrypt)

    fun storeUser(user: User) {
        prefs?.edit()
            ?.putString(KEY_USER_ID, user.id)
            ?.putString(KEY_USERNAME, user.username)
            ?.putString(KEY_GLOBAL_NAME, user.globalName)
            ?.putString(KEY_AVATAR, user.avatar)
            ?.apply()
    }

    fun getUser(): User? {
        val store = prefs ?: return null
        val id = store.getString(KEY_USER_ID, null) ?: return null
        return User(
            id = id,
            username = store.getString(KEY_USERNAME, "") ?: "",
            globalName = store.getString(KEY_GLOBAL_NAME, null),
            avatar = store.getString(KEY_AVATAR, null),
        )
    }

    fun clear() {
        prefs?.edit()?.clear()?.apply()
    }

    // --- Cifrado ------------------------------------------------------------------------------

    private const val ANDROID_KEYSTORE = "AndroidKeyStore"
    private const val KEY_ALIAS = "reloaded_discord_token_key"
    private const val TRANSFORMATION = "AES/GCM/NoPadding"
    private const val GCM_TAG_BITS = 128

    private fun key(): SecretKey? = runCatching {
        val keyStore = KeyStore.getInstance(ANDROID_KEYSTORE).apply { load(null) }
        (keyStore.getEntry(KEY_ALIAS, null) as? KeyStore.SecretKeyEntry)?.secretKey ?: run {
            val generator = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, ANDROID_KEYSTORE)
            generator.init(
                KeyGenParameterSpec.Builder(
                    KEY_ALIAS,
                    KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT,
                )
                    .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                    .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                    .build(),
            )
            generator.generateKey()
        }
    }.onFailure { Log.w(TAG, "keystore key unavailable", it) }.getOrNull()

    private fun encrypt(value: String): String? {
        if (value.isEmpty()) return ""
        val secret = key() ?: return null
        return runCatching {
            val cipher = Cipher.getInstance(TRANSFORMATION)
            cipher.init(Cipher.ENCRYPT_MODE, secret)
            val payload = cipher.iv + cipher.doFinal(value.toByteArray(Charsets.UTF_8))
            Base64.encodeToString(payload, Base64.NO_WRAP)
        }.onFailure { Log.w(TAG, "encrypt failed", it) }.getOrNull()
    }

    private fun decrypt(stored: String): String? {
        if (stored.isEmpty()) return ""
        val secret = key() ?: return null
        return runCatching {
            val payload = Base64.decode(stored, Base64.NO_WRAP)
            val iv = payload.copyOfRange(0, 12)
            val body = payload.copyOfRange(12, payload.size)
            val cipher = Cipher.getInstance(TRANSFORMATION)
            cipher.init(Cipher.DECRYPT_MODE, secret, GCMParameterSpec(GCM_TAG_BITS, iv))
            String(cipher.doFinal(body), Charsets.UTF_8)
        }.onFailure { Log.w(TAG, "decrypt failed", it) }.getOrNull()
    }
}
