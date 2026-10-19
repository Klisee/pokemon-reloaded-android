package com.klisee.reloaded

import android.content.Context
import android.util.AtomicFile
import java.io.File
import java.io.IOException

/**
 * The launcher's own settings. The game keeps its separate options screen for in-game things; this
 * only holds what the launcher itself can honour.
 */
class Preferences(context: Context) {
    private val prefs = context.getSharedPreferences("launcher", Context.MODE_PRIVATE)

    /** Keep the screen awake while the game is running, so long battles don't lock the phone. */
    var keepScreenOn: Boolean
        get() = prefs.getBoolean("keepScreenOn", true)
        set(value) {
            prefs.edit().putBoolean("keepScreenOn", value).apply()
        }

    /** Whether the first-run setup has been completed. */
    var onboardingDone: Boolean
        get() = prefs.getBoolean("onboardingDone", false)
        set(value) {
            prefs.edit().putBoolean("onboardingDone", value).apply()
        }

    /** Pad buttons the player has rebound, as `ACTION=keyCode`. Absent means the built-in layout. */
    var controlBindings: Map<ControlAction, Int>
        get() = prefs.getStringSet("controlBindings", emptySet()).orEmpty().mapNotNull { entry ->
            val parts = entry.split('=')
            if (parts.size != 2) return@mapNotNull null
            val action = ControlAction.entries.firstOrNull { it.name == parts[0] } ?: return@mapNotNull null
            val code = parts[1].toIntOrNull() ?: return@mapNotNull null
            action to code
        }.toMap()
        set(value) {
            prefs.edit().putStringSet("controlBindings", value.map { "${it.key.name}=${it.value}" }.toSet()).apply()
        }

    /** Whether a pad's sticks move the player. */
    var stickEnabled: Boolean
        get() = prefs.getBoolean("stickEnabled", true)
        set(value) {
            prefs.edit().putBoolean("stickEnabled", value).apply()
        }

    /** How opaque the control artwork is drawn. 1 is solid; lower lets the game show through. */
    var touchOpacity: Float
        get() = prefs.getFloat("touchOpacity", 0.5f)
        set(value) {
            prefs.edit().putFloat("touchOpacity", value).apply()
        }

    /** One-shot flag: the classic overlay goes back to its cruceta. */
    var touchPadMigration: Boolean
        get() = prefs.getBoolean("touchPadMigration", false)
        set(value) {
            prefs.edit().putBoolean("touchPadMigration", value).apply()
        }

    /** Which overlay the game draws: the new one, the classic one, or none. */
    var touchStyle: TouchOverlayStyle
        get() = TouchOverlayStyle.from(prefs.getString("touchStyle", null))
        set(value) {
            prefs.edit().putString("touchStyle", value.id).apply()
        }

    /** Whether the arrow keys are driven by the joystick or the D-pad. */
    var moveMode: MoveMode
        get() = MoveMode.from(prefs.getString("moveMode", null))
        set(value) {
            prefs.edit().putString("moveMode", value.id).apply()
        }

    /** The overall size of the new overlay's controls. */
    var touchScale: Float
        get() = prefs.getFloat("touchScale", 1f)
        set(value) {
            prefs.edit().putFloat("touchScale", value).apply()
        }

    /** Where the player moved and resized each control, as `name=cx,cy,scale`. Empty means defaults. */
    var touchLayout: Map<String, Placement>
        get() = prefs.getStringSet("touchLayout", emptySet()).orEmpty().mapNotNull { entry ->
            val parts = entry.split('=')
            if (parts.size != 2) return@mapNotNull null
            val numbers = parts[1].split(',').mapNotNull { it.toFloatOrNull() }
            if (numbers.size < 2) return@mapNotNull null
            parts[0] to Placement(numbers[0], numbers[1], numbers.getOrElse(2) { 1f })
        }.toMap()
        set(value) {
            prefs.edit()
                .putStringSet("touchLayout", value.map { "${it.key}=${it.value.cx},${it.value.cy},${it.value.scale}" }.toSet())
                .apply()
        }

    /** Mirrors the current pad controls into the file the emulator reads at startup. */
    fun writeControls(filesDir: File, padConnected: Boolean) =
        writeControlsFile(filesDir, controlBindings, stickEnabled, padConnected)

    /** Mirrors the touch overlay into the file the emulator reads at startup. */
    fun writeTouch(filesDir: File) = writeTouchFile(filesDir, touchStyle, moveMode, touchScale, touchOpacity, touchLayout)

