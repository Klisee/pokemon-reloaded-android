package com.klisee.reloaded.ui

import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material.icons.filled.FileDownload
import androidx.compose.material.icons.filled.FileUpload
import androidx.compose.material.icons.filled.Save
import androidx.compose.material.icons.filled.Warning
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import com.klisee.reloaded.GameRepository
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

/**
 * The game's save slots, read from the summaries the game writes beside each save.
 *
 * It is the same information its own "Cargar" screen shows, brought out where it can be looked at
 * without starting the game - including which slots are still empty.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun MySavesScreen(repo: GameRepository, onBack: () -> Unit) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val snackbar = remember { SnackbarHostState() }
    var slots by remember { mutableStateOf<List<GameRepository.SaveSlot>?>(null) }
    var revision by remember { mutableStateOf(0) }
    // Which slot a pending export belongs to: the picker answers later, and only gives a file.
    var exporting by remember { mutableStateOf<Int?>(null) }
    var importing by remember { mutableStateOf<Int?>(null) }
    // Deleting asks twice, and the second time says plainly what it costs.
    var askFirst by remember { mutableStateOf<Int?>(null) }
    var askSecond by remember { mutableStateOf<Int?>(null) }

    fun deleteSlot(index: Int) {
        askSecond = null
        scope.launch {
            val result = withContext(Dispatchers.IO) { repo.deleteSlot(index) }
            snackbar.showSnackbar(
                result.fold({ "Partida $index borrada." }, { "No pude borrar: ${it.message}" }),
            )
            if (result.isSuccess) {
                // Empty the card the moment the save is gone, rather than waiting for the re-read:
                // the list used to sit there showing a save that no longer existed.
                slots = slots?.map {
                    if (it.index == index) GameRepository.SaveSlot(index, null, null, null, null) else it
                }
            }
            revision++
        }
    }

    askFirst?.let { index ->
        AlertDialog(
            onDismissRequest = { askFirst = null },
            icon = { Icon(Icons.Filled.Delete, contentDescription = null) },
            title = { Text("Borrar la partida") },
            text = { Text("¿Estas seguro de que quieres borrar tu partida?") },
            confirmButton = {
                TextButton(onClick = {
                    askFirst = null
                    askSecond = index
                }) { Text("Si") }
            },
            dismissButton = { TextButton(onClick = { askFirst = null }) { Text("No") } },
        )
    }

    askSecond?.let { index ->
        AlertDialog(
            onDismissRequest = { askSecond = null },
            icon = { Icon(Icons.Filled.Warning, contentDescription = null) },
            title = { Text("ADVERTENCIA") },
            text = {
                Text(
                    "ESTAS A PUNTO DE BORRAR TU PARTIDA PARA SIEMPRE, ¿ESTAS SEGURO DE QUE ESTAS " +
                        "HACIENDO? SI BORRAS TU PARTIDA SE PERDERA PARA SIEMPRE (ESO ES MUCHO TIEMPO) " +
                        "a no ser que tengas una copia de seguridad.",
                )
            },
            confirmButton = { TextButton(onClick = { deleteSlot(index) }) { Text("Estoy seguro") } },
            dismissButton = { TextButton(onClick = { askSecond = null }) { Text("Mejor no") } },
        )
    }

    LaunchedEffect(revision) {
        slots = withContext(Dispatchers.IO) { repo.saveSlots() }
    }

    val save = rememberLauncherForActivityResult(ActivityResultContracts.CreateDocument("application/zip")) { uri ->
        val index = exporting
        exporting = null
        if (uri == null || index == null) return@rememberLauncherForActivityResult
        scope.launch {
            val file = repo.exportSlot(index).getOrElse {
                snackbar.showSnackbar("No pude exportar: ${it.message}")
                return@launch
            }
            val copied = withContext(Dispatchers.IO) {
                runCatching {
                    context.contentResolver.openOutputStream(uri)?.use { out ->
                        file.inputStream().use { it.copyTo(out) }
                    } ?: error("No pude abrir el destino.")
                }
            }
            file.delete()
            snackbar.showSnackbar(if (copied.isSuccess) "Partida $index exportada." else "No pude guardar el archivo.")
        }
    }

    val restore = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
        val index = importing
        importing = null
        if (uri == null || index == null) return@rememberLauncherForActivityResult
        scope.launch {
            val result = withContext(Dispatchers.IO) { repo.importSlot(uri, index) }
            snackbar.showSnackbar(
                result.fold(
                    { "Partida importada en el Slot $index." },
                    { "No pude importar: ${it.message}" },
                ),
            )
            revision++
        }
    }

    val saveAll = rememberLauncherForActivityResult(ActivityResultContracts.CreateDocument("application/zip")) { uri ->
        if (uri == null) return@rememberLauncherForActivityResult
        scope.launch {
            val file = repo.exportSaves().getOrElse {
                snackbar.showSnackbar("No pude exportar: ${it.message}")
                return@launch
            }
            val copied = withContext(Dispatchers.IO) {
                runCatching {
                    context.contentResolver.openOutputStream(uri)?.use { out ->
                        file.inputStream().use { it.copyTo(out) }
                    } ?: error("No pude abrir el destino.")
                }
            }
            file.delete()
            snackbar.showSnackbar(if (copied.isSuccess) "Todo exportado." else "No pude guardar el archivo.")
        }
    }

    val restoreEverything = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
        if (uri == null) return@rememberLauncherForActivityResult
        scope.launch {
            val result = withContext(Dispatchers.IO) { repo.importSaves(uri) }
            snackbar.showSnackbar(result.fold({ "Partidas, equipos y personajes restaurados." }, { "No pude importar: ${it.message}" }))
            revision++
        }
    }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("Mis partidas") },
                navigationIcon = {
                    IconButton(onClick = onBack) { Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "Volver") }
                },
            )
        },
        snackbarHost = { SnackbarHost(snackbar, modifier = Modifier.padding(bottom = BottomNavSpace)) },
        contentWindowInsets = WindowInsets(0.dp),
    ) { padding ->
        Column(
            modifier = Modifier.padding(padding).fillMaxSize().verticalScroll(rememberScrollState()).padding(16.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            val list = slots
            if (list == null) {
                Text("Leyendo las partidas...", style = MaterialTheme.typography.bodyMedium)
                return@Column
            }

            list.forEach { slot ->
                SaveSlotCard(
                    slot = slot,
                    onExport = {
                        exporting = slot.index
                        save.launch("partida${slot.index}.zip")
                    },
                    onImport = {
                        importing = slot.index
                        restore.launch(arrayOf("application/zip", "application/octet-stream", "*/*"))
                    },
                    onDelete = { askFirst = slot.index },
                )
            }

            // One slot at a time is the export and import on each card. This is the whole set: the
            // teams and the custom characters live outside the slots, so they need their own way in
            // and out. One under the other, because side by side the longer label wrapped and left
            // the two buttons uneven.
            Column {
                TextButton(
                    onClick = { saveAll.launch("partidas-reloaded.zip") },
                    modifier = Modifier.fillMaxWidth(),
                ) { Text("Exportar todas las partidas") }
                TextButton(
                    onClick = { restoreEverything.launch(arrayOf("application/zip", "application/octet-stream", "*/*")) },
                    modifier = Modifier.fillMaxWidth(),
                ) { Text("Importar todas las partidas") }
            }

            Spacer(Modifier.height(BottomNavSpace))
        }
    }
}

