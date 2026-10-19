package com.klisee.reloaded

import android.content.Context
import android.media.AudioAttributes
import android.os.Build
import android.os.SystemClock
import android.os.VibrationAttributes
import android.os.VibrationEffect
import android.os.Vibrator
import android.provider.Settings
import android.view.HapticFeedbackConstants
import android.view.View

/** Single short click on DOWN. No timers, repeated waveform or callback in the rendering loop. */
class TouchFeedback(private val context: Context, private val vibrator: Vibrator? = context.getSystemService(Vibrator::class.java)) {
    private var lastPulse = Long.MIN_VALUE
    val available: Boolean get() = vibrator?.hasVibrator() == true
    val supportsIntensity: Boolean get() = available && Build.VERSION.SDK_INT >= 30 &&
        vibrator!!.areAllPrimitivesSupported(VibrationEffect.Composition.PRIMITIVE_CLICK)

    fun press(view: View, appearance: TouchAppearance): Boolean {
        if (!appearance.haptics || !available || !view.isHapticFeedbackEnabled ||
            Settings.System.getInt(context.contentResolver, Settings.System.HAPTIC_FEEDBACK_ENABLED, 1) == 0) return false
        val now = SystemClock.uptimeMillis()
        // Coalesce simultaneous haptics, never the independent key events.
        if (lastPulse != Long.MIN_VALUE && now-lastPulse < 25) return false
        lastPulse = now
        return runCatching {
            if (supportsIntensity) {
                val effect = VibrationEffect.startComposition()
                    .addPrimitive(VibrationEffect.Composition.PRIMITIVE_CLICK, appearance.strength.scale).compose()
                if (Build.VERSION.SDK_INT >= 33) {
                    vibrator!!.vibrate(effect, VibrationAttributes.Builder().setUsage(VibrationAttributes.USAGE_TOUCH).build())
                } else {
                    @Suppress("DEPRECATION")
                    vibrator!!.vibrate(effect, AudioAttributes.Builder().setUsage(AudioAttributes.USAGE_ASSISTANCE_SONIFICATION).build())
                }
                true
            } else {
                view.performHapticFeedback(HapticFeedbackConstants.VIRTUAL_KEY)
            }
        }.getOrElse { false }
    }
}