    /** Keep the shape the game was drawn for, with bands at the sides, instead of filling the screen. */
    var keepAspect: Boolean
        get() = prefs.getBoolean("keepAspect", false)
        set(value) {
            prefs.edit().putBoolean("keepAspect", value).apply()
        }

    /** Mirrors the video mode into the file the emulator reads at startup. */
    fun writeVideo(filesDir: File) = writeVideoFile(filesDir, keepAspect)

    // --- Controles dinámicos (edición Controles) ---------------------------------------------

    /** Opacity of the classic overlay, as a percentage (5..100). */
    var touchOpacityPercent: Int
        get() = kotlin.math.round(touchOpacity * 100f).toInt()
        set(value) {
            touchOpacity = value.coerceIn(5, 100) / 100f
        }

    fun touchLayouts(): Map<String, TouchPlacement> {
        val values = TouchLayouts.parse(prefs.getString("touchLayoutV1", TouchLayouts.HEADER).orEmpty())
        if (prefs.getBoolean("battleTopLayoutV1", false)) return values
        // One-time: in battle, the three buttons on the right sat on top of the party's status bars.
        // They are lined up with the three on the left; only the untouched old default moves.
        val migrated = values.toMutableMap()
        var changed = false
        for (id in listOf("t", "l", "p")) {
            val key = "battle/$id"
            val placement = migrated[key] ?: continue
            if (kotlin.math.abs(placement.y - 0.050f) < 0.001f) {
                migrated[key] = placement.copy(y = 0.180f)
                changed = true
            }
        }
        prefs.edit().putBoolean("battleTopLayoutV1", true).apply()
        if (changed) prefs.edit().putString("touchLayoutV1", TouchLayouts.encode(migrated)).apply()
        return migrated
    }

    /** Called on an IO dispatcher by Save and exit; a failed save is reported to the editor. */
    fun saveTouchLayouts(values: Map<String, TouchPlacement>, filesDir: File) {
        val text = TouchLayouts.encode(values)
        if (!prefs.edit().putString("touchLayoutV1", text).commit()) {
            throw IOException("No se pudo guardar la configuración. Inténtalo nuevamente.")
        }
        writeTouchLayoutText(filesDir, text)
    }

    fun writeTouchLayouts(filesDir: File) = runCatching {
        writeTouchLayoutText(filesDir, TouchLayouts.encode(touchLayouts()))
    }

    var touchAppearance: TouchAppearance
        get() = TouchAppearance.parse(prefs.getString("touchAppearanceV1", "").orEmpty())
        set(value) {
            prefs.edit().putString("touchAppearanceV1", value.encode()).apply()
        }

    fun writeTouchAppearance(filesDir: File) = runCatching {
        writeAtomicText(File(filesDir, "touch-appearance.conf"), touchAppearance.encode())
    }

    val touchSprites: Map<String, String>
        get() = TouchSprites.parse(prefs.getString("touchSpritesV1", "{}").orEmpty())

    fun saveTouchSprites(values: Map<String, String>) {
        if (!prefs.edit().putString("touchSpritesV1", TouchSprites.encode(values)).commit()) {
            throw IOException("No se pudieron guardar los sprites.")
        }
    }

    fun saveTouchConfiguration(
        layouts: Map<String, TouchPlacement>,
        appearance: TouchAppearance,
        opacity: Int,
        sprites: Map<String, String>,
    ) {
        if (!prefs.edit().putString("touchLayoutV1", TouchLayouts.encode(layouts))
                .putString("touchAppearanceV1", appearance.encode())
                .putFloat("touchOpacity", opacity.coerceIn(5, 100) / 100f)
                .putString("touchSpritesV1", TouchSprites.encode(sprites)).commit()
        ) {
            throw IOException("No se pudo guardar el paquete de controles.")
        }
    }

    private fun writeTouchLayoutText(filesDir: File, text: String) {
        writeAtomicText(File(filesDir, "touch-layout.conf"), text)
    }

    private fun writeAtomicText(target: File, text: String) {
        val file = AtomicFile(target)
        val stream = file.startWrite()
        try {
            stream.write(text.toByteArray(Charsets.UTF_8))
            file.finishWrite(stream)
        } catch (error: Exception) {
            file.failWrite(stream)
            throw error
        }
    }
}
