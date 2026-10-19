package com.klisee.reloaded.ui

import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Article
import androidx.compose.material.icons.filled.Download
import androidx.compose.material.icons.filled.FolderOpen
import androidx.compose.material.icons.filled.PlayArrow
import androidx.compose.material.icons.filled.Settings
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ExperimentalMaterial3ExpressiveApi
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearWavyProgressIndicator
import androidx.compose.material3.LoadingIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
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
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import com.klisee.reloaded.Blog
import com.klisee.reloaded.Emulator
import com.klisee.reloaded.GameRepository
import kotlinx.coroutines.launch

/**
 * The home surface is the author's news plus the three things the user can do. Nothing else: the
 * game's own state is expressed through whether "Jugar" is available, not through a status panel.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun HomeScreen(
    repo: GameRepository,
    revision: Int = 0,
    networkGranted: Boolean,
    onRequestNetwork: () -> Unit,
    onLog: () -> Unit,
    onAbout: () -> Unit,
    onLaunch: () -> Unit,
    launching: Boolean = false,
    onSettings: () -> Unit,
    bottomBar: @Composable () -> Unit = {},
) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val snackbar = remember { SnackbarHostState() }
    // Re-read on every revision, so it reflects what happened in the other tabs.
    var status by remember(revision) { mutableStateOf(repo.status()) }
    var busy by remember { mutableStateOf(false) }
    var progress by remember { mutableStateOf<Int?>(null) }
    var post by remember { mutableStateOf<Blog.Post?>(null) }
    var blogError by remember { mutableStateOf<String?>(null) }
    var loadingPost by remember { mutableStateOf(true) }
    var blogAttempt by remember { mutableStateOf(0) }

    LaunchedEffect(blogAttempt, networkGranted) {
        post = null
        blogError = null
        loadingPost = true
        if (!networkGranted) {
            loadingPost = false
            return@LaunchedEffect
        }
        Blog.latest().fold(
            onSuccess = { post = it },
            onFailure = { blogError = it.message },
        )
        loadingPost = false
    }

    val picker = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
        if (uri == null) return@rememberLauncherForActivityResult
        val hadGame = status is GameRepository.Status.Ready
        busy = true
        scope.launch {
            val result = repo.import(uri) { progress = it }
            busy = false
            progress = null
            status = repo.status()
            snackbar.showSnackbar(
                result.fold(
                    {
                        if (hadGame) "Juego actualizado. Tus partidas se conservaron." else "Juego instalado."
                    },
                    { it.message ?: "No pude importar el juego." },
                ),
            )
        }
    }

    val installed = status as? GameRepository.Status.Ready
    // The post page is loaded rather than the mirror directly: the mirror renders as a blank page
    // when opened cold, but its link works when followed from the post, which the auto-click does.
    val downloadUrl = post?.postUrl ?: post?.links?.firstOrNull()?.url
    val postVersion = post?.version

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("Pokemon Reloaded", style = MaterialTheme.typography.titleLarge) },
                actions = {
                    Row(
                        horizontalArrangement = Arrangement.spacedBy(10.dp),
                        modifier = Modifier.padding(end = 6.dp),
                    ) {
                        CircleIconButton(Icons.Filled.Article, "Logs", onLog)
                        CircleIconButton(Icons.Filled.Settings, "Ajustes", onSettings)
                    }
                },
            )
        },
        // Lifted clear of the floating bar, which otherwise sits on top of the message.
        snackbarHost = { SnackbarHost(snackbar, modifier = Modifier.padding(bottom = BottomNavSpace)) },
        bottomBar = bottomBar,
        contentWindowInsets = WindowInsets(0.dp),
    ) { padding ->
        Box(modifier = Modifier.fillMaxSize().padding(padding)) {
            Column(modifier = Modifier.fillMaxSize()) {
                Column(
                    modifier = Modifier.weight(1f).verticalScroll(rememberScrollState()),
                    verticalArrangement = Arrangement.spacedBy(20.dp),
                ) {
                when {
                    loadingPost -> NewsPlaceholder()
                    post != null -> NewsFeed(post!!)
                    !networkGranted -> NoticeCard(
                        text = "La app necesita permiso de red para leer las novedades del blog y descargar el juego.",
                        action = "Conceder permiso",
                        onAction = onRequestNetwork,
                    )
                    blogError != null -> NoticeCard(
                        text = "No pude leer el blog: $blogError",
                        action = "Reintentar",
                        onAction = { blogAttempt++ },
                    )
                    else -> NoticeCard(text = "El blog no devolvio ninguna entrada.", action = "Reintentar", onAction = { blogAttempt++ })
                }
            }

            // The one-handed zone: the three actions live at the bottom, never in the middle.
            Surface(color = MaterialTheme.colorScheme.surfaceContainer) {
                Column(modifier = Modifier.fillMaxWidth().padding(24.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
                    Button(
                        onClick = onLaunch,
                        enabled = installed != null && !busy && !launching,
                        modifier = Modifier.fillMaxWidth().height(72.dp),
                        shape = MaterialTheme.shapes.extraLarge,
                    ) {
                        if (launching) {
                            CircularProgressIndicator(
                                modifier = Modifier.size(24.dp),
                                strokeWidth = 3.dp,
                            )
                        } else {
                            Icon(Icons.Filled.PlayArrow, contentDescription = null, modifier = Modifier.size(28.dp))
                        }
                        Spacer(Modifier.width(12.dp))
                        Text(if (launching) "Cargando..." else "Jugar", style = MaterialTheme.typography.titleLarge)
                    }
                    if (installed == null) {
                        Text(
                            text = "Para jugar hace falta el juego: descargalo del blog o importa el zip que ya tengas.",
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                            textAlign = TextAlign.Center,
                            modifier = Modifier.fillMaxWidth(),
                        )
                    }
                    Row(horizontalArrangement = Arrangement.spacedBy(12.dp), modifier = Modifier.fillMaxWidth()) {
                        TextButton(
                            onClick = { downloadUrl?.let { openInBrowser(context, it) } },
                            enabled = downloadUrl != null && !busy,
                            modifier = Modifier.weight(1f),
                        ) {
                            Icon(Icons.Filled.Download, contentDescription = null, modifier = Modifier.size(20.dp))
                            Spacer(Modifier.width(8.dp))
                            Text(if (postVersion != null) "Descargar $postVersion" else "Descargar")
                        }
                        TextButton(
                            onClick = { picker.launch(arrayOf("application/zip", "application/octet-stream", "*/*")) },
                            enabled = !busy,
                            modifier = Modifier.weight(1f),
                        ) {
                            Icon(Icons.Filled.FolderOpen, contentDescription = null, modifier = Modifier.size(20.dp))
                            Spacer(Modifier.width(8.dp))
                            Text("Importar")
                        }
                    }
                    // The bar hovers over the page, so the surface has to reach the bottom edge for
                    // it to land on this colour instead of on the darker page background.
                    Spacer(Modifier.height(BottomNavSpace))
                }
            }

            ImportPanel(busy = busy, progress = progress)
        }
    }
}
}

