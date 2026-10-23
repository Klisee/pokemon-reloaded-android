package com.klisee.reloaded

import android.app.NativeActivity
import android.content.Context
import android.media.AudioAttributes
import android.media.AudioFocusRequest
import android.media.AudioManager
import android.os.Build
import android.os.Bundle
import android.text.InputType
import android.util.Log
import android.view.KeyEvent
import android.view.View
import android.view.ViewGroup
import android.view.WindowManager
import android.view.inputmethod.BaseInputConnection
import android.view.inputmethod.EditorInfo
import android.view.inputmethod.InputConnection
import android.view.inputmethod.InputMethodManager
import android.widget.EditText
import android.widget.Toast
import androidx.core.view.ViewCompat
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import androidx.core.view.WindowInsetsControllerCompat
import com.klisee.reloaded.discord.DiscordRpcManager
import com.klisee.reloaded.discord.DiscordRpcService
import com.klisee.reloaded.ui.Strings

/**
 * The emulator activity: a plain `NativeActivity` (that is what loads `libgm8emulator.so`),
 * subclassed to hide the system bars, to honour the launcher's screen setting, and to be the target
 * the system keyboard attaches to.
 *
 * Hiding has to be done here rather than from Rust: the Context that `android-activity` publishes
 * for JNI is the *Application*, which has no `getWindow()`, so the native attempt always failed
 * with a NoSuchMethodError and the navigation bar stayed on screen.
 */
private const val TAG = "ReloadedIme"

class GameActivity : NativeActivity() {
    private val touchFeedback by lazy { TouchFeedback(this) }
    private val touchPreferences by lazy { Preferences(this) }
    private val audioManager by lazy { getSystemService(Context.AUDIO_SERVICE) as AudioManager }
    private var focusRequest: AudioFocusRequest? = null

    /**
     * Another app took the audio focus (a call, a notification, a video): the game pauses its output
     * instead of talking over it, and resumes when the focus comes back.
     */
    private val focusListener = AudioManager.OnAudioFocusChangeListener { change ->
        when (change) {
            AudioManager.AUDIOFOCUS_LOSS, AudioManager.AUDIOFOCUS_LOSS_TRANSIENT -> nativeAudioFocus(true)
            AudioManager.AUDIOFOCUS_GAIN -> nativeAudioFocus(false)
        }
    }

    override fun onResume() {
        super.onResume()
        // Ask once the window is actually on screen: some Android versions reject a focus request
        // made during onResume, and a denied request must never take the game down with it.
        window.decorView.post { requestAudioFocus() }
    }

    private fun requestAudioFocus() {
        if (focusRequest != null) return
        runCatching {
            val request = AudioFocusRequest.Builder(AudioManager.AUDIOFOCUS_GAIN)
                .setAudioAttributes(
                    AudioAttributes.Builder()
                        .setUsage(AudioAttributes.USAGE_GAME)
                        .setContentType(AudioAttributes.CONTENT_TYPE_MUSIC)
                        .build(),
                )
                .setOnAudioFocusChangeListener(focusListener)
                .build()
            if (audioManager.requestAudioFocus(request) == AudioManager.AUDIOFOCUS_REQUEST_GRANTED) {
                focusRequest = request
            }
        }
    }

    override fun onPause() {
        runCatching { focusRequest?.let { audioManager.abandonAudioFocusRequest(it) } }
        focusRequest = null
        super.onPause()
    }

    /** The 1x1 view the system keyboard types into. What it receives goes straight to the engine. */
    private var imeAnchor: EditText? = null

