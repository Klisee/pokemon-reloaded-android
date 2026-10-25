package com.klisee.reloaded

import android.content.Context
import android.hardware.display.DisplayManager
import android.util.AtomicFile
import android.view.Display
import java.io.File
import java.io.IOException

/**
 * The launcher's own settings. The game keeps its separate options screen for in-game things; this
 * only holds what the launcher itself can honour.
 */
class Preferences(context: Context) {
    private val appContext = context.applicationContext
    private val prefs = context.getSharedPreferences("launcher", Context.MODE_PRIVATE)

    /** Keep the screen awake while the game is running, so long battles don't lock the phone. */
    var keepScreenOn: Boolean
        get() = prefs.getBoolean("keepScreenOn", true)
        set(value) {
            prefs.edit().putBoolean("keepScreenOn", value).apply()
        }

    /** Muestra en Discord que se está jugando y en qué ruta. */
    var discordEnabled: Boolean
        get() = prefs.getBoolean("discordEnabled", false)
        set(value) {
            prefs.edit().putBoolean("discordEnabled", value).apply()
        }

    /**
     * How the trainer's name reads in "En combate contra …": -1 abbreviates the class
     * ("E. Rubén"), 0 keeps it complete, and a positive value truncates to that many characters.
     */
    var trainerNameLimit: Int
        get() = prefs.getInt("trainerNameLimit", -1)
        set(value) {
            prefs.edit().putInt("trainerNameLimit", value).apply()
        }

    /** The Rich Presence warning keeps appearing until this is checked. */
    var discordWarningDismissed: Boolean
        get() = prefs.getBoolean("discordWarningDismissed", false)
        set(value) {
            prefs.edit().putBoolean("discordWarningDismissed", value).apply()
        }

    /**
     * Game language: "auto" follows the phone, "es" and "en" force one. The engine's text reader
     * reads the game's own `data/text/config.ini`, so this is written there before launching.
     */
    var gameLanguage: String
        get() = prefs.getString("gameLanguage", "auto") ?: "auto"
        set(value) {
            prefs.edit().putString("gameLanguage", value).apply()
        }

    fun resolvedGameLanguage(): String {
        val mode = gameLanguage
        if (mode == "es" || mode == "en") return mode
        return if (java.util.Locale.getDefault().language == "es") "es" else "en"
    }

    /** Writes the game's language index (0 Spanish, 1 English) where the engine reads it. */
    fun writeGameLanguage(filesDir: File) {
        val file = File(filesDir, "game/data/text/config.ini")
        val index = if (resolvedGameLanguage() == "en") 1 else 0
        runCatching {
            file.parentFile?.mkdirs()
            file.writeText(" $index")
        }
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

    /**
     * How drawing is paced: "native" keeps the original one frame per game step (the safe default),
     * "60" and "120" interpolate at those fixed rates, and "uncapped" never waits.
     */
    var renderMode: String
        get() {
            val stored = prefs.getString("renderMode", "native") ?: "native"
            val hz = detectedRefreshRate()
            return when (stored) {
                // Names used by the first test builds. Keep an existing opt-in, but fall back to a
                // mode the panel can actually show.
                "auto", "experimental" -> if (hz >= 119.5f) "120" else "60"
                "120" -> if (hz >= 119.5f) "120" else "native"
                "uncapped" -> if (hz >= 119.5f) "uncapped" else "native"
                "60" -> if (hz >= 59.5f) "60" else "native"
                else -> "native"
            }
        }
        set(value) {
            val safe = if (value in setOf("60", "120", "uncapped")) value else "native"
            prefs.edit().putString("renderMode", safe).apply()
        }

    /**
     * Highest refresh rate the panel advertises, in Hz. Falls back to the active mode and then to
     * 60 when Android does not report one. This is what the experimental mode asks the system for.
     */
    fun detectedRefreshRate(): Float {
        val manager = appContext.getSystemService(DisplayManager::class.java)
        val display = manager?.getDisplay(Display.DEFAULT_DISPLAY)
        return display?.supportedModes?.maxOfOrNull { it.refreshRate }
            ?: display?.mode?.refreshRate
            ?: display?.refreshRate
            ?: 60f
    }

    /** Draw the engine's actual frame rate in the top centre of the game. */
    var showFps: Boolean
        get() = prefs.getBoolean("showFps", false)
        set(value) {
            prefs.edit().putBoolean("showFps", value).apply()
        }

    /** Mirrors the video mode into the file the emulator reads at startup. */
    fun writeVideo(filesDir: File) = writeVideoFile(filesDir, keepAspect, renderMode, detectedRefreshRate(), showFps)

    // --- Controles dinámicos (edición Controles) ---------------------------------------------

    /** Opacity of the classic overlay, as a percentage (5..100). */
    var touchOpacityPercent: Int
        get() = kotlin.math.round(touchOpacity * 100f).toInt()
        set(value) {
            touchOpacity = value.coerceIn(5, 100) / 100f
        }

    fun touchLayouts(): Map<String, TouchPlacement> {
        val values = TouchLayouts.parse(prefs.getString("touchLayoutV1", TouchLayouts.HEADER).orEmpty()).toMutableMap()
        // One-time: X joins Z on the same row, as at the start of the game. Forced on update.
        if (!prefs.getBoolean("touchXZLayoutV1", false)) {
            values["world/z"] = TouchPlacement(0.865f, 0.851f)
            values["world/x"] = TouchPlacement(0.933f, 0.851f)
            prefs.edit().putBoolean("touchXZLayoutV1", true)
                .putString("touchLayoutV1", TouchLayouts.encode(values)).apply()
        }
        // One-time: the Navigation profile also gets X and Z together.
        if (!prefs.getBoolean("touchXZMenuV1", false)) {
            values["menu_nav/z"] = TouchPlacement(0.865f, 0.851f)
            values["menu_nav/x"] = TouchPlacement(0.933f, 0.851f)
            prefs.edit().putBoolean("touchXZMenuV1", true)
                .putString("touchLayoutV1", TouchLayouts.encode(values)).apply()
        }
        // One-time: final default is the game's diagonal, X above-right of Z.
        if (!prefs.getBoolean("touchXZDiagonalV1", false)) {
            values["world/z"] = TouchPlacement(0.865f, 0.851f)
            values["world/x"] = TouchPlacement(0.933f, 0.700f)
            values["menu_nav/z"] = TouchPlacement(0.865f, 0.851f)
            values["menu_nav/x"] = TouchPlacement(0.933f, 0.700f)
            prefs.edit().putBoolean("touchXZDiagonalV1", true)
                .putString("touchLayoutV1", TouchLayouts.encode(values)).apply()
        }
        // Forced: cajas/PC get their Z/X/S/O buttons even on custom layouts.
        if (!prefs.getBoolean("touchBoxLayoutV1", false)) {
            values["menu/box_z"] = TouchPlacement(0.865f, 0.851f)
            values["menu/box_x"] = TouchPlacement(0.933f, 0.700f)
            values["menu/box_s"] = TouchPlacement(0.712f, 0.700f)
            values["menu/box_o"] = TouchPlacement(0.712f, 0.840f)
            prefs.edit().putBoolean("touchBoxLayoutV1", true)
                .putString("touchLayoutV1", TouchLayouts.encode(values)).apply()
        }
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
