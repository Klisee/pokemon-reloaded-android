package com.klisee.reloaded.discord

/**
 * The trainer classes the game combines with a name ("Estudiante" + "Rubén"). They are shortened
 * to their initials so the battle line fits: "Estudiante Rubén" -> "E. Rubén",
 * "Entr. Guay María Teresa" -> "E.G. María Teresa".
 */
object TrainerNames {
    private val classes = listOf(
        "Coleccionista", "Chica Kimono", "Domadragones", "Electricista", "RuinaManíaco",
        "Pokemaníaco", "Alto Mando", "Arqueólogo", "CazaBichos", "Científica", "Científico",
        "Comandante", "Dominguera", "Entr. Guay", "Estudiante", "Ornitóloga", "Ornitólogo",
        "Pandillero", "PokéRanger", "Presidenta", "Aromaflor", "Caballero", "Enfermera",
        "Enfermero", "Exorcista", "Montañero", "Motorista", "Niño Bien", "Olvidador", "Profesora",
        "Campeona", "Campista", "Capitana", "Ciclista", "Criadora", "Fogonero", "Karateca",
        "Marinero", "Nadadora", "Operario", "Pescador", "Pirómano", "Señorita", "Veterana",
        "Veterano", "Anciano", "Artista", "Brujita", "Campeón", "Capitán", "Criador", "Nadador",
        "Playera", "Playero", "Pokéfan", "Rockera", "Rockero", "Ladrón", "Modelo", "Médium",
        "Rekiem", "Sub As", "Bella", "Chica", "Chico", "Friki", "Líder", "Ninja", "Profe", "Señor",
        "Dama", "As",
    ).sortedByDescending { it.length }

    /** Short classes are kept whole: "Líder Brock" already fits and reads better than "L. Brock". */
    private val keepWhole = setOf("Líder")

    fun abbreviate(name: String): String {
        val trainerClass = classes.firstOrNull { name.startsWith("$it ", ignoreCase = true) } ?: return name
        if (trainerClass in keepWhole) return name
        val rest = name.substring(trainerClass.length).trimStart()
        val initials = trainerClass.split(' ').filter { it.isNotBlank() }.joinToString("") {
            "${it.first().uppercaseChar()}."
        }
        // Keep the first name, abbreviate the rest: "María Teresa" -> "María T.".
        val words = rest.split(' ').filter { it.isNotBlank() }
        val shortName = if (words.size <= 1) {
            rest
        } else {
            words.first() + words.drop(1).joinToString("") { " ${it.first().uppercaseChar()}." }
        }
        return "$initials $shortName"
    }
}
