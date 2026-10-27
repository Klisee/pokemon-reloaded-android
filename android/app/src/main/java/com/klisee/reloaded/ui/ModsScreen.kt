package com.klisee.reloaded.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Build
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import java.io.File

private data class ModEntry(
    val id: String,
    val name: String,
    val version: String,
    val description: String,
    val enabled: Boolean,
    val manifest: File,
)

private fun readMods(gameDir: File): List<ModEntry> {
    val dir = File(gameDir, "mods")
    if (!dir.isDirectory) return emptyList()
    return dir.listFiles().orEmpty()
        .filter { it.isDirectory && File(it, "mod.ini").isFile }
        .sortedBy { it.name.lowercase() }
        .map { folder ->
            val manifest = File(folder, "mod.ini")
            val values = manifest.readLines().mapNotNull { line ->
                val text = line.trim()
                if (text.isEmpty() || text.startsWith("#") || text.startsWith(";") || !text.contains('=')) null
                else text.substringBefore('=').trim().lowercase() to text.substringAfter('=').trim()
            }.toMap()
            ModEntry(
                id = folder.name,
                name = values["name"].orEmpty().ifEmpty { folder.name },
                version = values["version"].orEmpty(),
                description = values["description"].orEmpty(),
                enabled = !values["enabled"].orEmpty().lowercase().let { it == "0" || it == "false" || it == "no" || it == "off" },
                manifest = manifest,
            )
        }
}

private fun setEnabled(entry: ModEntry, enabled: Boolean) {
    val lines = entry.manifest.readLines().toMutableList()
    val index = lines.indexOfFirst { it.substringBefore('=').trim().equals("enabled", ignoreCase = true) }
    val value = if (enabled) "1" else "0"
    if (index >= 0) lines[index] = "enabled=$value" else lines.add("enabled=$value")
    entry.manifest.writeText(lines.joinToString("\n") + "\n")
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ModsScreen(filesDir: File, onBack: () -> Unit) {
    val gameDir = File(filesDir, "game")
    var mods by remember { mutableStateOf(readMods(gameDir)) }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("Mods") },
                navigationIcon = {
                    IconButton(onClick = onBack) { Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "Volver") }
                },
            )
        },
    ) { padding ->
        Column(
            modifier = Modifier.padding(padding).fillMaxSize().verticalScroll(rememberScrollState()).padding(horizontal = 16.dp),
            verticalArrangement = Arrangement.spacedBy(16.dp),
        ) {
            Spacer(Modifier.height(4.dp))
            Text(
                "Activa o desactiva los mods. Los cambios se aplican al reiniciar el juego.",
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )

            if (mods.isEmpty()) {
                Text(
                    "No hay mods instalados. Coloca cada mod en la carpeta game/mods/<id>/ con su mod.ini y su carpeta data o scripts.",
                    style = MaterialTheme.typography.bodyLarge,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                Text(
                    gameDir.resolve("mods").absolutePath,
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            } else {
                SettingsGroup("Instalados") {
                    mods.forEachIndexed { index, entry ->
                        val subtitle = buildString {
                            if (entry.version.isNotBlank()) append("v").append(entry.version).append("  ·  ")
                            append(entry.id)
                            if (entry.description.isNotBlank()) append("\n").append(entry.description)
                        }
                        SwitchRow(
                            icon = Icons.Filled.Build,
                            title = entry.name,
                            subtitle = subtitle,
                            checked = entry.enabled,
                            items = mods.size,
                            index = index,
                            onCheckedChange = { checked ->
                                setEnabled(entry, checked)
                                mods = readMods(gameDir)
                            },
                        )
                    }
                }
            }
            Spacer(Modifier.height(8.dp))
        }
    }
}
