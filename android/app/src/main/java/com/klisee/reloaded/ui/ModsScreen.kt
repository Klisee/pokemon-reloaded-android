package com.klisee.reloaded.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Build
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Button
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
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import java.io.File
import java.util.zip.ZipInputStream

private data class ModEntry(
    val id: String,
    val name: String,
    val version: String,
    val author: String,
    val description: String,
    val enabled: Boolean,
    val manifest: File,
)

/** Friendly text for the mods that ship with the app; also covers installs made before the
 *  manifests gained these fields. Imported mods use the author/description from their manifest. */
private val bundledModInfo = mapOf(
    "crt" to ("Klisee" to "Este mod permite darle al juego el aspecto de un monitor CRT, con líneas de escaneo, curvatura y un brillo cálido."),
    "gb" to ("Klisee" to "Este mod permite jugar con el aspecto y el sonido de una Game Boy, con colores limitados y audio de 8 bits."),
)

private fun subtitleFor(entry: ModEntry): String {
    val bundled = bundledModInfo[entry.id]
    val author = entry.author.ifBlank { bundled?.first.orEmpty() }
    val description = entry.description.ifBlank { bundled?.second.orEmpty() }
    return buildString {
        if (author.isNotBlank()) append("De ").append(author)
        if (description.isNotBlank()) {
            if (isNotEmpty()) append('\n')
            append(description)
        }
        if (isEmpty()) append(entry.id)
    }
}

private fun modsEnabled(gameDir: File): Boolean =
    File(gameDir, "mods/enabled.flag").isFile() &&
        !File(gameDir, "mods/disabled.flag").isFile()

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
                author = values["author"].orEmpty(),
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


private fun importModZip(context: android.content.Context, uri: android.net.Uri, gameDir: File): String? {
    val modsDir = File(gameDir, "mods").apply { mkdirs() }
    val temp = File(context.cacheDir, "reloaded-mod.zip")
    val staging = File(context.cacheDir, "reloaded-mod-${System.currentTimeMillis()}").apply { mkdirs() }
    return try {
        context.contentResolver.openInputStream(uri)?.use { input ->
            temp.outputStream().use { output -> input.copyTo(output) }
        } ?: return "No pude abrir el archivo"
        ZipInputStream(temp.inputStream()).use { zip ->
            while (true) {
                val entry = zip.nextEntry ?: break
                val name = entry.name.replace('\\', '/')
                if (name.startsWith("/") || name.split('/').any { it == ".." }) {
                    return "El ZIP contiene una ruta no permitida"
                }
                val target = File(staging, name)
                if (entry.isDirectory) {
                    target.mkdirs()
                } else {
                    target.parentFile?.mkdirs()
                    target.outputStream().use { output -> zip.copyTo(output) }
                }
                zip.closeEntry()
            }
        }
        val root = when {
            File(staging, "mod.ini").isFile -> staging
            else -> staging.listFiles().orEmpty()
                .filter { it.isDirectory && File(it, "mod.ini").isFile }
                .singleOrNull()
                ?: return "El ZIP debe contener un mod.ini"
        }
        val fallback = uri.lastPathSegment
            ?.substringAfterLast('/')
            ?.substringBeforeLast('.')
            ?.ifBlank { null }
            ?: "mod"
        val id = (if (root == staging) fallback else root.name)
            .replace(Regex("[^A-Za-z0-9._-]"), "_")
            .ifBlank { "mod" }
        val destination = File(modsDir, id)
        if (destination.exists()) destination.deleteRecursively()
        root.copyRecursively(destination, overwrite = true)
        null
    } catch (error: Exception) {
        "No pude importar el mod: ${error.message}"
    } finally {
        temp.delete()
        staging.deleteRecursively()
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ModsScreen(filesDir: File, onBack: () -> Unit) {
    val context = LocalContext.current
    val gameDir = File(filesDir, "game")
    var mods by remember { mutableStateOf(readMods(gameDir)) }
    var masterEnabled by remember { mutableStateOf(modsEnabled(gameDir)) }
    var importError by remember { mutableStateOf<String?>(null) }
    val importZip = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
        if (uri != null) {
            importError = importModZip(context, uri, gameDir)
            mods = readMods(gameDir)
        }
    }

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
            SettingsGroup("General") {
                SwitchRow(
                    icon = Icons.Filled.Build,
                    title = "Activar mods",
                    subtitle = if (masterEnabled) "Los mods se cargan al iniciar el juego" else "El juego arranca sin mods",
                    checked = masterEnabled,
                    items = 1,
                    index = 0,
                    onCheckedChange = { checked ->
                        masterEnabled = checked
                        val enabledFlag = File(gameDir, "mods/enabled.flag")
                        val disabledFlag = File(gameDir, "mods/disabled.flag")
                        enabledFlag.parentFile?.mkdirs()
                        if (checked) {
                            disabledFlag.delete()
                            enabledFlag.writeText("1")
                        } else {
                            enabledFlag.delete()
                            disabledFlag.writeText("1")
                        }
                        mods = readMods(gameDir)
                    },
                )
            }

            if (masterEnabled) {
                Button(
                    onClick = { importZip.launch(arrayOf("application/zip", "application/x-zip-compressed", "application/octet-stream")) },
                    modifier = Modifier.fillMaxWidth(),
                ) {
                    Text("Importar mod .zip")
                }
                importError?.let {
                    Text(it, color = MaterialTheme.colorScheme.error)
                }
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
                            SwitchRow(
                                icon = Icons.Filled.Build,
                                title = entry.name,
                                subtitle = subtitleFor(entry),
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
            }
            Spacer(Modifier.height(8.dp))
        }
    }
}
