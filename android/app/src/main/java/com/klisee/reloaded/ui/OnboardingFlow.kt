package com.klisee.reloaded.ui

import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.size
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Download
import androidx.compose.material.icons.filled.FolderOpen
import androidx.compose.material.icons.filled.LightMode
import androidx.compose.material3.Icon
import androidx.compose.material3.LinearWavyProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.LinkAnnotation
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.withLink
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.klisee.reloaded.Blog
import com.klisee.reloaded.GameRepository
import com.klisee.reloaded.Preferences
import kotlinx.coroutines.launch

/**
 * First run only. The reference apps open with a short, purposeful setup, and this one has a real
 * job to do: get the game onto the device before the user reaches a disabled "Jugar".
 */
@Composable
fun Onboarding(
    repo: GameRepository,
    prefs: Preferences,
    onFinish: () -> Unit,
) {
    val context = androidx.compose.ui.platform.LocalContext.current
    var step by remember { mutableStateOf(0) }
    var status by remember { mutableStateOf(repo.status()) }
    var post by remember { mutableStateOf<Blog.Post?>(null) }
    var keepScreenOn by remember { mutableStateOf(prefs.keepScreenOn) }
    var busy by remember { mutableStateOf(false) }
    var progress by remember { mutableStateOf<Int?>(null) }
    var installLog by remember { mutableStateOf(emptyList<String>()) }
    val scope = rememberCoroutineScope()

    LaunchedEffect(Unit) { post = Blog.latest().getOrNull() }
    LaunchedEffect(step) { status = repo.status() }

    val picker = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
        if (uri == null) return@rememberLauncherForActivityResult
        busy = true
        installLog = emptyList()
        progress = null
        scope.launch {
            repo.import(
                uri,
                onProgress = { progress = it },
                onLog = { line -> installLog = (installLog + line).takeLast(20) },
            )
            busy = false
            progress = null
            status = repo.status()
        }
    }

    val installed = status as? GameRepository.Status.Ready
    val downloadUrl = post?.postUrl ?: post?.links?.firstOrNull()?.url
    val postVersion = post?.version

    fun finish() {
        prefs.onboardingDone = true
        onFinish()
    }

    when (step) {
        0 -> OnboardingScaffold(
            step = 0,
            stepCount = 3,
            title = "Pokemon Reloaded en Android",
            subtitle = buildAnnotatedString {
                append(
                    Strings.tr("Un port hecho por la comunidad y para la comunidad, por @kliseex. Corre sobre " +
                        "OpenGMK, una reimplementación nativa del motor de Game Maker 8, así que el " +
                        "juego se ejecuta de verdad: no hay ningún Windows emulado por medio y el " +
                        "rendimiento es prácticamente el de un juego nativo.\n\n" +
                        "Sigue en beta, y eso se nota: PUEDE TENER MUCHOS ERRORES Y FALLOS. Si tienes " +
                        "algún bug, mínimo o grave, por favor guarda un log y repórtalo.\n\n" +
                        "Manda reportes de bugs, crasheos o congelamientos a "),
                )
                withLink(LinkAnnotation.Url("https://github.com/Klisee/pokemon-reloaded-android")) {
                    append("GitHub")
                }
                append(".")
            },
            continueLabel = "Empezar",
            continueEnabled = true,
            onContinue = { step = 1 },
            hero = { OnboardingHero("BIENVENIDO") },
        )

        1 -> OnboardingScaffold(
            step = 1,
            stepCount = 3,
            title = "Antes de empezar",
            subtitle = buildAnnotatedString {
                append(
                    Strings.tr("Para poder jugar a este port en Android necesitas los archivos originales del " +
                        "juego, que están en el blog de "),
                )
                withLink(LinkAnnotation.Url("https://pokemon-reloaded.blogspot.com/")) {
                    append("F3R")
                }
                append(
                    Strings.tr(". Este APK no trae ningún asset del juego, y eso es lo que mantiene el proyecto " +
                        "del código abierto.\n\n" +
                        "Para empezar: dale a Descargar del blog, consigue el juego en un .zip y " +
                        "después usa Importar un zip."),
                )
            },
            continueLabel = "Continuar",
            continueEnabled = installed != null,
            onContinue = { step = 2 },
            onBack = { step = 0 },
            hint = if (installed == null) "Instala el juego para poder continuar" else "Listo: ${installed.exe}",
        ) {
            BigActionRow(
                icon = Icons.Filled.Download,
                title = "Descargar del blog",
                subtitle = if (postVersion != null) "Version $postVersion" else "Abre la pagina de descarga",
                items = 2,
                index = 0,
                enabled = downloadUrl != null && !busy,
                onClick = { downloadUrl?.let { openInBrowser(context, it) } },
            )
            BigActionRow(
                icon = Icons.Filled.FolderOpen,
                title = "Importar un zip",
                subtitle = "Elige el archivo que ya tengas",
                items = 2,
                index = 1,
                enabled = !busy,
                onClick = { picker.launch(arrayOf("application/zip", "application/octet-stream", "*/*")) },
            )
            if (busy) {
                LinearWavyProgressIndicator(progress = { (progress ?: 0) / 100f }, modifier = Modifier.fillMaxWidth())
                Text(
                    text = installLog.takeLast(3).joinToString("\n").ifEmpty { "Comprobando el archivo…" },
                    style = MaterialTheme.typography.bodySmall,
                    fontFamily = FontFamily.Monospace,
                    fontSize = 10.sp,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }

        else -> OnboardingScaffold(
            step = 2,
            stepCount = 3,
            title = "Como quieres jugar",
            subtitle = AnnotatedString("Puedes cambiarlo cuando quieras desde Ajustes."),
            continueLabel = "Listo",
            continueEnabled = true,
            onContinue = { finish() },
            onBack = { step = 1 },
        ) {
            SwitchRow(
                icon = Icons.Filled.LightMode,
                title = "Mantener la pantalla encendida",
                subtitle = "Evita que el movil se bloquee durante una batalla larga",
                checked = keepScreenOn,
                items = 1,
                index = 0,
                onCheckedChange = {
                    keepScreenOn = it
                    prefs.keepScreenOn = it
                },
            )
            Text(
                text = "Los controles tactiles aparecen solos: joystick y botones en el mundo y en combate, y ninguno en los menus, que se tocan directamente.",
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}

@Composable
private fun BigActionRow(
    icon: ImageVector,
    title: String,
    subtitle: String,
    items: Int,
    index: Int,
    enabled: Boolean,
    onClick: () -> Unit,
) {
    GroupListItem(
        headlineContent = { Text(title, style = MaterialTheme.typography.titleMedium) },
        supportingContent = { Text(subtitle, style = MaterialTheme.typography.bodyMedium) },
        leadingContent = { Icon(icon, contentDescription = null, modifier = Modifier.size(28.dp)) },
        items = items,
        index = index,
        onClick = if (enabled) onClick else null,
    )
}
