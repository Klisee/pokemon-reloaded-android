package com.klisee.reloaded.discord

import android.content.Context
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.graphics.Canvas
import android.graphics.Paint
import android.graphics.RectF
import android.util.Log
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.MultipartBody
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody.Companion.asRequestBody
import java.io.File
import kotlin.math.min
import kotlin.math.roundToInt

/**
 * Builds the small image for a custom character from the game's own trainer sprite, without shipping
 * any artwork in the app: the sprite is in the installed game, the head is cropped from its top, and
 * only that one image is uploaded (anonymously, no account) when the character is used.
 */
object DiscordCharacterImage {
    private const val TAG = "DiscordRpc"
    // Catbox is unreachable from this network; x0.at is a 0x0 instance that answers and keeps
    // small images long enough. It needs the file in a "file" field and replies with the URL.
    private const val HOST = "https://x0.at"
    // x0.at is sometimes slow to complete the TLS handshake from a phone; the defaults gave up
    // after 10 s and the presence was left without its image.
    private val client = OkHttpClient.Builder()
        .connectTimeout(25, java.util.concurrent.TimeUnit.SECONDS)
        .readTimeout(30, java.util.concurrent.TimeUnit.SECONDS)
        .writeTimeout(30, java.util.concurrent.TimeUnit.SECONDS)
        .build()

    /** Crops the head from the sprite the engine exported (`files/rpc-player.png`). */
    fun headFromPlayer(context: Context): File? {
        val sourceFile = File(context.filesDir, "rpc-player.png")
        if (!sourceFile.isFile) return null
        val source = BitmapFactory.decodeFile(sourceFile.path) ?: return null
        return runCatching {
            // The head is the top of the sprite; the rest is the body. Discord shows this at about
            // 20 px, so a slight crop is enough and no artwork has to be bundled.
            // A few pixels lower for every sprite: the top of the hair is clipped anyway and the
            // chin/neck reads better with a sliver of body, which is how the portraits look.
            val cropTop = min(3, (source.height - 1).coerceAtLeast(0))
            // Proportional to the sprite's height, not its width: sprites have different proportions
            // (Mark 20x29, Kate 19x31) and the width rule framed them differently.
            val headHeight = min(
                source.height - cropTop,
                (source.height * 0.55f).roundToInt().coerceAtLeast(1),
            )
            val head = Bitmap.createBitmap(source, 0, cropTop, source.width, headHeight)
            val size = 128
            val out = Bitmap.createBitmap(size, size, Bitmap.Config.ARGB_8888)
            val scale = min(size.toFloat() / head.width, size.toFloat() / head.height)
            val width = head.width * scale
            val height = head.height * scale
            val left = (size - width) / 2f
            val top = (size - height) / 2f
            Canvas(out).drawBitmap(
                head,
                null,
                RectF(left, top, left + width, top + height),
                Paint().apply { isFilterBitmap = false },
            )
            val file = File(context.cacheDir, "rpc-head-player.png")
            file.outputStream().use { out.compress(Bitmap.CompressFormat.PNG, 100, it) }
            head.recycle()
            out.recycle()
            file
        }.also { source.recycle() }.getOrNull()
    }

    /** Uploads the image anonymously; catbox returns its public URL as plain text. */
    fun upload(file: File): Result<String> {
        var last: Throwable? = null
        repeat(3) { attempt ->
            runCatching { uploadOnce(file) }
                .onSuccess { url ->
                    Log.i(TAG, "imagen subida: $url")
                    return Result.success(url)
                }
                .onFailure {
                    last = it
                    Log.w(TAG, "subida fallida (intento ${attempt + 1}/3)", it)
                    if (attempt < 2) {
                        runCatching { Thread.sleep(700L * (attempt + 1)) }
                    }
                }
        }
        return Result.failure(last ?: IllegalStateException("no pude subir la imagen"))
    }

    private fun uploadOnce(file: File): String {
        val body = MultipartBody.Builder()
            .setType(MultipartBody.FORM)
            .addFormDataPart("file", file.name, file.asRequestBody("image/png".toMediaType()))
            .build()
        val request = Request.Builder().url(HOST).post(body).build()
        client.newCall(request).execute().use { response ->
            if (!response.isSuccessful) error("host respondio ${response.code}")
            val url = response.body?.string()?.trim().orEmpty()
            if (!url.startsWith("http")) error("host: $url")
            return url
        }
    }
}
