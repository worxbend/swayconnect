package com.swayconnect.android.ui.theme

import androidx.compose.animation.core.FiniteAnimationSpec
import androidx.compose.animation.core.snap
import androidx.compose.animation.core.spring
import androidx.compose.runtime.staticCompositionLocalOf

/**
 * The six motion specs of docs/UX.md 13. Same shape as material3's `MotionScheme`, which is `internal` in
 * material3 1.4.0 (and so is the `MaterialTheme(motionScheme = ...)` overload), so the app keeps its own scheme
 * and reads it through [LocalSwayMotion]. M3 components keep their built-in standard motion.
 */
interface SwayMotionScheme {
    fun <T> defaultSpatialSpec(): FiniteAnimationSpec<T>

    fun <T> fastSpatialSpec(): FiniteAnimationSpec<T>

    fun <T> slowSpatialSpec(): FiniteAnimationSpec<T>

    fun <T> defaultEffectsSpec(): FiniteAnimationSpec<T>

    fun <T> fastEffectsSpec(): FiniteAnimationSpec<T>

    fun <T> slowEffectsSpec(): FiniteAnimationSpec<T>
}

/** Springs with no duration, so an interrupted animation keeps its velocity. */
object SwayMotion : SwayMotionScheme {
    override fun <T> defaultSpatialSpec(): FiniteAnimationSpec<T> = spring(dampingRatio = 0.8f, stiffness = 380f)

    override fun <T> fastSpatialSpec(): FiniteAnimationSpec<T> = spring(dampingRatio = 0.7f, stiffness = 800f)

    override fun <T> slowSpatialSpec(): FiniteAnimationSpec<T> = spring(dampingRatio = 0.85f, stiffness = 200f)

    override fun <T> defaultEffectsSpec(): FiniteAnimationSpec<T> = spring(dampingRatio = 1f, stiffness = 1600f)

    override fun <T> fastEffectsSpec(): FiniteAnimationSpec<T> = spring(dampingRatio = 1f, stiffness = 3800f)

    override fun <T> slowEffectsSpec(): FiniteAnimationSpec<T> = spring(dampingRatio = 1f, stiffness = 500f)
}

/**
 * Reduce motion (UX 13): every spatial spec snaps; effects (colour, alpha) keep their springs.
 * Pulses, the ring stretch and animated scrolls are dropped by their callers through [LocalReduceMotion].
 */
object SwayMotionReduced : SwayMotionScheme by SwayMotion {
    override fun <T> defaultSpatialSpec(): FiniteAnimationSpec<T> = snap()

    override fun <T> fastSpatialSpec(): FiniteAnimationSpec<T> = snap()

    override fun <T> slowSpatialSpec(): FiniteAnimationSpec<T> = snap()
}

/** The current scheme: [SwayMotionReduced] while reduce motion is on, else [SwayMotion]. */
val LocalSwayMotion = staticCompositionLocalOf<SwayMotionScheme> { SwayMotion }

/** True when the system animator scale is 0 or the app setting asks for reduced motion. */
val LocalReduceMotion = staticCompositionLocalOf { false }
