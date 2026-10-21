package com.klisee.reloaded.discord

/**
 * Todo lo que la integración con Discord necesita, apuntando a la aplicación propia del proyecto.
 *
 * El Rich Presence va por el Gateway de Discord con el token de la cuenta que inicia sesión en
 * [DiscordLoginActivity]. No depende de aprobación del Social SDK; las imágenes salen de los assets
 * que se suban al Developer Portal con las claves de abajo.
 */
object DiscordConfig {

    /** Sin esto la función queda desactivada y Ajustes avisa de que falta configurarla. */
    const val APPLICATION_ID = "1561228465129128027"

    /** Claves subidas en el portal: Rich Presence -> Art Assets. */
    // Reused until the engine reports a character with its own asset.

    /**
     * Base pública donde están las cabezas (con barra final), p. ej. `https://host/heads/`.
     * Vacío: la presencia muestra el nombre del personaje pero no su imagen.
     */
    const val ASSETS_BASE_URL = ""

    /** Registra una imagen externa y devuelve el `mp:` que entiende Rich Presence. */
    const val EXTERNAL_ASSETS_URL = "https://discord.com/api/v9/applications/%s/external-assets"

    const val APP_NAME = "Pokémon Reloaded"

    /** The Discord application icon (CDN, hosted by Discord). This is what the presence shows. */
    const val APP_ICON_URL =
        "https://cdn.discordapp.com/app-icons/1561228465129128027/6ed2b13c6d164b5ad3da52172b630a62.png?size=256"

    /** Línea 2 y línea 3 cuando todavía no se conoce la sala. */
    const val FALLBACK_DETAILS = "En el Lanzador"
    const val FALLBACK_STATE = "Pokémon Reloaded"

    const val CURRENT_USER_URL = "https://discord.com/api/v10/users/@me"

    val isConfigured: Boolean get() = APPLICATION_ID.isNotBlank()
}
