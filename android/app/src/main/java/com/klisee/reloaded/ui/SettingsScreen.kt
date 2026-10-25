package com.klisee.reloaded.ui

import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Article
import androidx.compose.material.icons.filled.AspectRatio
import androidx.compose.material.icons.filled.CalendarMonth
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material.icons.filled.Fullscreen
import androidx.compose.material.icons.filled.Info
import androidx.compose.material.icons.filled.LightMode
import androidx.compose.material.icons.filled.Language
import androidx.compose.material.icons.filled.Link
import androidx.compose.material.icons.filled.LinkOff
import androidx.compose.material.icons.filled.Opacity
import androidx.compose.material.icons.filled.SportsEsports
import androidx.compose.material.icons.filled.Tune
import androidx.compose.material.icons.filled.SystemUpdate
import androidx.compose.material.icons.filled.Save
import androidx.compose.material.icons.filled.SaveAlt
import androidx.compose.material.icons.filled.SettingsBackupRestore
import androidx.compose.material.icons.filled.Speed
import androidx.compose.material.icons.filled.Storage
import androidx.compose.material.icons.filled.VideogameAsset
import androidx.compose.material3.AlertDialog
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
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.vectorResource
import androidx.compose.ui.unit.dp
import com.klisee.reloaded.GameRepository
import com.klisee.reloaded.R
import com.klisee.reloaded.Preferences
import com.klisee.reloaded.Update
import com.klisee.reloaded.discord.DiscordRpcManager
import kotlinx.coroutines.launch

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SettingsScreen(
    repo: GameRepository,
    prefs: Preferences,
    revision: Int = 0,
    onLog: () -> Unit,
    onAbout: () -> Unit,
    onControls: () -> Unit,
    onDiscord: () -> Unit,
    onTouchSettings: () -> Unit,
    onFpsSettings: () -> Unit,
    onMySaves: () -> Unit,
    onUpdateFound: (Update.Release) -> Unit,
    bottomBar: @Composable () -> Unit = {},
) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val snackbar = remember { SnackbarHostState() }
    var status by remember(revision) { mutableStateOf(repo.status()) }
    var keepScreenOn by remember { mutableStateOf(prefs.keepScreenOn) }
    var keepAspect by remember { mutableStateOf(prefs.keepAspect) }
    var gameLanguage by remember { mutableStateOf(prefs.gameLanguage) }
    var showLanguage by remember { mutableStateOf(false) }
    val discordUser by DiscordRpcManager.user.collectAsState()
    var checking by remember { mutableStateOf(false) }
    val appVersion = remember {
        runCatching { context.packageManager.getPackageInfo(context.packageName, 0).versionName }.getOrNull().orEmpty()
    }
    var confirmingDelete by remember { mutableStateOf(false) }
    val installed = status as? GameRepository.Status.Ready

    if (showLanguage) {
        AlertDialog(
            onDismissRequest = { showLanguage = false },
            title = { Text("Seleccionar idioma") },
            text = {
                Column {
                    listOf(
                        "auto" to ("Automático" to "Según el idioma del teléfono"),
                        "es" to ("Español" to "Texto del juego en español"),
                        "en" to ("English" to "Game text and Rich Presence labels in English"),
                    ).forEachIndexed { index, (mode, titleSubtitle) ->
                        val (title, subtitle) = titleSubtitle
                        ChoiceRow(
                            icon = Icons.Filled.Language,
                            title = title,
                            subtitle = subtitle,
                            selected = gameLanguage == mode,
                            items = 3,
                            index = index,
                            onSelect = {
                                gameLanguage = mode
                                prefs.gameLanguage = mode
                                Strings.update(prefs)
                                if (installed != null) prefs.writeGameLanguage(context.filesDir)
                                DiscordRpcManager.setLauncherScreen(
                                    if (DiscordRpcManager.isEnglish()) "In Settings" else "En los Ajustes",
                                )
                                showLanguage = false
                            },
                        )
                    }
                }
            },
            confirmButton = { TextButton(onClick = { showLanguage = false }) { Text("Cerrar") } },
        )
    }

    if (confirmingDelete) {
        AlertDialog(
            onDismissRequest = { confirmingDelete = false },
            title = { Text("Borrar el juego?") },
            text = { Text("Se borra la copia extraida (${formatBytes(installed?.sizeBytes ?: 0)}). Podras volver a descargarlo o importarlo, pero tus partidas guardadas se pierden.") },
            confirmButton = {
                TextButton(onClick = {
                    confirmingDelete = false
                    scope.launch {
                        repo.deleteGame()
                        status = repo.status()
                    }
                }) { Text("Borrar") }
            },
            dismissButton = { TextButton(onClick = { confirmingDelete = false }) { Text("Cancelar") } },
        )
    }

    Scaffold(
        topBar = { TopAppBar(title = { Text("Ajustes") }) },
        bottomBar = bottomBar,
        // Lifted clear of the floating bar, which otherwise sits on top of the message.
        snackbarHost = { SnackbarHost(snackbar, modifier = Modifier.padding(bottom = BottomNavSpace)) },
        contentWindowInsets = WindowInsets(0.dp),
    ) { padding ->
        Column(
            modifier = Modifier.fillMaxSize().padding(padding).verticalScroll(rememberScrollState()).padding(horizontal = 24.dp),
            verticalArrangement = Arrangement.spacedBy(24.dp),
        ) {
            Spacer(Modifier.height(4.dp))

            SettingsGroup("Juego") {
                if (installed == null) {
                    InfoRow(
                        icon = Icons.Filled.Info,
                        title = "Sin juego instalado",
                        value = "Descargalo o importalo desde la pantalla principal",
                        items = 1,
                        index = 0,
                    )
                } else {
                    InfoRow(Icons.Filled.VideogameAsset, "Version", installed.version ?: "sin datos", items = 4, index = 0)
                    InfoRow(Icons.Filled.Storage, "Tamano en disco", formatBytes(installed.sizeBytes), items = 4, index = 1)
                    InfoRow(Icons.Filled.CalendarMonth, "Instalado el", formatDate(installed.installedAt), items = 4, index = 2)
                    NavRow(
                        icon = Icons.Filled.Delete,
                        title = "Borrar el juego",
                        subtitle = "Libera ${formatBytes(installed.sizeBytes)} del almacenamiento",
                        items = 4,
                        index = 3,
                        onClick = { confirmingDelete = true },
                    )
                }
            }

            SettingsGroup("Idioma") {
                NavRow(
                    icon = Icons.Filled.Language,
                    title = "Seleccionar idioma",
                    subtitle = when (gameLanguage) {
                        "es" -> "Español"
                        "en" -> "English"
                        else -> "Automático"
                    },
                    items = 1,
                    index = 0,
                    onClick = { showLanguage = true },
                )
            }

            SettingsGroup("Partidas") {
                NavRow(
                    icon = Icons.Filled.Save,
                    title = "Mis partidas",
                    subtitle = if (installed == null) {
                        "El juego no esta descargado"
                    } else {
                        "Ve cuales partidas tienes guardadas en el juego"
                    },
                    items = 1,
                    index = 0,
                    enabled = installed != null,
                    onClick = onMySaves,
                )
            }

            SettingsGroup("Controles tactiles") {
                NavRow(
                    icon = Icons.Filled.Tune,
                    title = "Controles tactiles",
                    subtitle = "Estilo, movimiento, transparencia y editores",
                    items = 1,
                    index = 0,
                    onClick = onTouchSettings,
                )
            }

            SettingsGroup("Mando") {
                NavRow(
                    icon = Icons.Filled.SportsEsports,
                    title = "Mando",
                    subtitle = "Elige que boton del mando hace cada tecla, y pruebalo",
                    items = 1,
                    index = 0,
                    onClick = onControls,
                )
            }

            SettingsGroup("Resolucion") {
                ChoiceRow(
                    icon = Icons.Filled.Fullscreen,
                    title = "Rellenar la pantalla",
                    subtitle = "La imagen ocupa todo el movil, aunque se deforme un poco",
                    selected = !keepAspect,
                    items = 2,
                    index = 0,
                    onSelect = {
                        keepAspect = false
                        prefs.keepAspect = false
                        prefs.writeVideo(context.filesDir)
                    },
                )
                ChoiceRow(
                    icon = Icons.Filled.AspectRatio,
                    title = "Mantener la proporcion",
                    subtitle = "El aspecto clasico del PC, con bandas oscuras a los lados",
                    selected = keepAspect,
                    items = 2,
                    index = 1,
                    onSelect = {
                        keepAspect = true
                        prefs.keepAspect = true
                        prefs.writeVideo(context.filesDir)
                    },
                )
            }

            SettingsGroup("FPS") {
                NavRow(
                    icon = Icons.Filled.Speed,
                    title = "FPS",
                    subtitle = "Modifica el framerate del juego a tu gusto, con frames mayores a 30 FPS",
                    items = 1,
                    index = 0,
                    onClick = onFpsSettings,
                )
            }

            SettingsGroup("Pantalla") {
                SwitchRow(
                    icon = Icons.Filled.LightMode,
                    title = "Mantener la pantalla encendida",
                    subtitle = "Evita que el movil se bloquee mientras juegas",
                    checked = keepScreenOn,
                    items = 1,
                    index = 0,
                    onCheckedChange = {
                        keepScreenOn = it
                        prefs.keepScreenOn = it
                    },
                )
            }

            SettingsGroup("Discord") {
                NavRow(
                    icon = ImageVector.vectorResource(R.drawable.ic_discord),
                    title = "Discord",
                    subtitle = discordUser
                        ?: if (prefs.discordEnabled) "Rich Presence activado" else "Conecta tu cuenta y muestra lo que juegas",
                    items = 1,
                    index = 0,
                    onClick = onDiscord,
                )
            }

            SettingsGroup("Actualizaciones") {
                NavRow(
                    icon = Icons.Filled.SystemUpdate,
                    title = "Version de la app: $appVersion",
                    subtitle = "Checa si hay alguna actualizacion disponible de la aplicacion",
                    items = 1,
                    index = 0,
                    enabled = !checking,
                    onClick = {
                        checking = true
                        scope.launch {
                            val found = Update.check(appVersion).getOrNull()
                            checking = false
                            if (found != null) {
                                onUpdateFound(found)
                            } else {
                                snackbar.showSnackbar("Ya tienes la ultima version ($appVersion).")
                            }
                        }
                    },
                )
            }

            SettingsGroup("Ayuda") {
                NavRow(
                    icon = Icons.Filled.Article,
                    title = "Logs",
                    subtitle = "En caso de un error, comparte los logs, me ayudarán a saber qué falló",
                    items = 2,
                    index = 0,
                    onClick = onLog,
                )
                NavRow(
                    icon = Icons.Filled.Info,
                    title = "Acerca de",
                    subtitle = "Version, creditos y aviso",
                    items = 2,
                    index = 1,
                    onClick = onAbout,
                )
            }

            Spacer(Modifier.height(BottomNavSpace))
        }
    }
}
