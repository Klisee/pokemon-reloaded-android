package com.klisee.reloaded

import android.graphics.Bitmap
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.asImageBitmap
import java.util.Locale

enum class TouchStyle(val id: String, val title: String) {
    MODERN("modern", "Moderno"), CLASSIC("classic", "Clasico"),
}
enum class TouchStrength(val title: String, val scale: Float) { SOFT("Suave", .5f), MEDIUM("Media", .7f), STRONG("Fuerte", 1f) }
data class TouchAppearance(
    val style: TouchStyle = TouchStyle.MODERN,
    val colorHex: String = "original",
    val animations: Boolean = true,
    val haptics: Boolean = true,
    val strength: TouchStrength = TouchStrength.SOFT,
) {
    val tint: Int? get() = colorHex.takeIf { it.matches(Regex("[a-fA-F0-9]{6}")) }?.toIntOrNull(16)
    fun encode(): String = "touch-appearance-v1\nstyle=${style.id}\ntint=${tint?.let { String.format(Locale.ROOT, "%06X", it) } ?: "original"}\nanimations=${if (animations) 1 else 0}\nhaptics=${if (haptics) 1 else 0}\nstrength=${strength.name}\n"
    companion object {
        fun parse(text: String): TouchAppearance {
            if (text.length > 4096 || text.lineSequence().firstOrNull() != "touch-appearance-v1") return TouchAppearance()
            val fields = text.lineSequence().drop(1).mapNotNull { line -> line.split('=', limit=2).takeIf { it.size==2 }?.let { it[0].trim() to it[1].trim() } }.toMap()
            return TouchAppearance(
                style = if (fields["style"] in setOf("classic", "minimal", "pixel")) TouchStyle.CLASSIC else TouchStyle.MODERN,
                colorHex = fields["tint"]?.takeIf { it.matches(Regex("[a-fA-F0-9]{6}")) }?.uppercase(Locale.ROOT) ?: "original",
                animations = fields["animations"] != "0", haptics = fields["haptics"] != "0",
                strength = TouchStrength.entries.firstOrNull { it.name == fields["strength"] } ?: TouchStrength.SOFT,
            )
        }
        fun tintedPixel(pixel: Int, tint: Int): Int {
            val light = maxOf((pixel shr 16) and 255, (pixel shr 8) and 255, pixel and 255)
            fun channel(shift: Int) = ((light * ((tint shr shift) and 255) + 127) / 255) shl shift
            return (pixel and -0x1000000) or channel(16) or channel(8) or channel(0)
        }
        fun image(bitmap: Bitmap, tint: Int?): ImageBitmap {
            if (tint == null) return bitmap.asImageBitmap()
            val pixels = IntArray(bitmap.width * bitmap.height)
            bitmap.getPixels(pixels, 0, bitmap.width, 0, 0, bitmap.width, bitmap.height)
            for (i in pixels.indices) pixels[i] = tintedPixel(pixels[i], tint)
            return Bitmap.createBitmap(pixels, bitmap.width, bitmap.height, Bitmap.Config.ARGB_8888).asImageBitmap()
        }
    }
}