/** A small top-bar action on its own dark disc, so the icon reads clearly over the page. */
@Composable
private fun CircleIconButton(icon: ImageVector, description: String, onClick: () -> Unit) {
    IconButton(
        onClick = onClick,
        modifier = Modifier
            .size(40.dp)
            .background(MaterialTheme.colorScheme.surfaceContainerHigh, CircleShape),
    ) {
        Icon(icon, contentDescription = description, modifier = Modifier.size(20.dp))
    }
}

/**
 * The import has no page of its own, so it gets a panel over the page: a modal explains why the
 * buttons are disabled, and the wavy indicator (the one Material 3 Expressive uses) shows how far
 * the extraction has got.
 */
@OptIn(ExperimentalMaterial3ExpressiveApi::class)
@Composable
private fun ImportPanel(busy: Boolean, progress: Int?) {
    if (!busy) return
    Box(modifier = Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
        Surface(color = MaterialTheme.colorScheme.scrim.copy(alpha = 0.5f), modifier = Modifier.fillMaxSize()) {}
        Surface(
            shape = MaterialTheme.shapes.extraLarge,
            color = MaterialTheme.colorScheme.surfaceContainerHigh,
            modifier = Modifier.padding(32.dp),
        ) {
            Column(
                modifier = Modifier.padding(28.dp),
                horizontalAlignment = Alignment.CenterHorizontally,
                verticalArrangement = Arrangement.spacedBy(20.dp),
            ) {
                Text(
                    text = if (progress == null) "Preparando el archivo…" else "Instalando el juego…",
                    style = MaterialTheme.typography.titleMedium,
                    textAlign = TextAlign.Center,
                )
                if (progress == null) {
                    LoadingIndicator()
                } else {
                    LinearWavyProgressIndicator(progress = { progress / 100f }, modifier = Modifier.fillMaxWidth())
                    Text("$progress %", style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
            }
        }
    }
}

/** The author's own post: title, date, the pictures, and what it says. */
@Composable
private fun NewsFeed(post: Blog.Post) {
    val context = LocalContext.current
    Column(verticalArrangement = Arrangement.spacedBy(16.dp)) {
        Column(modifier = Modifier.padding(horizontal = 24.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
            Text(post.title, style = MaterialTheme.typography.headlineSmall)
            if (post.published != null) {
                val millis = runCatching {
                    java.text.SimpleDateFormat("yyyy-MM-dd", java.util.Locale.US).parse(post.published)?.time ?: 0L
                }.getOrDefault(0L)
                Text(
                    text = "Publicado el ${formatDate(millis)}",
                    style = MaterialTheme.typography.labelLarge,
                    color = MaterialTheme.colorScheme.primary,
                )
            }
        }
        if (post.images.isNotEmpty()) {
            LazyRow(
                contentPadding = PaddingValues(horizontal = 24.dp),
                horizontalArrangement = Arrangement.spacedBy(12.dp),
            ) {
                items(post.images.size) { index ->
                    RemoteImage(
                        url = post.images[index],
                        modifier = Modifier
                            .height(200.dp)
                            .width(if (index == 0) 320.dp else 240.dp)
                            .clip(MaterialTheme.shapes.extraLarge),
                    )
                }
            }
        }
        if (post.summary.isNotBlank()) {
            Text(
                text = post.summary,
                style = MaterialTheme.typography.bodyLarge,
                modifier = Modifier.padding(horizontal = 24.dp),
            )
        }
        if (post.postUrl != null) {
            TextButton(
                onClick = { openInBrowser(context, post.postUrl) },
                modifier = Modifier.padding(horizontal = 16.dp),
            ) {
                Text("Leer la entrada completa")
            }
        }
    }
}

@Composable
private fun NewsPlaceholder() {
    Column(
        modifier = Modifier.padding(horizontal = 24.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        Surface(color = MaterialTheme.colorScheme.surfaceContainerHighest, shape = MaterialTheme.shapes.large, modifier = Modifier.fillMaxWidth(0.7f).height(32.dp)) {}
        Surface(color = MaterialTheme.colorScheme.surfaceContainerHighest, shape = MaterialTheme.shapes.extraLarge, modifier = Modifier.fillMaxWidth().height(200.dp)) {}
        Surface(color = MaterialTheme.colorScheme.surfaceContainerHighest, shape = MaterialTheme.shapes.small, modifier = Modifier.fillMaxWidth().height(16.dp)) {}
        Surface(color = MaterialTheme.colorScheme.surfaceContainerHighest, shape = MaterialTheme.shapes.small, modifier = Modifier.fillMaxWidth(0.8f).height(16.dp)) {}
    }
}

@Composable
private fun NoticeCard(text: String, action: String, onAction: () -> Unit) {
    Surface(
        shape = MaterialTheme.shapes.large,
        color = MaterialTheme.colorScheme.surfaceContainerHigh,
        modifier = Modifier.fillMaxWidth().padding(horizontal = 24.dp),
    ) {
        Row(
            modifier = Modifier.padding(start = 24.dp, top = 16.dp, end = 16.dp, bottom = 16.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(text, style = MaterialTheme.typography.bodyMedium, modifier = Modifier.weight(1f))
            TextButton(onClick = onAction) { Text(action) }
        }
    }
}
