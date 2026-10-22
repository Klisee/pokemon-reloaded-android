package com.klisee.reloaded

/** These profiles mirror touch_controls::current_context; the editor never selects the live mode. */
enum class TouchProfile(val id: String, val title: String, val description: String) {
    WORLD("world", "Exploración", "Controles para caminar por el mundo."),
    BATTLE("battle", "Combate", "Perfil compartido por Pokémon salvajes y entrenadores."),
    DIALOG("dialog", "Diálogos", "Controles de los diálogos; por defecto, el botón para avanzar."),
    CHARACTER("character", "Personaje", "Selección de personaje. Las flechas originales siguen al personaje; al editarlas, quedan en la posición elegida."),
    CHOICE("choice", "Opciones", "Joystick y Z/X para responder las listas de opciones."),
    SAFARI("safari", "Safari", "Controles de captura en la Zona Safari."),
    MENU_NAV("menu_nav", "Navegación", "Mochila y listas: joystick, X, Z y gestos táctiles."),
    MENU("menu", "Menús y caja", "Toque directo: este modo no muestra botones superpuestos."),
}

data class TouchControlSpec(
    val id: String, val label: String, val x: Float, val y: Float, val radius: Float,
    val profiles: Set<TouchProfile>, val asset: String?, val joystick: Boolean = false, val aspect: Float = 1f,
) {
    val title: String get() = when (id) {
        "joystick" -> "Joystick"
        "pkmn" -> "SEL · equipo en Combate"
        "sel_world" -> "SEL · movimientos en Exploración"
        "dialogo" -> "Avanzar diálogo (OK)"
        else -> label
    }
}

data class TouchPlacement(val x: Float, val y: Float, val scale: Float = 1f, val visible: Boolean = true)
data class TouchGeometry(val x: Float, val y: Float, val radius: Float, val halfWidth: Float = radius)

