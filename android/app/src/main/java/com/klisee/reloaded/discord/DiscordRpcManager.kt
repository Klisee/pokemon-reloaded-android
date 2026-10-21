package com.klisee.reloaded.discord

import android.content.Context
import android.content.Intent
import android.os.Build
import android.util.Log
import com.klisee.reloaded.Preferences
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import org.json.JSONObject
import java.net.HttpURLConnection
import java.net.URL

/**
 * Único dueño de la conexión con Discord mientras la app vive.
 *
 * El motor (Rust) llama a [setRoute] al cambiar de sala; Ajustes llama a [login]/[logout]. El resto
 * —conectar el Gateway y publicar la actividad— pasa aquí.
 */
object DiscordRpcManager {
    private const val TAG = "DiscordRpc"

    enum class Status { Disconnected, Connecting, Connected, Error }

    private lateinit var context: Context
    private lateinit var prefs: Preferences
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private val connectMutex = Mutex()

    /**
     * Presence frames are coalesced: walking through several rooms at once used to queue one
     * WebSocket message per room, and Discord kept showing an old one. Only the latest is sent.
     */
    private val presenceQueue = Channel<String>(Channel.CONFLATED)
    private var presenceConsumerStarted = false
    private var headJob: Job? = null

    @Volatile private var gateway: DiscordGateway? = null
    @Volatile private var currentRoute: String? = null
    @Volatile private var sessionStartMs: Long = 0L
    @Volatile private var character: RpcCharacter? = null
    @Volatile private var characterImage: String? = null
    @Volatile private var largeImage: String? = null
    @Volatile private var currentPlayerName: String? = null
    @Volatile private var battleText: String? = null
    @Volatile private var battleTrainer: String? = null
    @Volatile private var battleSpecies: Int = -1

    private val _status = MutableStateFlow(Status.Disconnected)
    val status: StateFlow<Status> = _status

    /** What the preview screen shows: the route, the character and its locally built head. */
    private val _route = MutableStateFlow<String?>(null)
    val route: StateFlow<String?> = _route
    private val _characterName = MutableStateFlow<String?>(null)
    val characterName: StateFlow<String?> = _characterName
    private val _characterFemale = MutableStateFlow(false)
    val characterFemale: StateFlow<Boolean> = _characterFemale
    private val _launcherScreen = MutableStateFlow<String?>(null)
    val launcherScreen: StateFlow<String?> = _launcherScreen
    private val _headFile = MutableStateFlow<java.io.File?>(null)
    val headFile: StateFlow<java.io.File?> = _headFile
    private val _playerName = MutableStateFlow<String?>(null)
    val playerName: StateFlow<String?> = _playerName

    private val _user = MutableStateFlow<String?>(null)
    val user: StateFlow<String?> = _user

    /** Si la integración está activada en Ajustes. */
    val enabled: Boolean get() = prefs.discordEnabled

    fun init(appContext: Context) {
        context = appContext.applicationContext
        prefs = Preferences(context)
        DiscordTokenStore.init(context)
        DiscordCharacters.load(context)
        DiscordExternalAssets.init(context)
        _user.value = DiscordTokenStore.getUser()?.let { it.globalName ?: it.username }
        if (!presenceConsumerStarted) {
            presenceConsumerStarted = true
            scope.launch {
                for (frame in presenceQueue) {
                    val gateway = gateway
                    if (gateway != null && gateway.isReady) runCatching { gateway.updatePresence(frame) }
                    delay(1_000)
                }
            }
        }
    }

    fun isAuthorized(): Boolean = !DiscordTokenStore.getToken().isNullOrBlank()

    /** The effective language of the game and of the Rich Presence labels. */
    fun isEnglish(): Boolean = prefs.resolvedGameLanguage() == "en"

