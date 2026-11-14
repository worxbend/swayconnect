package com.swayconnect.android.ui.keys

import android.view.inputmethod.EditorInfo
import android.view.inputmethod.InputConnection
import androidx.compose.ui.platform.PlatformTextInputInterceptor
import androidx.compose.ui.platform.PlatformTextInputMethodRequest
import androidx.compose.ui.platform.PlatformTextInputSession

/**
 * Text typed for the computer (commands, host names, passwords) must not teach or sync the phone's keyboard
 * (docs/UX.md 9.2, 17.8). Every input session started inside `InterceptPlatformTextInput(NoLearningInterceptor)`
 * gets [noLearning] on its `imeOptions`, in plain and Private mode alike.
 */
object NoLearningInterceptor : PlatformTextInputInterceptor {
    override suspend fun interceptStartInputMethod(request: PlatformTextInputMethodRequest, nextHandler: PlatformTextInputSession): Nothing =
        nextHandler.startInputMethod(NoLearningRequest(request))
}

private class NoLearningRequest(private val original: PlatformTextInputMethodRequest) : PlatformTextInputMethodRequest {
    override fun createInputConnection(outAttributes: EditorInfo): InputConnection {
        val connection = original.createInputConnection(outAttributes)
        outAttributes.imeOptions = noLearning(outAttributes.imeOptions)
        return connection
    }
}

/** No personalised learning, no full-screen extract editor (which would show the text outside the app). */
internal fun noLearning(imeOptions: Int): Int = imeOptions or EditorInfo.IME_FLAG_NO_PERSONALIZED_LEARNING or EditorInfo.IME_FLAG_NO_EXTRACT_UI