object TouchLayouts {
    const val HEADER = "touch-layout-v1"
    // Kept in sync with the native catalogue by verification/check_catalogue.py.
    val controls = listOf(
        TouchControlSpec("alpha1", "1", 0.300f, 0.055f, 0.020f, setOf(TouchProfile.SAFARI), null, aspect = 1.000000f),
        TouchControlSpec("alpha2", "2", 0.372f, 0.055f, 0.020f, setOf(TouchProfile.SAFARI), null, aspect = 1.000000f),
        TouchControlSpec("alpha3", "3", 0.444f, 0.055f, 0.020f, setOf(TouchProfile.SAFARI), null, aspect = 1.000000f),
        TouchControlSpec("alpha4", "4", 0.516f, 0.055f, 0.020f, setOf(TouchProfile.SAFARI), null, aspect = 1.000000f),
        TouchControlSpec("alpha5", "5", 0.588f, 0.055f, 0.020f, setOf(TouchProfile.SAFARI), null, aspect = 1.000000f),
        TouchControlSpec("alpha6", "6", 0.660f, 0.055f, 0.020f, setOf(TouchProfile.SAFARI), null, aspect = 1.000000f),
        TouchControlSpec("alpha7", "7", 0.732f, 0.055f, 0.020f, setOf(TouchProfile.WORLD), null),
        TouchControlSpec("alpha8", "8", 0.228f, 0.050f, 0.022f, setOf(TouchProfile.WORLD), "alpha8.png"),
        TouchControlSpec("sel_world", "SEL", 0.300f, 0.760f, 0.038f, setOf(TouchProfile.WORLD), "sel_world.png"),
        TouchControlSpec("space", "PKM", 0.300f, 0.145f, 0.022f, setOf(TouchProfile.WORLD, TouchProfile.BATTLE, TouchProfile.SAFARI), "space.png", aspect = 1.000000f),
        TouchControlSpec("o", "O", 0.372f, 0.145f, 0.022f, setOf(TouchProfile.WORLD, TouchProfile.BATTLE, TouchProfile.SAFARI), "o.png", aspect = 1.000000f),
        TouchControlSpec("m", "M", 0.444f, 0.145f, 0.022f, setOf(TouchProfile.WORLD, TouchProfile.BATTLE, TouchProfile.SAFARI), "m.png", aspect = 1.000000f),
        TouchControlSpec("e", "E", 0.516f, 0.145f, 0.022f, setOf(TouchProfile.WORLD, TouchProfile.BATTLE, TouchProfile.SAFARI), "e.png", aspect = 1.000000f),
        TouchControlSpec("t", "T", 0.588f, 0.145f, 0.022f, setOf(TouchProfile.WORLD, TouchProfile.BATTLE, TouchProfile.SAFARI), "t.png", aspect = 1.000000f),
        TouchControlSpec("l", "L", 0.660f, 0.145f, 0.022f, setOf(TouchProfile.WORLD, TouchProfile.BATTLE, TouchProfile.SAFARI), "l.png", aspect = 1.000000f),
        TouchControlSpec("p", "P", 0.732f, 0.145f, 0.022f, setOf(TouchProfile.WORLD, TouchProfile.BATTLE, TouchProfile.SAFARI), "p.png", aspect = 1.000000f),
        TouchControlSpec("z", "Z", 0.865f, 0.851f, 0.033f, setOf(TouchProfile.WORLD, TouchProfile.BATTLE, TouchProfile.CHOICE, TouchProfile.MENU_NAV), "z.png", aspect = 1.000000f),
        TouchControlSpec("x", "X", 0.933f, 0.700f, 0.033f, setOf(TouchProfile.WORLD, TouchProfile.BATTLE, TouchProfile.CHOICE, TouchProfile.MENU_NAV), "x.png", aspect = 1.000000f),
        TouchControlSpec("c", "C", 0.797f, 0.700f, 0.033f, setOf(TouchProfile.WORLD, TouchProfile.BATTLE), "c.png", aspect = 1.000000f),
        TouchControlSpec("v", "V", 0.865f, 0.549f, 0.033f, setOf(TouchProfile.WORLD, TouchProfile.BATTLE), "v.png", aspect = 1.000000f),
        TouchControlSpec("b", "B", 0.712f, 0.840f, 0.026f, setOf(TouchProfile.BATTLE, TouchProfile.SAFARI), "b.png", aspect = 1.000000f),
        TouchControlSpec("s", "S", 0.712f, 0.700f, 0.026f, setOf(TouchProfile.BATTLE), "s.png", aspect = 1.000000f),
        TouchControlSpec("pkmn", "SEL", 0.300f, 0.760f, 0.038f, setOf(TouchProfile.BATTLE), "pkmn.png", aspect = 1.000000f),
        TouchControlSpec("a", "A", 0.640f, 0.900f, 0.024f, setOf(TouchProfile.WORLD, TouchProfile.BATTLE, TouchProfile.SAFARI), "a.png", aspect = 1.000000f),
        TouchControlSpec("n", "N", 0.640f, 0.760f, 0.024f, setOf(TouchProfile.WORLD, TouchProfile.SAFARI), "n.png", aspect = 1.000000f),
        TouchControlSpec("return", "OK", 0.500f, 0.945f, 0.032f, setOf(TouchProfile.WORLD, TouchProfile.BATTLE, TouchProfile.SAFARI), "return.png", aspect = 1.000000f),
        TouchControlSpec("escape", "ESC", 0.400f, 0.905f, 0.026f, setOf(TouchProfile.WORLD, TouchProfile.BATTLE, TouchProfile.SAFARI), "escape.png", aspect = 1.000000f),
        TouchControlSpec("dialogo", "Z", 0.940f, 0.880f, 0.024f, setOf(TouchProfile.DIALOG), "dialogo.png", aspect = 1.000000f),
        TouchControlSpec("flechaizquierda", "←", 0.095f, 0.340f, 0.030f, setOf(TouchProfile.CHARACTER), "flechaizquierda.png", aspect = 0.500000f),
        TouchControlSpec("flechaderecha", "→", 0.430f, 0.340f, 0.030f, setOf(TouchProfile.CHARACTER), "flechaderecha.png", aspect = 0.500000f),
        TouchControlSpec("eleccion_ok", "OK", 0.932f, 0.600f, 0.036f, setOf(TouchProfile.CHARACTER), "eleccion_ok.png", aspect = 1.900000f),
        TouchControlSpec("eleccion_atras", "X", 0.972f, 0.420f, 0.028f, setOf(TouchProfile.CHARACTER), "eleccion_atras.png", aspect = 1.000000f),
        TouchControlSpec("truco_ctrl_p", "C+P", 0.500f, 0.060f, 0.018f, setOf(TouchProfile.CHARACTER), "truco_ctrl_p.png", aspect = 7.625000f),
        TouchControlSpec("joystick", "Joystick", .135f, .700f, .200f,
            setOf(TouchProfile.WORLD, TouchProfile.BATTLE, TouchProfile.CHOICE, TouchProfile.MENU_NAV), "joystick-base.png", joystick = true),
    )
    fun key(profile: TouchProfile, spec: TouchControlSpec) = "${profile.id}/${spec.id}"
    fun availableFor(profile: TouchProfile) = if (profile == TouchProfile.MENU) emptyList() else
        controls.filter { (!it.joystick || profile in it.profiles) && !(profile == TouchProfile.WORLD && it.id in setOf("alpha1", "alpha2", "alpha3", "alpha4", "alpha5", "alpha6", "alpha7")) && (it.id != "sel_world" || profile == TouchProfile.WORLD) && (it.id != "pkmn" || profile == TouchProfile.BATTLE) }
    fun controlsFor(profile: TouchProfile, values: Map<String, TouchPlacement> = emptyMap()) =
        availableFor(profile).filter { profile in it.profiles || key(profile, it) in values }
    fun added(profile: TouchProfile, spec: TouchControlSpec) = profile !in spec.profiles

