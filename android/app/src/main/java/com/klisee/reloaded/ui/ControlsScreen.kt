package com.klisee.reloaded.ui

import android.content.Context
import android.hardware.input.InputManager
import android.view.InputDevice
import android.view.KeyEvent
import android.view.MotionEvent
import android.view.View
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.RestartAlt
import androidx.compose.material.icons.filled.SportsEsports
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.Scaffold
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import com.klisee.reloaded.ControlAction
import com.klisee.reloaded.Preferences
import com.klisee.reloaded.buttonName
import com.klisee.reloaded.connectedPads
import com.klisee.reloaded.hint
import com.klisee.reloaded.writeControlsFile
import java.io.File

/** The action a button is currently bound to, for showing what a press will do. */
private fun actionFor(bindings: Map<ControlAction, Int>, code: Int): ControlAction? =
    ControlAction.entries.firstOrNull { (bindings[it] ?: it.defaultCode) == code }

/**
 * A view whose only job is to sit there focused and report every pad button pressed. That is what
 * makes this screen both a test bench and the way a remap learns which button was meant.
 */
private class PadProbe(context: Context) : View(context) {
    var onKey: ((Int) -> Unit)? = null
    var onStick: ((Float, Float) -> Unit)? = null

    init {
        isFocusable = true
        isFocusableInTouchMode = true
    }

    override fun onKeyDown(keyCode: Int, event: KeyEvent): Boolean {
        onKey?.invoke(keyCode)
        return true
    }

    override fun onGenericMotionEvent(event: MotionEvent): Boolean {
        if (event.source and InputDevice.SOURCE_JOYSTICK == InputDevice.SOURCE_JOYSTICK) {
            onStick?.invoke(event.getAxisValue(MotionEvent.AXIS_X), event.getAxisValue(MotionEvent.AXIS_Y))
            return true
        }
        return super.onGenericMotionEvent(event)
    }

    override fun onTouchEvent(event: MotionEvent): Boolean {
        if (event.actionMasked == MotionEvent.ACTION_DOWN) requestFocus()
        return true
    }
}

/**
 * The control screen: what pad is connected, a live test bench, and the button layout. Everything it
 * changes goes to `files/controls.conf`, which is the file the emulator reads when it starts.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ControlsScreen(prefs: Preferences, filesDir: File, onBack: () -> Unit) {
    val context = LocalContext.current
    var bindings by remember { mutableStateOf(prefs.controlBindings) }
    var stick by remember { mutableStateOf(prefs.stickEnabled) }
    var capturing by remember { mutableStateOf<ControlAction?>(null) }
    var lastCode by remember { mutableStateOf<Int?>(null) }
    var stickReadout by remember { mutableStateOf(0f to 0f) }
    var pads by remember { mutableStateOf(connectedPads(context)) }

    fun store(newBindings: Map<ControlAction, Int>, newStick: Boolean) {
        bindings = newBindings
        stick = newStick
        prefs.controlBindings = newBindings
        prefs.stickEnabled = newStick
        writeControlsFile(filesDir, newBindings, newStick, connectedPads(context).isNotEmpty())
    }

    // Pads come and go while this screen is open, so the list is kept current.
    DisposableEffect(Unit) {
        val manager = context.getSystemService(Context.INPUT_SERVICE) as InputManager
        val listener = object : InputManager.InputDeviceListener {
            override fun onInputDeviceAdded(deviceId: Int) { pads = connectedPads(context) }
            override fun onInputDeviceRemoved(deviceId: Int) { pads = connectedPads(context) }
            override fun onInputDeviceChanged(deviceId: Int) { pads = connectedPads(context) }
        }
        manager.registerInputDeviceListener(listener, null)
        onDispose { manager.unregisterInputDeviceListener(listener) }
    }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("Controles") },
                navigationIcon = {
                    IconButton(onClick = onBack) { Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "Volver") }
                },
            )
        },
    ) { padding ->
        Column(
            modifier = Modifier.padding(padding).fillMaxSize().verticalScroll(rememberScrollState()),
            verticalArrangement = Arrangement.spacedBy(16.dp),
        ) {
            SettingsGroup("Mando", modifier = Modifier.padding(horizontal = 16.dp)) {
                InfoRow(
                    icon = Icons.Filled.SportsEsports,
                    title = "Estado",
                    value = if (pads.isEmpty()) "Sin mando conectado" else pads.joinToString(", "),
                    items = 1,
                    index = 0,
                )
            }

            SettingsGroup("Probar", modifier = Modifier.padding(horizontal = 16.dp)) {
                InfoRow(
                    icon = Icons.Filled.SportsEsports,
                    title = capturing?.let { "Pulsa el boton para la tecla ${it.key}" } ?: "Ultimo boton pulsado",
                    value = when {
                        lastCode == null -> "Pulsa algo en el mando; aqui dira que tecla dispara"
                        else -> {
                            val action = actionFor(bindings, lastCode!!)
                            val name = buttonName(lastCode!!)
                            if (action == null) "$name - sin asignar" else "$name -> tecla ${action.key}"
                        }
                    },
                    items = 1,
                    index = 0,
                )
                // Focusable and always present, so it keeps receiving the pad's buttons.
                AndroidView(
                    factory = { ctx -> PadProbe(ctx).apply { requestFocus() } },
                    modifier = Modifier.height(1.dp),
                    update = { probe ->
                        probe.onKey = { code ->
                            lastCode = code
                            capturing?.let { action ->
                                store(bindings + (action to code), stick)
                                capturing = null
                            }
                        }
                        probe.onStick = { x, y -> stickReadout = x to y }
                        probe.requestFocus()
                    },
                )
                if (capturing != null) {
                    TextButton(onClick = { capturing = null }) { Text("Cancelar el cambio") }
                }
                val (stickX, stickY) = stickReadout
                if (stick && (stickX != 0f || stickY != 0f)) {
                    Text(
                        text = "Stick: x ${"%.2f".format(stickX)}  y ${"%.2f".format(stickY)}",
                        modifier = Modifier.padding(start = 16.dp),
                    )
                }
            }

            SettingsGroup("Botones", modifier = Modifier.padding(horizontal = 16.dp)) {
                ControlAction.entries.forEachIndexed { index, action ->
                    val code = bindings[action] ?: action.defaultCode
                    NavRow(
                        icon = Icons.Filled.SportsEsports,
                        title = "Tecla ${action.key}",
                        subtitle = listOfNotNull(
                            buttonName(code),
                            action.hint.takeIf { it.isNotEmpty() },
                        ).joinToString(" - "),
                        items = ControlAction.entries.size,
                        index = index,
                        onClick = { capturing = action },
                    )
                }
            }

            SettingsGroup("Stick", modifier = Modifier.padding(horizontal = 16.dp)) {
                SwitchRow(
                    icon = Icons.Filled.SportsEsports,
                    title = "Mover con el stick",
                    subtitle = "El stick izquierdo anda y navega, con la misma zona muerta que el tactil",
                    checked = stick,
                    items = 1,
                    index = 0,
                    onCheckedChange = { store(bindings, it) },
                )
            }

            Row(modifier = Modifier.padding(horizontal = 16.dp), verticalAlignment = androidx.compose.ui.Alignment.CenterVertically) {
                TextButton(onClick = { store(emptyMap(), true) }) {
                    Icon(Icons.Filled.RestartAlt, contentDescription = null)
                    androidx.compose.foundation.layout.Spacer(Modifier.width(8.dp))
                    Text("Restaurar los botones por defecto")
                }
            }
        }
    }
}
