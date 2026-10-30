package com.klisee.reloaded.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material3.Button
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import com.klisee.reloaded.GameRepository
import kotlinx.coroutines.launch

/**
 * Reloaded Online is part of the launcher. This screen only stores the startup configuration;
 * the native engine opens the connection before the game starts and draws remote players itself.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ReloadedOnlineScreen(
    repo: GameRepository,
    onBack: () -> Unit,
    onStart: () -> Unit,
) {
    val initial = remember { repo.reloadedOnlineConfig() }
    val installed = remember { repo.status() is GameRepository.Status.Ready }
    var enabled by remember { mutableStateOf(initial.enabled) }
    var server by remember { mutableStateOf(initial.server) }
    var name by remember { mutableStateOf(initial.name) }
    var room by remember { mutableStateOf(initial.room) }
    val snackbar = remember { SnackbarHostState() }
    val scope = rememberCoroutineScope()

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("Reloaded Online") },
                navigationIcon = {
                    IconButton(onClick = onBack) {
                        Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "Volver")
                    }
                },
            )
        },
        snackbarHost = { SnackbarHost(snackbar) },
    ) { padding ->
        Column(
            modifier = Modifier
                .fillMaxSize()
                .padding(padding)
                .verticalScroll(rememberScrollState())
                .padding(horizontal = 24.dp),
            verticalArrangement = Arrangement.spacedBy(16.dp),
        ) {
            Spacer(Modifier.height(8.dp))
            Text(
                text = "Mundo compartido",
                style = MaterialTheme.typography.headlineSmall,
            )
            Text(
                text = "Conecta con otras personas que estén en el mismo mapa. Las posiciones se envían a un servidor propio de Reloaded Online; el juego sigue siendo tuyo y no se modifica.",
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )

            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
            ) {
                Text("Activar Reloaded Online", style = MaterialTheme.typography.titleMedium)
                Switch(checked = enabled, onCheckedChange = { enabled = it })
            }

            OutlinedTextField(
                value = server,
                onValueChange = { server = it },
                label = { Text("Servidor (host:puerto)") },
                supportingText = { Text("Ejemplo: 192.168.1.50:7777") },
                singleLine = true,
                modifier = Modifier.fillMaxWidth(),
            )
            OutlinedTextField(
                value = name,
                onValueChange = { name = it },
                label = { Text("Tu nombre") },
                singleLine = true,
                modifier = Modifier.fillMaxWidth(),
            )
            OutlinedTextField(
                value = room,
                onValueChange = { room = it },
                label = { Text("Sala") },
                singleLine = true,
                modifier = Modifier.fillMaxWidth(),
            )
            Button(
                onClick = {
                    val config = GameRepository.ReloadedOnlineConfig(
                        enabled = enabled,
                        server = server,
                        name = name,
                        room = room,
                        bridgeUpstream = "",
                    )
                    repo.writeReloadedOnline(config).fold(
                        onSuccess = { onStart() },
                        onFailure = { error ->
                            scope.launch { snackbar.showSnackbar(error.message ?: "No pude guardar la configuración.") }
                        },
                    )
                },
                enabled = installed,
                modifier = Modifier.fillMaxWidth().height(56.dp),
            ) {
                Text("Guardar y jugar")
            }
            if (!installed) {
                Text(
                    text = "Primero instala el juego para usar Reloaded Online.",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.error,
                )
            }
            Text(
                text = "El servidor no está incluido en el APK. Ejecuta reloaded-online-server en el PC o en el servidor donde quieras alojar las partidas.",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}