    val profileDefaults = mapOf(
        "world/space" to TouchPlacement(0.300f, 0.050f),
        "world/o" to TouchPlacement(0.372f, 0.050f),
        "world/m" to TouchPlacement(0.444f, 0.050f),
        "world/e" to TouchPlacement(0.516f, 0.050f),
        "world/t" to TouchPlacement(0.588f, 0.050f),
        "world/l" to TouchPlacement(0.660f, 0.050f),
        "world/p" to TouchPlacement(0.732f, 0.050f),
        "world/n" to TouchPlacement(0.645f, 0.720f),
        "world/a" to TouchPlacement(0.645f, 0.890f),
        "battle/space" to TouchPlacement(0.042f, 0.180f),
        "battle/o" to TouchPlacement(0.096f, 0.180f),
        "battle/e" to TouchPlacement(0.148f, 0.180f),
        // Level with the three on the left (0.180): higher up they landed on the party's status bars.
        "battle/t" to TouchPlacement(0.440f, 0.180f),
        "battle/l" to TouchPlacement(0.520f, 0.180f),
        "battle/p" to TouchPlacement(0.600f, 0.180f),
        "battle/m" to TouchPlacement(0.860f, 0.330f),
        "battle/a" to TouchPlacement(0.755f, 0.448f),
        "battle/s" to TouchPlacement(0.700f, 0.650f),
    )
    fun original(spec: TouchControlSpec, profile: TouchProfile = TouchProfile.WORLD) =
        profileDefaults[key(profile, spec)] ?: TouchPlacement(spec.x, spec.y)
    fun placement(values: Map<String, TouchPlacement>, profile: TouchProfile, spec: TouchControlSpec) =
        values[key(profile, spec)] ?: original(spec, profile)

    fun parse(text: String): Map<String, TouchPlacement> {
        if (text.length > 65536 || text.lineSequence().firstOrNull() != HEADER) return emptyMap()
        val result = linkedMapOf<String, TouchPlacement>()
        for (line in text.lineSequence().drop(1)) {
            val fields = line.trim().split(Regex("\\s+"))
            if (fields.size != 6) continue
            val profile = TouchProfile.entries.firstOrNull { it.id == fields[0] } ?: continue
            val spec = controls.firstOrNull { it.id == fields[1] } ?: continue
            if (profile == TouchProfile.MENU) continue
            val x = fields[2].toFloatOrNull()?.takeIf { it.isFinite() } ?: continue
            val y = fields[3].toFloatOrNull()?.takeIf { it.isFinite() } ?: continue
            val scale = fields[4].toFloatOrNull()?.takeIf { it.isFinite() } ?: continue
            val visible = when (fields[5]) { "1" -> true; "0" -> false; else -> continue }
            result[key(profile, spec)] = TouchPlacement(x.coerceIn(0f, 1f), y.coerceIn(0f, 1f), scale.coerceIn(.5f, 2f), visible)
        }
        return result
    }

