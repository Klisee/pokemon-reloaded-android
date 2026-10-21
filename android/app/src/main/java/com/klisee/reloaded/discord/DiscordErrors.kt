package com.klisee.reloaded.discord

/** Errores del inicio de sesión de Discord, separados para poder avisar de cada uno. */
sealed class DiscordAuthException(message: String, cause: Throwable? = null) : Exception(message, cause) {
    class Cancelled(message: String = "Inicio de sesión cancelado") : DiscordAuthException(message)
    class InvalidGrant(message: String = "Token de Discord inválido") : DiscordAuthException(message)
}
