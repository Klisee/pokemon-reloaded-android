package com.klisee.reloaded.ui

import android.graphics.BitmapFactory
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Link
import androidx.compose.material.icons.filled.Refresh
import androidx.compose.material.icons.filled.SportsEsports
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Checkbox
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.Surface
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import androidx.compose.ui.res.painterResource
import androidx.core.content.ContextCompat
import androidx.core.graphics.drawable.toBitmap
import com.klisee.reloaded.Preferences
import com.klisee.reloaded.R
import com.klisee.reloaded.discord.DiscordConfig
import com.klisee.reloaded.discord.DiscordRpcManager
import kotlinx.coroutines.launch
import java.io.File

/**
 * The Discord submenu: account, the Rich Presence switch and a preview of the card as Discord
 * shows it. Keeping it on its own screen leaves the settings list readable.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun DiscordScreen(prefs: Preferences, onBack: () -> Unit) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val snackbar = remember { SnackbarHostState() }
    var enabled by remember { mutableStateOf(prefs.discordEnabled) }
    val user by DiscordRpcManager.user.collectAsState()
    val status by DiscordRpcManager.status.collectAsState()
    val route by DiscordRpcManager.route.collectAsState()
    val character by DiscordRpcManager.characterName.collectAsState()
    val characterFemale by DiscordRpcManager.characterFemale.collectAsState()
    val launcherScreen by DiscordRpcManager.launcherScreen.collectAsState()
    val playerName by DiscordRpcManager.playerName.collectAsState()
    val head by DiscordRpcManager.headFile.collectAsState()
    var confirmLogin by remember { mutableStateOf(false) }
    // The notice lives at the top of the screen and stays there until it is explicitly checked.
    var warningVisible by remember { mutableStateOf(!prefs.discordWarningDismissed) }
    val configured = DiscordConfig.isConfigured
    val authorized = DiscordRpcManager.isAuthorized()
    val subtitle = when {
        !configured -> "Falta configurar la aplicación de Discord"
        !authorized -> "Vincula tu cuenta para empezar"
        !enabled -> "Muestra en tu perfil que estás jugando"
        status == DiscordRpcManager.Status.Connected -> "Conectado a Discord"
        status == DiscordRpcManager.Status.Connecting -> "Conectando…"
        status == DiscordRpcManager.Status.Error -> "Error de conexión"
        else -> "Desconectado"
    }

    if (confirmLogin) {
        AlertDialog(
            onDismissRequest = { confirmLogin = false },
            title = { Text("Vincular Discord") },
            text = {
                Text(
                    "Se abrirá el login de Discord y la app usará el token de tu cuenta para mostrar tu actividad. " +
                        "Eso va contra los Términos de Discord y la cuenta podría ser suspendida: usa una cuenta secundaria.\n\n" +
                        "¿Quieres continuar?",
                )
            },
            confirmButton = {
                TextButton(onClick = {
                    confirmLogin = false
                    DiscordRpcManager.login(context) { ok ->
                        if (ok) DiscordRpcManager.startIfEnabled()
                        scope.launch {
                            snackbar.showSnackbar(if (ok) "Cuenta de Discord vinculada." else "No se pudo vincular la cuenta.")
                        }
                    }
                }) { Text("Continuar") }
            },
            dismissButton = { TextButton(onClick = { confirmLogin = false }) { Text("Cancelar") } },
        )
    }

    Scaffold(
        // The whole screen starts below the status bar, so the header never runs under the clock.
        modifier = Modifier.statusBarsPadding(),
        contentWindowInsets = WindowInsets(0.dp),
        topBar = {
            TopAppBar(
                title = { Text("Discord") },
                navigationIcon = {
                    IconButton(onClick = onBack) {
                        Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "Volver")
                    }
                },
                windowInsets = WindowInsets(0.dp),
            )
        },
        snackbarHost = { SnackbarHost(snackbar) },
    ) { padding ->
        Box(Modifier.fillMaxSize().padding(padding), contentAlignment = Alignment.TopCenter) {
        Column(
            modifier = Modifier
                .widthIn(max = 640.dp)
                .fillMaxWidth()
                .fillMaxHeight()
                .verticalScroll(rememberScrollState())
                .navigationBarsPadding()
                .padding(horizontal = 16.dp),
            verticalArrangement = Arrangement.spacedBy(20.dp),
        ) {
            if (warningVisible) {
                Surface(
                    shape = RoundedCornerShape(20.dp),
                    color = MaterialTheme.colorScheme.surfaceContainerHigh,
                    modifier = Modifier.fillMaxWidth(),
                ) {
                    Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                        Text("Aviso: Rich Presence con tu cuenta", style = MaterialTheme.typography.titleMedium)
                        Text(
                            "Esto usa un método no oficial de Kizzy (dead8309). Puede haber riesgos, como una suspensión, pero no se han reportado cuentas cerradas por usar clientes alternos. Úsalo bajo tu responsabilidad: el token se guarda cifrado y puedes cerrar sesión cuando quieras.\n\nBasado en KizzyRPC.",
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                        TextButton(onClick = { openInBrowser(context, "https://github.com/dead8309/KizzyRPC") }) {
                            Text("Ver el proyecto de Kizzy")
                        }
                        Row(verticalAlignment = Alignment.CenterVertically) {
                            Checkbox(
                                checked = false,
                                onCheckedChange = { checked ->
                                    if (checked) {
                                        prefs.discordWarningDismissed = true
                                        warningVisible = false
                                    }
                                },
                            )
                            Text("No volver a mostrar")
                        }
                    }
                }
            }

            SettingsGroup("Cuenta") {
                Surface(
                    shape = RoundedCornerShape(20.dp),
                    color = MaterialTheme.colorScheme.surfaceContainerHigh,
                    modifier = Modifier.fillMaxWidth(),
                ) {
                    Row(Modifier.padding(16.dp), verticalAlignment = Alignment.CenterVertically) {
                        Column(Modifier.weight(1f)) {
                            Text(user ?: "Sin vincular", style = MaterialTheme.typography.titleMedium)
                            Text(
                                if (authorized) "Autorizado" else "Conecta tu cuenta para empezar",
                                style = MaterialTheme.typography.bodyMedium,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                        }
                        TextButton(
                            enabled = configured,
                            onClick = {
                                if (authorized) {
                                    DiscordRpcManager.logout()
                                    enabled = false
                                    prefs.discordEnabled = false
                                    scope.launch { snackbar.showSnackbar(Strings.tr("Cuenta desvinculada.")) }
                                } else {
                                    confirmLogin = true
                                }
                            },
                        ) { Text(if (authorized) "Cerrar sesión" else "Vincular cuenta") }
                    }
                }
            }

            SettingsGroup("Rich presence") {
                SwitchRow(
                    icon = Icons.Filled.Link,
                    title = "Activar Discord Rich Presence",
                    subtitle = subtitle,
                    checked = enabled,
                    items = 3,
                    index = 0,
                    onCheckedChange = { on ->
                        enabled = on
                        prefs.discordEnabled = on
                        if (!on) {
                            DiscordRpcManager.disconnect()
                        } else if (!DiscordRpcManager.isAuthorized()) {
                            confirmLogin = true
                        } else {
                            DiscordRpcManager.startIfEnabled()
                        }
                    },
                )
                DiscordPreview(
                    route = route,
                    character = character,
                    playerName = playerName,
                    head = head,
                    female = characterFemale,
                    launcherScreen = launcherScreen,
                )
                NavRow(
                    icon = Icons.Filled.Refresh,
                    title = "Actualizar RPC",
                    subtitle = "Reenvía tu estado actual a Discord",
                    items = 3,
                    index = 2,
                    enabled = authorized && enabled,
                    onClick = {
                        DiscordRpcManager.republish()
                        scope.launch { snackbar.showSnackbar(Strings.tr("Presencia actualizada.")) }
                    },
                )
            }

            Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
                Text(
                    "El Rich Presence usa el token de tu cuenta. Esa práctica va contra los Términos de Discord; usa una cuenta secundaria.",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                Text(
                    "Créditos: KizzyRPC, de Kizzy (dead8309), la base de este método.",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
        }
    }
}

/** How the activity will read on Discord: large image, title, route and character. */
@Composable
private fun DiscordPreview(
    route: String?,
    character: String?,
    playerName: String?,
    head: File?,
    female: Boolean,
    launcherScreen: String?,
) {
    val bitmap = remember(head?.path, head?.lastModified()) {
        head?.let { BitmapFactory.decodeFile(it.path)?.asImageBitmap() }
    }
    val context = LocalContext.current
    val appIcon = remember {
        runCatching {
            ContextCompat.getDrawable(context, R.mipmap.ic_launcher)?.toBitmap(96, 96)?.asImageBitmap()
        }.getOrNull()
    }
    Surface(
        shape = RoundedCornerShape(20.dp),
        color = MaterialTheme.colorScheme.surfaceContainerHigh,
        modifier = Modifier.fillMaxWidth(),
    ) {
        Row(Modifier.padding(14.dp), verticalAlignment = Alignment.CenterVertically) {
            Box(
                modifier = Modifier
                    .size(72.dp)
                    .clip(RoundedCornerShape(16.dp))
                    .background(androidx.compose.ui.graphics.Color.Black),
                contentAlignment = Alignment.Center,
            ) {
                // Discord uses the application icon when no large_image is sent. This is that exact
                // icon (Discord's CDN); the local app icon stays underneath as a fallback.
                if (appIcon != null) {
                    Image(appIcon, contentDescription = null, modifier = Modifier.fillMaxSize(), contentScale = ContentScale.Crop)
                }
                RemoteImage(
                    DiscordConfig.APP_ICON_URL,
                    modifier = Modifier.fillMaxSize(),
                    contentScale = ContentScale.Crop,
                )
                if (bitmap != null) {
                    Image(
                        bitmap,
                        contentDescription = null,
                        modifier = Modifier
                            .align(Alignment.BottomEnd)
                            .size(26.dp)
                            .clip(androidx.compose.foundation.shape.CircleShape),
                        contentScale = ContentScale.Crop,
                    )
                }
            }
            Spacer(Modifier.width(14.dp))
            Column(Modifier.weight(1f)) {
                Text("Jugando a Pokémon Reloaded", style = MaterialTheme.typography.titleMedium)
                Text(
                    route ?: DiscordConfig.FALLBACK_DETAILS,
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                Text(
                    playerName?.let { "${if (female) "Jugadora" else "Jugador"}: $it" }
                        ?: launcherScreen
                        ?: character
                        ?: DiscordConfig.FALLBACK_STATE,
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
    }
}
