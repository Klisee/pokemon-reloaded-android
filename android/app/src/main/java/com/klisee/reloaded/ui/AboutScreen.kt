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
import androidx.compose.material.icons.filled.Brush
import androidx.compose.material.icons.filled.Favorite
import androidx.compose.material.icons.filled.Memory
import androidx.compose.material.icons.filled.Person
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun AboutScreen(onBack: () -> Unit) {
    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("Acerca de") },
                navigationIcon = {
                    IconButton(onClick = onBack) { Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "Volver") }
                },
            )
        },
    ) { padding ->
        Column(
            modifier = Modifier.fillMaxSize().padding(padding).verticalScroll(rememberScrollState()).padding(horizontal = 24.dp),
            verticalArrangement = Arrangement.spacedBy(24.dp),
        ) {
            Spacer(Modifier.height(4.dp))

            Text(
                text = "Pokemon Reloaded",
                style = MaterialTheme.typography.headlineMedium,
                modifier = Modifier.padding(horizontal = 4.dp),
            )
            Text(
                text = "Port no oficial para Android del fangame, hecho con OpenGMK. No emula Windows: lee los datos del juego y los ejecuta directamente.",
                style = MaterialTheme.typography.bodyLarge,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.padding(horizontal = 4.dp),
            )

            SettingsGroup("Quien lo hizo") {
                InfoRow(Icons.Filled.Person, "El juego", "Fer Productions", items = 3, index = 0)
                InfoRow(Icons.Filled.Memory, "El motor", "OpenGMK, reimplementacion en Rust", items = 3, index = 1)
                InfoRow(Icons.Filled.Favorite, "Este port", "Comunidad, sin animo de lucro", items = 3, index = 2)
            }

            SettingsGroup("Licencias y avisos") {
                InfoRow(Icons.Filled.Brush, "Tipografia", "Google Sans Flex, bajo licencia OFL", items = 2, index = 0)
                InfoRow(
                    icon = Icons.Filled.Memory,
                    title = "Marcas",
                    value = "Pokemon es de Nintendo, Game Freak y Creatures. Este proyecto no esta afiliado ni respaldado por ellos.",
                    items = 2,
                    index = 1,
                )
            }

            SettingsGroup("Version") {
                val context = androidx.compose.ui.platform.LocalContext.current
                val version = remember(context) {
                    runCatching {
                        val info = context.packageManager.getPackageInfo(context.packageName, 0)
                        "${info.versionName} (${info.versionCode})"
                    }.getOrDefault("desconocida")
                }
                InfoRow(Icons.Filled.Memory, "Launcher", version, items = 1, index = 0)
            }

            Spacer(Modifier.height(8.dp))
        }
    }
}