    fun encode(values: Map<String, TouchPlacement>): String = buildString {
        appendLine(HEADER)
        for (profile in TouchProfile.entries.filter {it != TouchProfile.MENU}) for (spec in controls) {
            val p = values[key(profile, spec)] ?: continue
            if (!p.x.isFinite() || !p.y.isFinite() || !p.scale.isFinite()) continue
            appendLine("${profile.id} ${spec.id} ${p.x.coerceIn(0f, 1f)} ${p.y.coerceIn(0f, 1f)} ${p.scale.coerceIn(.5f, 2f)} ${if (p.visible) 1 else 0}")
        }
    }

    fun resetProfile(values: Map<String, TouchPlacement>, profile: TouchProfile) =
        values.filterKeys { !it.startsWith(profile.id + "/") }

    /** Normalised coordinates survive resolution changes. Dialogues retain their game-viewport anchor. */
    fun viewport(width: Float, height: Float, keepAspect: Boolean): FloatArray {
        if (!keepAspect || width <= 0f || height <= 0f) return floatArrayOf(0f, 0f, 1f, 1f)
        val gameAspect = 4f / 3f
        val w = minOf(width, height * gameAspect) / width
        val h = minOf(height, width / gameAspect) / height
        return floatArrayOf((1f - w) / 2f, (1f - h) / 2f, w, h)
    }

    fun geometry(spec: TouchControlSpec, p: TouchPlacement, width: Float, height: Float, keepAspect: Boolean, forceFit: Boolean = false): TouchGeometry {
        if (width <= 0f || height <= 0f) return TouchGeometry(0f, 0f, 0f)
        val v = if (TouchProfile.DIALOG in spec.profiles) viewport(width, height, keepAspect) else floatArrayOf(0f,0f,1f,1f)
        val rawRadius = spec.radius * p.scale * if (spec.joystick) height else width * v[2]
        // An untouched control must preview the original engine layout exactly, including on 4:3
        // tablets. Once edited, both the engine and editor keep its full touch target on screen.
        if (p == original(spec) && !forceFit) return TouchGeometry((v[0] + p.x * v[2]) * width,
            (v[1] + p.y * v[3]) * height, rawRadius, rawRadius * spec.aspect)
        val radius = if (spec.joystick) rawRadius else minOf(rawRadius, width * .49f / spec.aspect, height * .49f)
        val margin = if (spec.joystick) radius * 1.05f else radius
        val mx = minOf(margin * spec.aspect, width * .49f)
        val my = minOf(margin, height * .49f)
        return TouchGeometry(((v[0] + p.x * v[2]) * width).coerceIn(mx, width - mx),
            ((v[1] + p.y * v[3]) * height).coerceIn(my, height - my), radius, radius * spec.aspect)
    }

    fun moved(spec: TouchControlSpec, p: TouchPlacement, dx: Float, dy: Float, width: Float, height: Float, keepAspect: Boolean, forceFit: Boolean = false): TouchPlacement {
        if (width <= 0f || height <= 0f) return p
        val v = if (TouchProfile.DIALOG in spec.profiles) viewport(width, height, keepAspect) else floatArrayOf(0f,0f,1f,1f)
        val current = geometry(spec, p, width, height, keepAspect, forceFit)
        val desired = p.copy(x = ((current.x + dx) / width - v[0]) / v[2], y = ((current.y + dy) / height - v[1]) / v[3])
        val fitted = geometry(spec, desired, width, height, keepAspect, forceFit)
        return desired.copy(x = ((fitted.x / width - v[0]) / v[2]).coerceIn(0f,1f),
            y = ((fitted.y / height - v[1]) / v[3]).coerceIn(0f,1f))
    }
}
