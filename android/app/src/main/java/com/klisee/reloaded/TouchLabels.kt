package com.klisee.reloaded

import android.content.Context
import android.graphics.Bitmap
import android.graphics.Canvas
import android.graphics.Paint
import android.graphics.Rect
import android.graphics.Typeface
import java.io.File

/**
 * Draws every control label with a clean sans-serif face and leaves the images where the engine
 * reads them, so the overlay shows real text instead of the blocky built-in bitmap font.
 *
 * The engine derives the same set of labels from its catalogue and looks them up by the same file
 * name (`label_<label with non-alphanumerics replaced by _>.png`), so both sides must agree.
 */
object TouchLabels {
    /** Every label the classic catalogue can show (arrows are drawn as triangles, not text). */
    private val LABELS = listOf(
        "1", "2", "3", "4", "5", "6", "7",
        "PKM", "SEL", "ESC", "OK", "C+P",
        "O", "M", "E", "T", "L", "P",
        "Z", "X", "C", "V", "B", "S", "A", "N",
    )

    fun fileName(label: String): String =
        "label_" + label.map { if (it.isLetterOrDigit()) it else '_' }.joinToString("") + ".png"

    /** Mirrors the label images into the folder the engine reads at startup. */
    fun writeRuntime(context: Context, filesDir: File) {
        val dir = File(filesDir, "touch-labels").apply { mkdirs() }
        dir.listFiles()?.forEach { it.delete() }
        val paint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
            // A neutral, geometric sans. If the device does not have this family, Android falls back
            // to its default sans, which still reads as clean text.
            typeface = Typeface.create("sans-serif-black", Typeface.BOLD)
            color = 0xFFA8A8AA.toInt()
            textAlign = Paint.Align.LEFT
        }
        for (label in LABELS) {
            val bitmap = drawLabel(label, paint) ?: continue
            runCatching { File(dir, fileName(label)).outputStream().use { bitmap.compress(Bitmap.CompressFormat.PNG, 100, it) } }
            bitmap.recycle()
        }
    }

    private fun drawLabel(label: String, paint: Paint): Bitmap? {
        paint.textSize = 96f
        val bounds = Rect()
        paint.getTextBounds(label, 0, label.length, bounds)
        val pad = 10
        val width = (bounds.width() + pad * 2).coerceAtLeast(1)
        val height = (bounds.height() + pad * 2).coerceAtLeast(1)
        return runCatching {
            val bitmap = Bitmap.createBitmap(width, height, Bitmap.Config.ARGB_8888)
            val canvas = Canvas(bitmap)
            canvas.drawText(label, pad.toFloat(), (pad - bounds.top).toFloat(), paint)
            bitmap
        }.getOrNull()
    }
}
