package com.klisee.reloaded

import java.io.File

/** Which overlay the game draws. */
enum class TouchOverlayStyle(val id: String, val label: String) {
    New("new", "Nuevos"),
    Classic("classic", "Clasicos"),
    None("none", "Ninguno");

    companion object {
        // The Controles distribution is the one the game uses; "Nuevos" is kept only for legacy files.
        fun from(id: String?): TouchOverlayStyle = if (id == "none") None else Classic
    }
}

/** What drives the arrow keys. */
enum class MoveMode(val id: String, val label: String) {
    Joystick("joystick", "Joystick"),
    Dpad("dpad", "Cruceta");

    companion object {
        fun from(id: String?): MoveMode = entries.firstOrNull { it.id == id } ?: Joystick
    }
}

/** Where and how big a control is, in window-normalised coordinates. */
data class Placement(val cx: Float, val cy: Float, val scale: Float = 1f)

/**
 * A control the editor offers, with the place it has by default.
 *
 * This mirrors the engine's table (`touch_controls.rs`, `CONTROLS` plus the movement control): if a
 * control moves there, move it here too. The two picker arrows, the dialogue button and the trick
 * button are left out on purpose - the first two ride on the game's own drawing, and the last is a
 * one-off.
 */
data class TouchControl(val name: String, val label: String, val cx: Float, val cy: Float) {
    val placement: Placement get() = Placement(cx, cy, 1f)
}

val TOUCH_CONTROLS: List<TouchControl> = listOf(
    TouchControl("dpad", "Movimiento", 0.135f, 0.700f),
    TouchControl("alpha1", "1", 0.300f, 0.055f),
    TouchControl("alpha2", "2", 0.372f, 0.055f),
    TouchControl("alpha3", "3", 0.444f, 0.055f),
    TouchControl("alpha5", "5", 0.516f, 0.055f),
    TouchControl("alpha7", "7", 0.588f, 0.055f),
    TouchControl("alpha8", "8", 0.228f, 0.050f),
    TouchControl("space", "Pokemon", 0.300f, 0.145f),
    TouchControl("o", "Mochila", 0.372f, 0.145f),
    TouchControl("m", "Mega", 0.444f, 0.145f),
    TouchControl("e", "Escapar", 0.516f, 0.145f),
    TouchControl("t", "T", 0.588f, 0.145f),
    TouchControl("l", "L", 0.660f, 0.145f),
    TouchControl("p", "P", 0.732f, 0.145f),
    TouchControl("z", "Z", 0.865f, 0.851f),
    TouchControl("x", "X", 0.933f, 0.700f),
    TouchControl("c", "C", 0.797f, 0.700f),
    TouchControl("v", "V", 0.865f, 0.549f),
    TouchControl("b", "Pokebola", 0.712f, 0.840f),
    TouchControl("s", "Cambio", 0.712f, 0.700f),
    TouchControl("pkmn", "Equipo", 0.300f, 0.760f),
    TouchControl("a", "A", 0.640f, 0.900f),
    TouchControl("n", "N", 0.640f, 0.760f),
    TouchControl("return", "OK", 0.500f, 0.945f),
    TouchControl("escape", "ESC", 0.400f, 0.905f),
    TouchControl("eleccion_ok", "OK eleccion", 0.932f, 0.600f),
    TouchControl("eleccion_atras", "X eleccion", 0.972f, 0.420f),
)

/** The place a control has when the player has not moved it. */
fun defaultPlacement(name: String): Placement =
    TOUCH_CONTROLS.firstOrNull { it.name == name }?.placement ?: Placement(0.5f, 0.5f)

/**
 * Writes the overlay the engine reads at startup: the style, what drives the movement, the sizes and
 * where the player put each control.
 */
fun writeTouchFile(
    filesDir: File,
    style: TouchOverlayStyle,
    move: MoveMode,
    scale: Float,
    opacity: Float,
    layout: Map<String, Placement>,
) {
    val text = buildString {
        appendLine("# Escrito por el editor de controles. El emulador lo lee al arrancar.")
        appendLine("style=${style.id}")
        appendLine("move=${move.id}")
        appendLine("scale=$scale")
        appendLine("opacity=$opacity")
        for ((name, place) in layout) {
            appendLine("pos.$name=${place.cx},${place.cy}")
            appendLine("size.$name=${place.scale}")
        }
    }
    runCatching { File(filesDir, "touch.conf").writeText(text) }
}
