package com.klisee.reloaded.ui

import android.content.Context
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import com.klisee.reloaded.Preferences
import org.json.JSONObject

/**
 * UI translation. Spanish is the source; `ui_en.json` holds the English strings. The language is
 * the same one used for the game and the Rich Presence (auto by phone, or forced in Ajustes).
 */
object Strings {
    var english by mutableStateOf(false)
        private set

    private var map: Map<String, String> = emptyMap()

    fun init(context: Context) {
        if (map.isNotEmpty()) return
        map = runCatching {
            val text = context.assets.open("ui_en.json").bufferedReader().use { it.readText() }
            val json = JSONObject(text)
            json.keys().asSequence().associateWith { json.getString(it) }
        }.getOrDefault(emptyMap())
    }

    fun update(prefs: Preferences) {
        english = prefs.resolvedGameLanguage() == "en"
    }

    fun tr(text: String): String = if (!english) text else map[text] ?: text
}
