package com.klisee.reloaded

import android.content.Context
import android.content.Intent

/** Starts the native activity that runs the bundled `libgm8emulator.so`. */
object Emulator {
    private const val GAME_ACTIVITY = "com.klisee.reloaded.GameActivity"

    fun start(context: Context) {
        // The emulator reads the pad layout and the video mode once, at startup, so they have to be
        // on disk by now.
        val prefs = Preferences(context)
        // One-time: the classic overlay goes back to the cruceta it always had.
        if (!prefs.touchPadMigration) {
            if (prefs.touchStyle == TouchOverlayStyle.Classic) prefs.moveMode = MoveMode.Dpad
            prefs.touchPadMigration = true
        }
        prefs.writeControls(context.filesDir, connectedPads(context).isNotEmpty())
        prefs.writeTouch(context.filesDir)
        prefs.writeVideo(context.filesDir)
        prefs.writeTouchLayouts(context.filesDir)
        prefs.writeTouchAppearance(context.filesDir)
        prefs.writeGameLanguage(context.filesDir)
        TouchSprites.writeRuntime(context, prefs.touchSprites)
        TouchLabels.writeRuntime(context, context.filesDir)
        context.startActivity(Intent().setClassName(context.packageName, GAME_ACTIVITY))
    }
}
