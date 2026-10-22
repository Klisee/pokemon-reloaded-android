package com.klisee.reloaded.discord

import android.content.Context
import org.json.JSONObject

/**
 * The game's room names are asset identifiers (`Home_1`, `MapaMenu`, `IntroROOM`), not display
 * names. This table (editable at `assets/room_names.json`) maps every room to the name shown in the
 * Rich Presence; anything missing falls back to the launcher's prettifier.
 */
object RoomNames {
    private var names: Map<String, String> = emptyMap()

    fun load(context: Context) {
        if (names.isNotEmpty()) return
        names = runCatching {
            val text = context.assets.open("room_names.json").bufferedReader().use { it.readText() }
            val json = JSONObject(text)
            json.keys().asSequence().associateWith { json.getString(it) }
        }.getOrDefault(emptyMap())
    }

    private val trailingNumber = Regex("""\s+\d+$""")
    // A route number or a floor is a different place; the number must stay.
    private val numberedPlace = Regex("""(?i)\b(Ruta|Piso|Sótano|Sotano|Planta|Nivel)\s+\d+$""")

    /** "En el Centro Pokémon 2" -> "En el Centro Pokémon"; "En la Ruta 2" is left alone. */
    fun clean(name: String): String {
        if (numberedPlace.containsMatchIn(name)) return name
        return name.replace(trailingNumber, "")
    }

    fun display(raw: String?): String? = raw?.let { names[it] }?.let { clean(it) }
}
