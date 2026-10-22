package com.klisee.reloaded.ui

import android.graphics.BitmapFactory
import androidx.compose.foundation.Image
import androidx.compose.foundation.layout.Spacer
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.layout.ContentScale
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.withContext
import java.net.HttpURLConnection
import java.net.URL

/**
 * Loads one of the blog's pictures with the platform decoder. No image library is needed for what
 * the launcher shows, and it keeps the APK from growing another dependency.
 */
@Composable
fun RemoteImage(url: String, modifier: Modifier = Modifier, contentScale: ContentScale = ContentScale.Crop) {
    var bitmap by remember(url) { mutableStateOf<ImageBitmap?>(null) }

    LaunchedEffect(url) {
        repeat(4) { attempt ->
            val loaded = withContext(Dispatchers.IO) {
                runCatching {
                    val connection = (URL(url).openConnection() as HttpURLConnection).apply {
                        connectTimeout = 15_000
                        readTimeout = 15_000
                    }
                    connection.inputStream.use { BitmapFactory.decodeStream(it)?.asImageBitmap() }
                }.getOrNull()
            }
            if (loaded != null) {
                bitmap = loaded
                return@LaunchedEffect
            }
            delay(2_000L * (attempt + 1))
        }
    }

    val image = bitmap
    if (image == null) {
        // Transparent: callers can draw something underneath (the app icon in the preview).
        Spacer(modifier)
    } else {
        Image(
            bitmap = image,
            contentDescription = null,
            contentScale = contentScale,
            modifier = modifier,
        )
    }
}