/** One slot: what the game knows about it, and what can be done with it. */
@Composable
private fun SaveSlotCard(
    slot: GameRepository.SaveSlot,
    onExport: () -> Unit,
    onImport: () -> Unit,
    onDelete: () -> Unit,
) {
    val filled = slot.name != null
    Card(
        modifier = Modifier.fillMaxWidth(),
        shape = MaterialTheme.shapes.extraLarge,
        colors = CardDefaults.cardColors(
            containerColor = if (filled) {
                MaterialTheme.colorScheme.surfaceContainerHigh
            } else {
                MaterialTheme.colorScheme.surfaceContainerLow
            },
        ),
    ) {
        Column(modifier = Modifier.padding(20.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Icon(
                    imageVector = Icons.Filled.Save,
                    contentDescription = null,
                    modifier = Modifier.size(24.dp),
                    tint = MaterialTheme.colorScheme.primary,
                )
                Spacer(Modifier.size(12.dp))
                Text(
                    text = "Slot ${slot.index}",
                    style = MaterialTheme.typography.titleLarge,
                    color = MaterialTheme.colorScheme.onSurface,
                )
                Spacer(Modifier.weight(1f))
                // Only a slot with something in it can be deleted.
                if (filled) {
                    IconButton(onClick = onDelete) {
                        Icon(
                            imageVector = Icons.Filled.Delete,
                            contentDescription = "Borrar la partida",
                            tint = MaterialTheme.colorScheme.error,
                        )
                    }
                }
            }

            Spacer(Modifier.height(12.dp))

            if (!filled) {
                Text(
                    text = "Ranura vacia",
                    style = MaterialTheme.typography.bodyLarge,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            } else {
                Text(slot.name.orEmpty(), style = MaterialTheme.typography.headlineSmall)
                Spacer(Modifier.height(8.dp))
                Text("Vistos: ${slot.seen ?: "?"}", style = MaterialTheme.typography.bodyLarge)
                Text("Capturados: ${slot.caught ?: "?"}", style = MaterialTheme.typography.bodyLarge)
                Text("Localizacion: ${slot.place ?: "?"}", style = MaterialTheme.typography.bodyLarge)
            }

            Spacer(Modifier.height(8.dp))

            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                TextButton(onClick = onExport, enabled = filled) {
                    Icon(Icons.Filled.FileDownload, contentDescription = null, modifier = Modifier.size(20.dp))
                    Spacer(Modifier.size(8.dp))
                    Text("Exportar partida")
                }
                TextButton(onClick = onImport) {
                    Icon(Icons.Filled.FileUpload, contentDescription = null, modifier = Modifier.size(20.dp))
                    Spacer(Modifier.size(8.dp))
                    Text("Importar partida")
                }
            }
        }
    }
}
