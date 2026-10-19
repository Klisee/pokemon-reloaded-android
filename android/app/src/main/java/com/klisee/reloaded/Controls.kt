package com.klisee.reloaded

import android.content.Context
import android.hardware.input.InputManager
import android.view.InputDevice
import java.io.File

/** The pads Android is reporting right now. */
fun connectedPads(context: Context): List<String> {
    val manager = context.getSystemService(Context.INPUT_SERVICE) as InputManager
    val names = mutableListOf<String>()
    for (id in manager.inputDeviceIds) {
        val device = manager.getInputDevice(id) ?: continue
        val gamepad = device.sources and InputDevice.SOURCE_GAMEPAD == InputDevice.SOURCE_GAMEPAD
        val joystick = device.sources and InputDevice.SOURCE_JOYSTICK == InputDevice.SOURCE_JOYSTICK
        if (!device.isVirtual && (gamepad || joystick)) names.add(device.name)
    }
    return names
}

/**
 * The keyboard keys the game reads that a pad button can be bound to.
 *
 * The game only ever reads the keyboard, so controller support is a pad pressing those keys.
 * Android reports the same key codes for every brand of pad, which is what lets one layout cover
 * an Xbox pad, a PlayStation one and a generic one at the same time.
 */
enum class ControlAction(val key: String, val defaultCode: Int) {
    Z("Z", 96), // A (Xbox, Nintendo) / X (PlayStation) / abajo del grupo derecho
    X("X", 97), // B (Xbox, Nintendo) / circulo (PlayStation) / derecha
    C("C", 99), // X (Xbox, Nintendo) / cuadrado (PlayStation) / izquierda
    V("V", 100), // Y (Xbox, Nintendo) / triangulo (PlayStation) / arriba
    ENTER("RETURN", 109), // select, share, view, menos
    SPACE("SPACE", 108), // start, options, menu, mas
    B("B", 103), // R1, RB
    M("M", 102), // L1, LB
    O("O", 105), // R2, RT, ZR
    S("S", 104), // L2, LT, ZL
    E("E", 106), // pulsar cualquier stick
}

/** What the key does in the game, for the ones that are known. */
val ControlAction.hint: String
    get() = when (this) {
        ControlAction.Z -> "Interactuar y confirmar"
        ControlAction.X -> "Cancelar y correr"
        ControlAction.O -> "Mochila"
        ControlAction.E -> "Escapar de un Pokemon salvaje"
        else -> ""
    }

/** The name of a pad button, for showing it on screen. */
fun buttonName(code: Int): String = when (code) {
    96 -> "A"
    97 -> "B"
    99 -> "X"
    100 -> "Y"
    102 -> "L1"
    103 -> "R1"
    104 -> "L2"
    105 -> "R2"
    106 -> "Stick izquierdo"
    107 -> "Stick derecho"
    108 -> "Start"
    109 -> "Select"
    else -> "Boton $code"
}

/**
 * Writes the layout the emulator reads when it starts.
 *
 * Only the buttons the player actually changed are written, so everything they did not touch keeps
 * the built-in layout (which, for instance, accepts either stick press for `E`).
 */
fun writeControlsFile(filesDir: File, bindings: Map<ControlAction, Int>, stick: Boolean, padConnected: Boolean) {
    val text = buildString {
        appendLine("# Escrito por la pantalla de Controles. El emulador lo lee al arrancar.")
        for (action in ControlAction.entries) {
            bindings[action]?.let { appendLine("$it=${action.key}") }
        }
        appendLine("stick=${if (stick) 1 else 0}")
        // With a pad connected the game starts with the touch controls hidden; touching the screen
        // brings them back.
        appendLine("pad=${if (padConnected) 1 else 0}")
    }
    runCatching { File(filesDir, "controls.conf").writeText(text) }
}
