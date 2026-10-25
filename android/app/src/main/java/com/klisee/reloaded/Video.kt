package com.klisee.reloaded

import java.io.File
import kotlin.math.roundToInt

/**
 * Writes the video mode the emulator reads when it starts.
 *
 * The game was drawn for a 4:3 shape, so `aspect` shows it the way it looked on a PC, with bands at
 * the sides, while `full` stretches it to fill the phone's screen.
 */
fun writeVideoFile(filesDir: File, keepAspect: Boolean, renderMode: String, detectedHz: Float, showFps: Boolean) {
    val hz = detectedHz.takeIf { it.isFinite() && it > 0f }?.roundToInt()?.coerceAtLeast(1) ?: 60
    val text = buildString {
        appendLine("# Escrito por Ajustes. El emulador lo lee al arrancar.")
        appendLine("scaling=${if (keepAspect) "aspect" else "full"}")
        // native = one drawn frame per game step (30 FPS original); 60/120 = interpolate at that
        // fixed rate; uncapped = no waiting at all. detected_hz is kept for diagnostics and for
        // the engine's Auto fallback.
        val render = when {
            renderMode == "60" && hz >= 60 -> "60"
            renderMode == "120" && hz >= 120 -> "120"
            renderMode == "uncapped" && hz >= 120 -> "uncapped"
            else -> "native"
        }
        appendLine("render=$render")
        appendLine("detected_hz=$hz")
        appendLine("show_fps=${if (showFps) 1 else 0}")
    }
    runCatching { File(filesDir, "video.conf").writeText(text) }
}
