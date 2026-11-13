package com.swayconnect.android.ui.theme

import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.hapticfeedback.HapticFeedback
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.platform.LocalHapticFeedback

/** The haptics setting of docs/UX.md 14. Subtle drops the tick types. */
enum class HapticLevel { Off, Subtle, Full }

/**
 * The haptic moments of docs/UX.md 14. Haptics never carry meaning alone.
 *
 * API 29 fallback, recorded for slice A1 from the bytecode, not from a device:
 * Compose ui 1.12.1 `PlatformHapticFeedback` passes the raw `HapticFeedbackConstants` value to
 * `ViewCompat.performHapticFeedback`. androidx.core 1.18.0 `HapticFeedbackConstantsCompat` then maps a
 * constant the platform lacks. Below API 34: TOGGLE_ON, GESTURE_THRESHOLD_ACTIVATE, SEGMENT_TICK -> CONTEXT_CLICK;
 * TOGGLE_OFF, SEGMENT_FREQUENT_TICK -> CLOCK_TICK. Below API 30: CONFIRM -> VIRTUAL_KEY; REJECT -> LONG_PRESS.
 * LONG_PRESS is native. So on API 29 this app feels:
 *
 * | Type                     | API 29 constant |
 * |--------------------------|-----------------|
 * | SegmentTick              | CONTEXT_CLICK   |
 * | Confirm                  | VIRTUAL_KEY     |
 * | Reject                   | LONG_PRESS      |
 * | ToggleOn                 | CONTEXT_CLICK   |
 * | ToggleOff                | CLOCK_TICK      |
 * | SegmentFrequentTick      | CLOCK_TICK      |
 * | GestureThresholdActivate | CONTEXT_CLICK   |
 * | LongPress                | LONG_PRESS      |
 *
 * The library already falls back, so this wrapper adds no mapping of its own. The feel on a real API 29
 * device is unverified (the only emulator here runs API 37).
 */
enum class SwayHaptic(val type: HapticFeedbackType, val isTick: Boolean) {
    SegmentTick(HapticFeedbackType.SegmentTick, isTick = true),
    Confirm(HapticFeedbackType.Confirm, isTick = false),
    Reject(HapticFeedbackType.Reject, isTick = false),
    ToggleOn(HapticFeedbackType.ToggleOn, isTick = false),
    ToggleOff(HapticFeedbackType.ToggleOff, isTick = false),
    SegmentFrequentTick(HapticFeedbackType.SegmentFrequentTick, isTick = true),
    GestureThresholdActivate(HapticFeedbackType.GestureThresholdActivate, isTick = false),
    LongPress(HapticFeedbackType.LongPress, isTick = false),
}

fun HapticLevel.allows(haptic: SwayHaptic): Boolean = when (this) {
    HapticLevel.Off -> false
    HapticLevel.Subtle -> !haptic.isTick
    HapticLevel.Full -> true
}

/** Performs a [SwayHaptic] when the setting allows it. Get it from [LocalHaptics]. */
class Haptics(private val feedback: HapticFeedback?, private val level: HapticLevel) {
    fun perform(haptic: SwayHaptic) {
        if (level.allows(haptic)) feedback?.performHapticFeedback(haptic.type)
    }
}

val LocalHaptics = staticCompositionLocalOf { Haptics(feedback = null, level = HapticLevel.Off) }

@Composable
internal fun rememberHaptics(level: HapticLevel): Haptics {
    val feedback = LocalHapticFeedback.current
    return remember(feedback, level) { Haptics(feedback, level) }
}