    /** Abre el login de Discord, toma el token de sesión y conecta el Gateway. */
    fun login(activityContext: Context, onResult: (Boolean) -> Unit) {
        if (!DiscordConfig.isConfigured) {
            onResult(false)
            return
        }
        scope.launch {
            try {
                DiscordLoginActivity.newPending()
                withContext(Dispatchers.Main) {
                    activityContext.startActivity(
                        Intent(activityContext, DiscordLoginActivity::class.java)
                            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK),
                    )
                }
                val token = DiscordLoginActivity.awaitToken()
                val user = fetchUser(token) ?: throw DiscordAuthException.InvalidGrant()
                DiscordTokenStore.storeToken(token)
                DiscordTokenStore.storeUser(user)
                _user.value = user.globalName ?: user.username
                connect()
                withContextMain { onResult(true) }
            } catch (e: Exception) {
                Log.w(TAG, "login failed", e)
                _status.value = Status.Error
                withContextMain { onResult(false) }
            }
        }
    }

    fun cancelLogin() = DiscordLoginActivity.cancelPending()

    fun logout() {
        disconnect()
        DiscordTokenStore.clear()
        _user.value = null
    }

    /** Conecta si está activado y hay sesión; seguro de llamar en cada arranque del juego. */
    fun startIfEnabled() {
        if (!DiscordConfig.isConfigured || !prefs.discordEnabled || !isAuthorized()) {
            Log.i(TAG, "not starting (configured=${DiscordConfig.isConfigured}, enabled=${prefs.discordEnabled})")
            return
        }
        ensureLargeImage()
        scope.launch { connect() }
    }

    /**
     * Registers the Discord application icon (its CDN URL) as an external asset. A user-token
     * presence does not render the portal icon by itself, and omitting the large image shows "?";
     * the `mp:` external asset is what actually displays, and Discord's CDN does not expire.
     */
    private fun ensureLargeImage() {
        if (largeImage != null) return
        val token = DiscordTokenStore.getToken() ?: return
        val cacheKey = "logo:cdn:app-v1"
        DiscordExternalAssets.cached(cacheKey)?.let {
            largeImage = it
            publish()
            return
        }
        scope.launch {
            DiscordExternalAssets.register(token, DiscordConfig.APP_ICON_URL)
                .onSuccess { path ->
                    DiscordExternalAssets.remember(cacheKey, path)
                    largeImage = path
                    publish()
                }
                .onFailure { Log.w(TAG, "no pude registrar el icono de Discord", it) }
        }
    }

    fun disconnect() {
        gateway?.close()
        gateway = null
        _status.value = Status.Disconnected
    }

    /** Llamado desde el motor cuando el jugador entra en otra sala. */
    fun setRoute(route: String?) {
        currentRoute = route
        _route.value = route
        publish()
    }

    /** Publishes the current activity again, e.g. from the preview screen's refresh button. */
    fun republish() = publish()

    /**
     * The launcher is open: show "En el lanzador" and the screen the player is on in the Kotlin app.
     * A null screen means the game took over, so the engine's room is used again.
     */
    fun setLauncherScreen(screen: String?) {
        _launcherScreen.value = screen
        if (screen != null) {
            currentPlayerName = null
            _playerName.value = null
            character = null
            _characterName.value = null
            _characterFemale.value = false
            characterImage = null
            _headFile.value = null
            headJob?.cancel()
            currentRoute = if (isEnglish()) "In the Launcher" else DiscordConfig.FALLBACK_DETAILS
            _route.value = currentRoute
        }
        publish()
    }

    /** Title, character selection and the intro: there is no player yet, and no head is loaded. */
    fun clearPlayer() {
        if (currentPlayerName == null && character == null && characterImage == null) return
        currentPlayerName = null
        _playerName.value = null
        character = null
        _characterName.value = null
        _characterFemale.value = false
        characterImage = null
        _headFile.value = null
        headJob?.cancel()
        publish()
    }

    /** "Combate contra X" for trainers, "Combate salvaje" otherwise; null goes back to the room. */
    fun setBattle(inBattle: Boolean, trainer: String?, species: Int) {
        battleTrainer = if (inBattle) trainer else null
        battleSpecies = if (inBattle) species else -1
        val text = if (!inBattle) {
            null
        } else {
            val name = trainer?.trim().orEmpty()
            if (name.isNotEmpty() && !name.equals("Salvaje", ignoreCase = true)) {
                val mode = prefs.trainerNameLimit
                val shown = when {
                    mode == -1 -> TrainerNames.abbreviate(name)
                    mode > 0 && name.length > mode -> name.take(mode).trimEnd() + "…"
                    else -> name
                }
                if (isEnglish()) "In battle against $shown" else "En combate contra $shown"
            } else {
                if (isEnglish()) "In battle" else "En combate"
            }
        }
        if (text == battleText) return
        battleText = text
        publish()
    }

    /** Rebuilds the battle line after the player changes the trainer-name limit. */
    fun retrimBattle() {
        val name = battleTrainer ?: return
        setBattle(true, name, battleSpecies)
    }

    /** The nickname typed when the save was created; it takes precedence over the character name. */
    fun setPlayerName(name: String?) {
        val clean = name?.trim()?.takeIf { it.isNotEmpty() } ?: return
        if (clean == currentPlayerName) return
        currentPlayerName = clean
        _playerName.value = clean
        publish()
    }

    /**
     * Llamado desde el motor al cambiar de personaje. Resuelve el nombre y, si hay host configurado,
     * registra la cabeza como imagen externa (cacheada) para usarla en la presencia.
     */
    fun setCharacter(prota: Int, outfit: Int, char2: Int) {
        val resolved = DiscordCharacters.resolve(prota, char2) ?: return
        Log.i(TAG, "personaje prota=$prota vestuario=$outfit char2=$char2 -> ${resolved.name} (${resolved.file})")
        _characterName.value = resolved.name
        _characterFemale.value = resolved.female
        if (resolved != character) {
            character = resolved
            characterImage = null
            _headFile.value = null
            publish()
        } else if (characterImage != null) {
            return
        }

        val token = DiscordTokenStore.getToken() ?: return
        loadCharacterImage(token, resolved)
    }

    /**
     * Resolves the small image for [resolved] with retries. The first attempt can fail because the
     * sprite PNG is still being written by the engine, or because the network is not ready yet.
     */
    private fun loadCharacterImage(token: String, resolved: RpcCharacter) {
        val base = DiscordConfig.ASSETS_BASE_URL
        if (base.isNotBlank()) {
            val url = base.trimEnd('/') + "/" + resolved.file
            DiscordExternalAssets.cached(url)?.let {
                characterImage = it
                publish()
                return
            }
            headJob?.cancel()
            headJob = scope.launch {
                repeat(6) { attempt ->
                    if (character != resolved) return@launch
                    DiscordExternalAssets.register(token, url).getOrNull()?.let {
                        if (character != resolved) return@launch
                        characterImage = it
                        publish()
                        return@launch
                    }
                    Log.w(TAG, "cabeza: registro falló (intento ${attempt + 1})")
                    delay(3_000L * (attempt + 1))
                }
            }
            return
        }

        // No bundled artwork: the engine exports the player's own sprite, whatever the character,
        // and the head is cropped from it before uploading.
        val cacheKey = "head:v6:${resolved.file}"
        DiscordExternalAssets.cached(cacheKey)?.let {
            characterImage = it
            publish()
            return
        }
        headJob?.cancel()
        headJob = scope.launch {
            repeat(9) { attempt ->
                if (character != resolved) return@launch
                val file = DiscordCharacterImage.headFromPlayer(context)
                if (file != null) {
                    _headFile.value = file
                    val uploaded = DiscordCharacterImage.upload(file).getOrNull()
                    val path = uploaded?.let { DiscordExternalAssets.register(token, it).getOrNull() }
                    if (path != null) {
                        if (character != resolved) return@launch
                        DiscordExternalAssets.remember(cacheKey, path)
                        characterImage = path
                        publish()
                        return@launch
                    }
                    Log.w(TAG, "cabeza: subida/registro falló (intento ${attempt + 1})")
                } else {
                    Log.w(TAG, "cabeza: rpc-player.png no disponible (intento ${attempt + 1})")
                }
                delay(3_000L * (attempt + 1))
            }
        }
    }

    private suspend fun connect() {
        connectMutex.withLock {
            // Already connected (the launcher asked and then the game did): keep the one session.
            if (gateway != null) return
            _status.value = Status.Connecting
            val token = DiscordTokenStore.getToken()
            if (token.isNullOrBlank()) {
                _status.value = Status.Disconnected
                return
            }
            sessionStartMs = System.currentTimeMillis()

            val created = DiscordGateway(
                deviceName = Build.DEVICE ?: "android",
                onReady = {
                    _status.value = Status.Connected
                    scope.launch { fetchUser(token)?.let { DiscordTokenStore.storeUser(it) } }
                    publish()
                    if (characterImage == null) character?.let { loadCharacterImage(token, it) }
                },
                onClosed = {
                    if (_status.value != Status.Disconnected) _status.value = Status.Error
                },
            ).apply { setToken(token) }
            gateway = created
            created.connect()
        }
    }

    private fun publish() {
        val gw = gateway ?: return
        if (!gw.isReady) return
        val english = isEnglish()
        val activity = DiscordPresence.buildActivity(
            route = battleText ?: currentRoute ?: (if (english) "In the Launcher" else "En el Lanzador"),
            startMs = sessionStartMs.takeIf { it > 0 },
            smallImage = characterImage,
            smallText = character?.name,
            characterName = currentPlayerName ?: character?.name,
            characterFemale = character?.female == true,
            stateOverride = _launcherScreen.value,
            largeImage = largeImage,
            english = english,
        )
        // Never send from the game thread: the WebSocket queue can block during a scene change.
        presenceQueue.trySend(DiscordPresence.buildPresenceUpdate(activity))
    }

    /** Lee la cuenta detrás del token; sirve también para comprobar que el token es válido. */
    private fun fetchUser(token: String): DiscordTokenStore.User? {
        try {
            val connection = (URL(DiscordConfig.CURRENT_USER_URL).openConnection() as HttpURLConnection).apply {
                requestMethod = "GET"
                connectTimeout = 10_000
                readTimeout = 10_000
                setRequestProperty("Authorization", token)
                setRequestProperty("Accept", "application/json")
            }
            val status = connection.responseCode
            val text = (if (status in 200..299) connection.inputStream else connection.errorStream)
                ?.bufferedReader()?.use { it.readText() }.orEmpty()
            connection.disconnect()
            if (status !in 200..299) {
                Log.w(TAG, "user fetch HTTP $status")
                return null
            }

            val json = JSONObject(text)
            return DiscordTokenStore.User(
                id = json.getString("id"),
                username = json.getString("username"),
                globalName = json.optString("global_name").takeIf { it.isNotEmpty() && it != "null" },
                avatar = json.optString("avatar").takeIf { it.isNotEmpty() && it != "null" },
            )
        } catch (e: Exception) {
            Log.w(TAG, "user fetch failed", e)
            return null
        }
    }

    private suspend fun withContextMain(block: () -> Unit) {
        withContext(Dispatchers.Main) { block() }
    }
}
