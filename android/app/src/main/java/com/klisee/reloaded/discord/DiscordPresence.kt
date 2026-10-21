package com.klisee.reloaded.discord

import org.json.JSONArray
import org.json.JSONObject

/**
 * Construye el frame `PRESENCE_UPDATE` (op 3) que espera el Gateway.
 *
 * La actividad lleva nombre, tipo y dos líneas de texto (`details` arriba, `state` abajo), más las
 * imágenes de los assets de la aplicación.
 */
object DiscordPresence {

    /** 0 = "Jugando a", que es de lo que va esto. */
    private const val TYPE_PLAYING = 0

    fun buildActivity(
        route: String?,
        startMs: Long?,
        smallImage: String? = null,
        smallText: String? = null,
        largeImage: String? = null,
        characterName: String? = null,
        characterFemale: Boolean = false,
        stateOverride: String? = null,
        english: Boolean = false,
    ): JSONObject {
        val shownRoute = route?.takeIf { it.isNotBlank() }

        val activity = JSONObject()
        activity.put("name", DiscordConfig.APP_NAME)
        activity.put("type", TYPE_PLAYING)
        // Without this Discord cannot tell which application owns the asset keys below, and draws
        // a question mark in their place.
        activity.put("application_id", DiscordConfig.APPLICATION_ID)
        activity.put("details", shownRoute ?: DiscordConfig.FALLBACK_DETAILS)
        activity.put(
            "state",
            characterName
                ?.takeIf { it.isNotBlank() }
                ?.let {
                    if (english) "Player: $it"
                    else "${if (characterFemale) "Jugadora" else "Jugador"}: $it"
                }
                ?: stateOverride?.takeIf { it.isNotBlank() }
                ?: DiscordConfig.FALLBACK_STATE,
        )

        val assets = JSONObject()
        // No large_image: Discord falls back to the application's icon from the Developer Portal.
        // That one is hosted by Discord and does not expire like an external file host URL does.
        largeImage?.takeIf { it.isNotBlank() }?.let {
            assets.put("large_image", it)
            assets.put("large_text", DiscordConfig.APP_NAME)
        }
        // A portal asset key does not resolve with a user token and Discord draws a question mark.
        // Until the character head is uploaded, the small slot is simply left out.
        smallImage?.takeIf { it.isNotBlank() }?.let {
            assets.put("small_image", it)
            assets.put("small_text", smallText ?: shownRoute ?: DiscordConfig.APP_NAME)
        }
        activity.put("assets", assets)

        if (startMs != null && startMs > 0L) {
            activity.put("timestamps", JSONObject().put("start", startMs))
        }

        return activity
    }

    fun buildPresenceUpdate(activity: JSONObject?, status: String = "online"): String {
        val activities = JSONArray()
        if (activity != null) activities.put(activity)

        val d = JSONObject()
        d.put("since", 0)
        d.put("activities", activities)
        d.put("status", status)
        d.put("afk", false)

        return JSONObject().put("op", 3).put("d", d).toString()
    }

    /** Quita la actividad del perfil; se manda al cerrar el juego. */
    fun buildClearActivity(): String = buildPresenceUpdate(activity = null)
}
