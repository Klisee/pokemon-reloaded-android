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
import androidx.compose.material.icons.filled.Speed
import androidx.compose.material.icons.filled.Visibility
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
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import com.klisee.reloaded.Preferences

/**
 * Frame-rate options. Kept on their own screen so the main settings list stays short.
 *
 * 30 FPS is the original engine. The higher rates are experimental: the simulation still runs at
 * 30 Hz, only drawing is interpolated. 120 and unlimited are offered only when the panel reports
 * at least 120 Hz.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun FpsScreen(prefs: Preferences, onBack: () -> Unit) {
    val context = LocalContext.current
    var renderMode by remember { mutableStateOf(prefs.renderMode) }
    var showFps by remember { mutableStateOf(prefs.showFps) }
    val detectedHz = remember { prefs.detectedRefreshRate() }
    val supports60 = detectedHz >= 59.5f
    val supports120 = detectedHz >= 119.5f

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("FPS") },
                navigationIcon = {
                    IconButton(onClick = onBack) { Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "Volver") }
                },
            )
        },
    ) { padding ->
        Column(
            modifier = Modifier.padding(padding).fillMaxSize().verticalScroll(rememberScrollState()).padding(horizontal = 16.dp),
            verticalArrangement = Arrangement.spacedBy(20.dp),
        ) {
            Spacer(Modifier.height(4.dp))

            SwitchRow(
                icon = Icons.Filled.Visibility,
                title = "Mostrar FPS en el juego",
                subtitle = "Muestra dentro del juego a cuántos FPS está ejecutándose el juego",
                checked = showFps,
                items = 1,
                index = 0,
                onCheckedChange = {
                    showFps = it
                    prefs.showFps = it
                    prefs.writeVideo(context.filesDir)
                },
            )

            Text(
                "Modifica el framerate del juego a tu gusto, con frames mayores a 30 FPS.",
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )

            SettingsGroup("FPS") {
                ChoiceRow(
                    icon = Icons.Filled.Speed,
                    title = "30 FPS (Original)",
                    subtitle = "Pokémon Reloaded viene originalmente capado a 30 FPS desde hace más de una década por limitaciones de GM8; es la forma clásica y original de jugar el juego.",
                    selected = renderMode == "native",
                    items = 4,
                    index = 0,
                    onSelect = {
                        renderMode = "native"
                        prefs.renderMode = "native"
                        prefs.writeVideo(context.filesDir)
                    },
                )
                ChoiceRow(
                    icon = Icons.Filled.Speed,
                    title = "60 FPS (Experimental)",
                    subtitle = "Gracias a la reimplementación del motor de OpenGMK se puede ahora correr el juego a 60 o más FPS. Por el momento la opción es experimental; cualquier error repórtalo al GitHub.",
                    selected = renderMode == "60",
                    items = 4,
                    index = 1,
                    enabled = supports60,
                    onSelect = {
                        renderMode = "60"
                        prefs.renderMode = "60"
                        prefs.writeVideo(context.filesDir)
                    },
                )
                ChoiceRow(
                    icon = Icons.Filled.Speed,
                    title = "120 FPS (Experimental)",
                    subtitle = "También es posible correrlo a 120 FPS en pantallas compatibles. Si esta opción no te deja habilitarla, significa que tu teléfono no soporta los 120 FPS.",
                    selected = renderMode == "120",
                    items = 4,
                    index = 2,
                    enabled = supports120,
                    onSelect = {
                        renderMode = "120"
                        prefs.renderMode = "120"
                        prefs.writeVideo(context.filesDir)
                    },
                )
                ChoiceRow(
                    icon = Icons.Filled.Speed,
                    title = "FPS sin Límite",
                    subtitle = "Si tu pantalla es de 200 FPS o más, esta opción puede servir: corre el juego sin límite de FPS hasta lo que dé tu pantalla, útil para tener una experiencia de fotogramas altos.",
                    selected = renderMode == "uncapped",
                    items = 4,
                    index = 3,
                    enabled = supports120,
                    onSelect = {
                        renderMode = "uncapped"
                        prefs.renderMode = "uncapped"
                        prefs.writeVideo(context.filesDir)
                    },
                )
            }

            Spacer(Modifier.height(8.dp))
        }
    }
}
