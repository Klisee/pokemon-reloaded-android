package com.klisee.reloaded.discord

import android.content.Context
import org.json.JSONObject

/**
 * One playable character: the name, the head file and, when it was uploaded to the Discord
 * application, the asset key that Rich Presence can use directly.
 */
data class RpcCharacter(val name: String, val file: String, val asset: String = "", val female: Boolean = false)

/**
 * Maps what the engine reports — the protagonist slot and, for a custom character, its trainer id —
 * to the name and head image. The table is generated from the game's own character list; the images
 * themselves live outside the app.
 */
object DiscordCharacters {
    private var base: List<RpcCharacter> = emptyList()
    private var custom: Map<String, RpcCharacter> = emptyMap()
    private var loaded = false

    fun load(context: Context) {
        if (loaded) return
        loaded = true
        runCatching {
            val text = context.assets.open("characters.json").bufferedReader().use { it.readText() }
            val root = JSONObject(text)
            val baseArray = root.getJSONArray("base")
            base = (0 until baseArray.length()).map { index ->
                val item = baseArray.getJSONObject(index)
                RpcCharacter(
                    item.getString("name"),
                    item.getString("file"),
                    item.optString("asset"),
                    item.optString("sex").equals("f", ignoreCase = true),
                )
            }
            val customObject = root.getJSONObject("custom")
            custom = customObject.keys().asSequence().associateWith { key ->
                val item = customObject.getJSONObject(key)
                RpcCharacter(
                    item.getString("name"),
                    item.getString("file"),
                    item.optString("asset"),
                    item.optString("sex").equals("f", ignoreCase = true),
                )
            }
        }
    }

    /** 0..9 are the base roster; 10 is a custom trainer, identified by its `char2` id. */
    fun resolve(prota: Int, char2: Int): RpcCharacter? =
        if (prota == 10) custom[char2.toString()] else base.getOrNull(prota)
}
