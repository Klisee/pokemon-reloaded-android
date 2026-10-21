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
import androidx.compose.material.icons.filled.Fullscreen
import androidx.compose.material.icons.filled.Opacity
import androidx.compose.material.icons.filled.Settings
import androidx.compose.material.icons.filled.SportsEsports
import androidx.compose.material.icons.filled.Tune
import androidx.compose.material.icons.filled.VideogameAsset
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.Scaffold
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import com.klisee.reloaded.MoveMode
import com.klisee.reloaded.Preferences
import com.klisee.reloaded.TouchStyle
import kotlin.math.roundToInt

/**
 * Every touch-control option, in one place: which overlay is drawn, what moves the player, the look
 * of the buttons, opacity/size and the three editors. The pad keeps its own screen ("Mando").
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun TouchSettingsScreen(prefs: Preferences, onBack: () -> Unit) {
    val context = LocalContext.current
    var move by remember { mutableStateOf(prefs.moveMode) }
    var opacity by remember { mutableStateOf(prefs.touchOpacity) }
    var scale by remember { mutableStateOf(prefs.touchScale) }
    var appearance by remember { mutableStateOf(prefs.touchAppearance) }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("Controles tactiles") },
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

            // Moderno always uses the joystick, so the choice only makes sense for the classic look.
            if (appearance.style == TouchStyle.CLASSIC) {
                SettingsGroup("Movimiento") {
                    MoveMode.entries.forEachIndexed { index, option ->
                        ChoiceRow(
                            icon = Icons.Filled.SportsEsports,
                            title = option.label,
                            subtitle = if (option == MoveMode.Joystick) {
                                "Palanca analogica: mueve en cualquier direccion"
                            } else {
                                "Cruceta de cuatro direcciones, con vibracion al marcar"
                            },
                            selected = move == option,
                            items = MoveMode.entries.size,
                            index = index,
                            onSelect = {
                                move = option
                                prefs.moveMode = option
                            },
                        )
                    }
                }
            }

            SettingsGroup("Estilo visual") {
                TouchStyle.entries.forEachIndexed { index, option ->
                    ChoiceRow(
                        icon = Icons.Filled.VideogameAsset,
                        title = option.title,
                        subtitle = when (option) {
                            TouchStyle.MODERN -> "Sprites semitransparentes y la distribucion por perfiles"
                            TouchStyle.CLASSIC -> "Botones suaves con anillos y etiquetas limpias"
                        },
                        selected = appearance.style == option,
                        items = TouchStyle.entries.size,
                        index = index,
                        onSelect = {
                            appearance = appearance.copy(style = option)
                            prefs.touchAppearance = appearance
                            if (option == TouchStyle.MODERN) {
                                move = MoveMode.Joystick
                                prefs.moveMode = MoveMode.Joystick
                            } else {
                                // Clasico brings back its cruceta.
                                move = MoveMode.Dpad
                                prefs.moveMode = MoveMode.Dpad
                            }
                        },
                    )
                }
            }

            SettingsGroup("Transparencia y tamano") {
                SliderRow(
                    icon = Icons.Filled.Opacity,
                    title = "Opacidad de los botones",
                    subtitle = "${(opacity * 100).roundToInt()} %",
                    value = opacity,
                    items = 2,
                    index = 0,
                    onValueChange = {
                        opacity = it
                        prefs.touchOpacity = it
                    },
                )
                SliderRow(
                    icon = Icons.Filled.Fullscreen,
                    title = "Tamano de los botones",
                    subtitle = "${(scale * 100).roundToInt()} %",
                    value = scale,
                    items = 2,
                    index = 1,
                    onValueChange = {
                        scale = it
                        prefs.touchScale = it
                    },
                )
            }

            SettingsGroup("Editores") {
                NavRow(
                    icon = Icons.Filled.Tune,
                    title = "Posiciones y tamano",
                    subtitle = "Arrastra y redimensiona por perfil (Exploracion, Combate...)",
                    items = 3,
                    index = 0,
                    onClick = { context.startActivity(android.content.Intent(context, com.klisee.reloaded.TouchEditorActivity::class.java)) },
                )
                NavRow(
                    icon = Icons.Filled.VideogameAsset,
                    title = "Sprites y paquetes",
                    subtitle = "Cambia la imagen de un boton o importa un pack",
                    items = 3,
                    index = 1,
                    onClick = { context.startActivity(android.content.Intent(context, com.klisee.reloaded.TouchAssetsActivity::class.java)) },
                )
                NavRow(
                    icon = Icons.Filled.Settings,
                    title = "Animaciones y vibracion",
                    subtitle = "Ajustes finos de la apariencia",
                    items = 3,
                    index = 2,
                    onClick = { context.startActivity(android.content.Intent(context, com.klisee.reloaded.TouchAppearanceActivity::class.java)) },
                )
            }

            Spacer(Modifier.height(24.dp))
        }
    }
}
