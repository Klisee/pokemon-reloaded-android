package com.klisee.reloaded.ui

import android.Manifest
import android.content.pm.PackageManager
import androidx.activity.compose.BackHandler
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.pager.HorizontalPager
import androidx.compose.foundation.pager.rememberPagerState
import androidx.compose.material3.ExperimentalMaterial3ExpressiveApi
import androidx.compose.material3.LoadingIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import androidx.core.content.ContextCompat
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.lifecycle.compose.LocalLifecycleOwner
import com.klisee.reloaded.Emulator
import com.klisee.reloaded.GameRepository
import com.klisee.reloaded.Preferences
import com.klisee.reloaded.Update
import com.klisee.reloaded.discord.DiscordRpcManager
import com.klisee.reloaded.installUpdate
import kotlinx.coroutines.launch

/** Screens reached from a tab rather than being tabs themselves. */
private enum class Pushed { About, Controls, Discord, TouchSettings, Fps, TouchLayout, MySaves, Mods, ReloadedOnline }

@OptIn(ExperimentalMaterial3ExpressiveApi::class)
@Composable
fun App(repo: GameRepository, prefs: Preferences) {
    var onboarded by remember { mutableStateOf(prefs.onboardingDone) }
    var tab by remember { mutableStateOf(Tab.Home) }
    var pushed by remember { mutableStateOf<Pushed?>(null) }
    // Bumped whenever a tab comes into view, so the pages re-read the game's state. Without it,
    // deleting the game in Ajustes left "Jugar" enabled back in Inicio.
    var revision by remember { mutableStateOf(0) }
    // The engine spends several seconds loading the game's data behind a window the size of a stamp.
    // This covers that gap with something that says what is happening, and goes away by itself when
    // the launcher comes back to the front.
    var launching by remember { mutableStateOf(false) }
    val scope = rememberCoroutineScope()

    val lifecycleOwner = LocalLifecycleOwner.current
    var resumeTick by remember { mutableStateOf(0) }
    DisposableEffect(lifecycleOwner) {
        val observer = LifecycleEventObserver { _, event ->
            if (event == Lifecycle.Event.ON_RESUME) {
                launching = false
                resumeTick++
            }
        }
        lifecycleOwner.lifecycle.addObserver(observer)
        onDispose { lifecycleOwner.lifecycle.removeObserver(observer) }
    }

    // Swiping between the tabs is more comfortable than aiming at a button, so the three of them
    // are pages of a pager. The bar and the pager are kept in step in both directions.
    val pagerState = rememberPagerState { Tab.entries.size }
    LaunchedEffect(pagerState) {
        snapshotFlow { pagerState.currentPage }.collect { page ->
            tab = Tab.entries[page]
            revision++
        }
    }

    // Android 17 does not auto-grant INTERNET, so it has to be requested at runtime or the app
    // cannot open a single socket (socket() fails with EPERM).
    val context = LocalContext.current
    var network by remember {
        mutableStateOf(ContextCompat.checkSelfPermission(context, Manifest.permission.INTERNET) == PackageManager.PERMISSION_GRANTED)
    }
    val requestNetwork = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { granted ->
        network = granted
    }
    LaunchedEffect(Unit) {
        if (!network) requestNetwork.launch(Manifest.permission.INTERNET)
        // Make sure the bundled MIDI soundfont is next to the game (copied on first run).
        repo.prepare()
    }

    // A newer build, offered once per run. Nothing is downloaded until the player says so.
    var update by remember { mutableStateOf<Update.Release?>(null) }
    var updating by remember { mutableStateOf(false) }
    var written by remember { mutableStateOf(0L) }
    var total by remember { mutableStateOf(0L) }
    var problem by remember { mutableStateOf<String?>(null) }

    LaunchedEffect(Unit) {
        val installed = runCatching {
            context.packageManager.getPackageInfo(context.packageName, 0).versionName
        }.getOrNull().orEmpty()
        if (installed.isNotEmpty()) {
            Update.check(installed).getOrNull()?.let { update = it }
        }
    }

    update?.let { release ->
        UpdateDialog(
            release = release,
            busy = updating,
            written = written,
            total = total,
            problem = problem,
            onDownload = {
                val url = release.apkUrl
                if (url == null) {
                    // No APK attached to the release: the page is the honest fallback.
                    openInBrowser(context, release.pageUrl)
                    update = null
                } else {
                    updating = true
                    written = 0
                    total = 0
                    problem = null
                    scope.launch {
                        val result = Update.download(url, java.io.File(context.cacheDir, "update.apk"), release.digest) { done, size ->
                            written = done
                            total = size
                        }
                        updating = false
                        val file = result.getOrNull()
                        if (file == null) {
                            // It used to fail in silence and open the browser, which looked exactly
                            // like nothing happening. Now it says so.
                            problem = "No pude descargar la actualizacion: ${result.exceptionOrNull()?.message}"
                        } else {
                            problem = installUpdate(context, file)
                            if (problem == null) update = null
                        }
                    }
                }
            },
            onDismiss = {
                update = null
                problem = null
            },
            onOpenReleases = {
                openInBrowser(context, Update.RELEASES_PAGE)
                update = null
            },
        )
    }

    val english = prefs.resolvedGameLanguage() == "en"
    val launcherLabel = when {
        !onboarded -> if (english) "Ready to start" else "Listo para iniciar"
        pushed == Pushed.About -> if (english) "Viewing About" else "Viendo el Acerca de"
        pushed == Pushed.Controls -> if (english) "Customizing the Gamepad" else "Personalizando el Mando"
        pushed == Pushed.Discord -> if (english) "Viewing Rich Presence" else "Viendo el Rich Presence"
        pushed == Pushed.TouchSettings || pushed == Pushed.TouchLayout ->
            if (english) "Customizing the Controls" else "Personalizando los Controles"
        pushed == Pushed.MySaves -> if (english) "Viewing the Saves" else "Viendo las Partidas"
        tab == Tab.Log -> if (english) "In Logs" else "En los Logs"
        tab == Tab.Settings -> if (english) "In Settings" else "En los Ajustes"
        else -> if (english) "Ready to start" else "Listo para iniciar"
    }
    LaunchedEffect(launcherLabel, resumeTick) {
        DiscordRpcManager.setLauncherScreen(launcherLabel)
    }

    if (!onboarded) {
        Onboarding(
            repo = repo,
            prefs = prefs,
            onFinish = { onboarded = true },
        )
        return
    }

    BackHandler(enabled = pushed != null) { pushed = null }

    when (pushed) {
        Pushed.About -> AboutScreen(onBack = { pushed = null })
        Pushed.Controls -> ControlsScreen(
            prefs = prefs,
            filesDir = context.filesDir,
            onBack = { pushed = null },
        )
        Pushed.Discord -> DiscordScreen(prefs = prefs, onBack = { pushed = null })
        Pushed.TouchSettings -> TouchSettingsScreen(prefs = prefs, onBack = { pushed = null })
        Pushed.Fps -> FpsScreen(prefs = prefs, onBack = { pushed = null })
        Pushed.TouchLayout -> TouchLayoutScreen(prefs = prefs, onBack = { pushed = null })
        Pushed.MySaves -> MySavesScreen(repo = repo, onBack = { pushed = null })
        Pushed.Mods -> ModsScreen(filesDir = context.filesDir, onBack = { pushed = null })
        Pushed.ReloadedOnline -> ReloadedOnlineScreen(
            repo = repo,
            onBack = { pushed = null },
            onStart = {
                launching = true
                DiscordRpcManager.setLauncherScreen(null)
                Emulator.start(context)
            },
        )
        null -> Box(modifier = Modifier.fillMaxSize()) {
            HorizontalPager(state = pagerState, modifier = Modifier.fillMaxSize()) { page ->
                when (Tab.entries[page]) {
                    Tab.Home -> HomeScreen(
                        repo,
                        revision = revision,
                        networkGranted = network,
                        onRequestNetwork = { requestNetwork.launch(Manifest.permission.INTERNET) },
                        onLog = { scope.launch { pagerState.animateScrollToPage(Tab.Log.ordinal) } },
                        onAbout = { pushed = Pushed.About },
                        onReloadedOnline = { pushed = Pushed.ReloadedOnline },
                        onLaunch = {
                            launching = true
                            DiscordRpcManager.setLauncherScreen(null)
                            Emulator.start(context)
                        },
                        launching = launching,
                        onSettings = { scope.launch { pagerState.animateScrollToPage(Tab.Settings.ordinal) } },
                    )
                    Tab.Log -> LogScreen(repo)
                    Tab.Settings -> SettingsScreen(
                        repo,
                        prefs,
                        revision = revision,
                        onLog = { scope.launch { pagerState.animateScrollToPage(Tab.Log.ordinal) } },
                        onAbout = { pushed = Pushed.About },
                        onControls = { pushed = Pushed.Controls },
                        onDiscord = { pushed = Pushed.Discord },
                        onTouchSettings = { pushed = Pushed.TouchSettings },
                        onFpsSettings = { pushed = Pushed.Fps },
                        onMySaves = { pushed = Pushed.MySaves },
                        onMods = { pushed = Pushed.Mods },
                        onUpdateFound = { update = it },
                    )
                }
            }
            // The bar floats over the pages instead of taking a strip of its own, so nothing is
            // cut off at the bottom and there is no band of a different colour behind it.
            ReloadedBottomNav(
                current = tab,
                onSelect = { target ->
                    // A tap animates across instead of jumping, so it reads the same as a swipe.
                    scope.launch { pagerState.animateScrollToPage(target.ordinal) }
                },
                modifier = Modifier.align(Alignment.BottomCenter),
            )
        }
    }
}