    override fun onCreate(savedInstanceState: Bundle?) {
        // Draw behind the notch/camera hole. Without this the system insets the window on the short
        // edge and the game is left with a black band beside it instead of filling the screen.
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
            val attrs = window.attributes
            attrs.layoutInDisplayCutoutMode =
                WindowManager.LayoutParams.LAYOUT_IN_DISPLAY_CUTOUT_MODE_SHORT_EDGES
            window.attributes = attrs
        }
        super.onCreate(savedInstanceState)
        instance = this
        if (Preferences(applicationContext).keepScreenOn) {
            window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        }
        // Mantiene a Discord al tanto de la ruta mientras el juego está abierto.
        DiscordRpcManager.init(applicationContext)
        Preferences(applicationContext).writeGameLanguage(filesDir)
        DiscordRpcManager.setLauncherScreen(null)
        com.klisee.reloaded.discord.RoomNames.load(applicationContext)
        if (Preferences(applicationContext).discordEnabled) {
            runCatching { DiscordRpcService.start(applicationContext) }
        }
        installImeAnchor()
        watchImeHeight()
        hideSystemBars()
    }

    override fun onDestroy() {
        if (instance === this) instance = null
        // El motor terminar la actividad es lo que cierra la sesión; pasar a segundo plano no.
        if (isFinishing) runCatching { DiscordRpcService.stop(applicationContext) }
        super.onDestroy()
    }

    // The bars come back on some system events, so re-hide them whenever we regain focus.
    override fun onWindowFocusChanged(hasFocus: Boolean) {
        super.onWindowFocusChanged(hasFocus)
        if (hasFocus) hideSystemBars()
    }

    private fun hideSystemBars() {
        WindowCompat.setDecorFitsSystemWindows(window, false)
        WindowInsetsControllerCompat(window, window.decorView).apply {
            hide(WindowInsetsCompat.Type.systemBars())
            systemBarsBehavior = WindowInsetsControllerCompat.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
        }
    }

    // ------------------------------------------------------------------------------------------
    // System keyboard. The game only asks for text through its own modal and has no Android view of
    // its own, so this tiny transparent view exists only to be the target the keyboard attaches to.
    // Nothing typed is kept in it: a custom InputConnection forwards each character to the engine,
    // which is the one that decides when to raise the keyboard (it sees the game's dialog open).

    private fun installImeAnchor() {
        val anchor = object : EditText(this) {
            override fun onCreateInputConnection(outAttrs: EditorInfo): InputConnection {
                outAttrs.inputType = InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_FLAG_NO_SUGGESTIONS
                // No fullscreen/extract editor and no suggestion strip: in landscape the system
                // would otherwise cover the game with its own text box.
                outAttrs.imeOptions = EditorInfo.IME_ACTION_DONE or
                    EditorInfo.IME_FLAG_NO_EXTRACT_UI or
                    EditorInfo.IME_FLAG_NO_FULLSCREEN
                return ForwardingInputConnection(this)
            }
        }
        anchor.isFocusable = true
        anchor.isFocusableInTouchMode = true
        anchor.alpha = 0f
        addContentView(anchor, ViewGroup.LayoutParams(1, 1))
        imeAnchor = anchor
    }

    /** Sends what is typed to the engine instead of leaving it in the view. */
    private inner class ForwardingInputConnection(target: View) : BaseInputConnection(target, false) {
        override fun commitText(text: CharSequence?, newCursorPosition: Int): Boolean {
            if (!text.isNullOrEmpty()) nativeCommitText(text.toString())
            return true
        }

        override fun deleteSurroundingText(beforeLength: Int, afterLength: Int): Boolean {
            repeat(beforeLength) { nativeBackspace() }
            return true
        }

        override fun sendKeyEvent(event: KeyEvent): Boolean {
            when (event.keyCode) {
                KeyEvent.KEYCODE_DEL -> {
                    nativeBackspace()
                    return true
                }
                KeyEvent.KEYCODE_ENTER -> {
                    nativeEnter()
                    return true
                }
            }
            return super.sendKeyEvent(event)
        }

        override fun performEditorAction(actionCode: Int): Boolean {
            nativeEnter()
            return true
        }
    }

    /**
     * Keeps the engine told of the system keyboard's height, so it can fit the game above it instead
     * of leaving what is being typed hidden under the keyboard.
     */
    private fun watchImeHeight() {
        ViewCompat.setOnApplyWindowInsetsListener(window.decorView) { _, insets ->
            reportImeHeight(insets.getInsets(WindowInsetsCompat.Type.ime()).bottom)
            insets
        }
    }

    private fun currentImeHeight(): Int =
        ViewCompat.getRootWindowInsets(window.decorView)
            ?.getInsets(WindowInsetsCompat.Type.ime())?.bottom ?: 0

    private fun reportImeHeight(bottom: Int) {
        Log.i(TAG, "altura del teclado = $bottom")
        nativeImeInset(bottom)
    }

    private fun applyImeVisible(visible: Boolean) {
        val anchor = imeAnchor ?: return
        val imm = getSystemService(Context.INPUT_METHOD_SERVICE) as InputMethodManager
        Log.i(TAG, "teclado visible = $visible")
        if (visible) {
            anchor.requestFocus()
            imm.showSoftInput(anchor, 0)
            // The keyboard animates in, so its height is not final on the first frame; ask again once
            // it has settled. If it is still not up by then, the system ignored the first request
            // (it happens when nothing was touched), so ask once more.
            anchor.postDelayed({
                if (currentImeHeight() == 0) imm.showSoftInput(anchor, 0)
                reportImeHeight(currentImeHeight())
            }, 400)
        } else {
            imm.hideSoftInputFromWindow(anchor.windowToken, 0)
            anchor.clearFocus()
        }
    }

    /** Called from the audio focus listener: true pauses the game's output. */
    external fun nativeAudioFocus(lost: Boolean)

    external fun nativeCommitText(text: String)
    external fun nativeBackspace()
    external fun nativeEnter()
    external fun nativeImeInset(height: Int)

    companion object {
        /** Called from the engine when a touch control is pressed, to make it vibrate. */
        @JvmStatic
        fun touchControlPressed() {
            val activity = instance ?: return
            activity.runOnUiThread {
                if (!activity.isFinishing && !activity.isDestroyed && activity.hasWindowFocus()) {
                    activity.touchFeedback.press(activity.window.decorView, activity.touchPreferences.touchAppearance)
                }
            }
        }

        @JvmStatic
        fun showTouchHint() {
            val activity = instance ?: return
            activity.runOnUiThread {
                if (activity.isFinishing || activity.isDestroyed) return@runOnUiThread
                Strings.init(activity)
                Strings.update(Preferences(activity))
                Toast.makeText(
                    activity,
                    Strings.tr("Toca una pieza y pulsa Girar para rotarla."),
                    Toast.LENGTH_LONG,
                ).show()
            }
        }

        init {
            // These are declared as `native` methods on this class, so the JVM has to find their
            // `Java_com_klisee_reloaded_GameActivity_*` symbols in the library. The emulator loads
            // the same one through the manifest, and this only links it to the app's class loader.
            System.loadLibrary("gm8emulator")
        }

        private var instance: GameActivity? = null

        /**
         * Called from the engine over JNI (it only holds the Application context, so it goes through
         * this static). The keyboard has to be raised from the UI thread.
         */
        @JvmStatic
        fun setImeVisible(visible: Boolean) {
            val activity = instance ?: return
            activity.runOnUiThread {
                if (!activity.isFinishing && !activity.isDestroyed) activity.applyImeVisible(visible)
            }
        }

        /**
         * Called from the engine to make it read the keyboard's height again. The height normally
         * reaches it on its own from the insets; this is there so a callback that never arrived
         * cannot leave the game pushed up with its controls hidden.
         */
        @JvmStatic
        fun refreshImeHeight() {
            val activity = instance ?: return
            activity.runOnUiThread {
                if (!activity.isFinishing && !activity.isDestroyed) {
                    activity.reportImeHeight(activity.currentImeHeight())
                }
            }
        }

        /**
         * Llamado desde el motor (JNI) al entrar en otra sala. `route` es el nombre crudo, p. ej.
         * `rmRuta1`; se deja legible antes de mandarlo a Discord.
         */
        @JvmStatic
        fun onGameRoute(@Suppress("UNUSED_PARAMETER") roomId: Int, route: String?) {
            val raw = route?.lowercase().orEmpty()
            val noPlayer = raw.startsWith("inicio") || raw.startsWith("select") || raw.startsWith("intro")
            val newGame = raw.startsWith("select") || raw.startsWith("charla") || raw.startsWith("intro")
            if (noPlayer) DiscordRpcManager.clearPlayer()
            val newGameText = if (DiscordRpcManager.isEnglish()) "Starting a new game" else "Iniciando una nueva partida"
            DiscordRpcManager.setRoute(
                if (newGame) newGameText
                else (com.klisee.reloaded.discord.RoomNames.display(route) ?: prettyRoute(route))
                    ?.let { com.klisee.reloaded.discord.RoomNames.clean(it) },
            )
        }

        /** Protagonist slot (10 = custom), costume and custom trainer id, reported by the engine. */
        @JvmStatic
        fun onGameCharacter(prota: Int, outfit: Int, char2: Int) {
            DiscordRpcManager.setCharacter(prota, outfit, char2)
        }

        /** The trainer's nickname, as typed by the player. */
        @JvmStatic
        fun onGamePlayerName(name: String?) {
            DiscordRpcManager.setPlayerName(name)
        }

        /** Battle state, the opponent's name (empty when wild) and the wild species id. */
        @JvmStatic
        fun onGameBattle(inBattle: Boolean, trainer: String?, species: Int) {
            DiscordRpcManager.setBattle(inBattle, trainer, species)
        }

        /** `rmRuta1` / `Room_PuebloPaleta` -> `Ruta 1` / `Pueblo Paleta`. */
        private fun prettyRoute(raw: String?): String? {
            val value = raw?.trim().orEmpty()
            if (value.isEmpty()) return null
            var name = value
            for (prefix in listOf("room_", "room", "rm_", "rm")) {
                if (name.startsWith(prefix, ignoreCase = true)) {
                    name = name.substring(prefix.length)
                    break
                }
            }
            name = name.trim('_', ' ').replace('_', ' ')
            if (name.isEmpty()) return value
            return name.replace(Regex("(?<=[A-Za-zÁÉÍÓÚáéíóúÑñ])(?=\\d)"), " ").trim()
        }
    }
}
