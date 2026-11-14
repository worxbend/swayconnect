package com.swayconnect.android

import android.content.Intent
import android.os.Bundle
import android.view.KeyEvent
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.viewModels
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.IntState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.lifecycleScope
import androidx.lifecycle.repeatOnLifecycle
import com.swayconnect.android.ui.MainViewModel
import com.swayconnect.android.ui.common.LocalIconCache
import com.swayconnect.android.ui.desktop.DesktopArea
import com.swayconnect.android.ui.desktop.TrustLostPage
import com.swayconnect.android.ui.pair.PairScreen
import com.swayconnect.android.ui.shell.AppShell
import com.swayconnect.android.ui.shell.VOLUME_KEY_STEP_PCT
import com.swayconnect.android.ui.shell.VolumeKey
import com.swayconnect.android.ui.shell.volumeKeyTarget
import com.swayconnect.android.ui.state.Screen
import com.swayconnect.android.ui.theme.SwayConnectTheme
import kotlinx.coroutines.launch

class MainActivity : ComponentActivity() {

    private val viewModel: MainViewModel by viewModels { MainViewModel.Factory(AppContainer(applicationContext)) }

    /** Phone volume keys sent to the computer; the shell shows "PC 45 %" after each (docs/UX.md 7.2). */
    private val volumeKeyPresses = mutableIntStateOf(0)

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        // Only a fresh launch delivers the link: a recreated activity must not fill the form again.
        if (savedInstanceState == null) offerPairingLink(intent)
        // The session lives only while the activity is started. Background means disconnect.
        // A pairing attempt is the exception: it lives in the view model until it ends.
        lifecycleScope.launch {
            repeatOnLifecycle(Lifecycle.State.STARTED) { viewModel.connectWhileStarted() }
        }
        setContent { SwayConnectTheme { App(viewModel, volumeKeyPresses) } }
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        offerPairingLink(intent)
    }

    /**
     * While the Audio area is shown, the volume keys step the computer's default output by 5 % and the mute key toggles
     * it (docs/UX.md 7.2, 17.8). The press acts, and both press and release are taken from the phone. When the phone may
     * not change that output now, the keys stay the phone's own. `onKeyDown`, not `dispatchKeyEvent`: the latter is a
     * restricted API on `ComponentActivity`, and no composable takes volume keys, so they reach this point.
     */
    override fun onKeyDown(keyCode: Int, event: KeyEvent): Boolean {
        val key = volumeKeyOf(keyCode) ?: return super.onKeyDown(keyCode, event)
        val sink = volumeKeyTarget(viewModel.state.value, key) ?: return super.onKeyDown(keyCode, event)
        when (key) {
            VolumeKey.Up -> viewModel.stepDefaultVolume(VOLUME_KEY_STEP_PCT)
            VolumeKey.Down -> viewModel.stepDefaultVolume(-VOLUME_KEY_STEP_PCT)
            // A held mute key repeats; only the first press toggles.
            VolumeKey.Mute -> if (event.repeatCount == 0) viewModel.setMuted(sink.id, !sink.muted) else return true
        }
        volumeKeyPresses.intValue++
        return true
    }

    override fun onKeyUp(keyCode: Int, event: KeyEvent): Boolean {
        val taken = volumeKeyOf(keyCode)?.let { volumeKeyTarget(viewModel.state.value, it) } != null
        return taken || super.onKeyUp(keyCode, event)
    }

    private fun volumeKeyOf(keyCode: Int): VolumeKey? =
        when (keyCode) {
            KeyEvent.KEYCODE_VOLUME_UP -> VolumeKey.Up
            KeyEvent.KEYCODE_VOLUME_DOWN -> VolumeKey.Down
            KeyEvent.KEYCODE_VOLUME_MUTE -> VolumeKey.Mute
            else -> null
        }

    /** A `swayconnect://pair` link only fills the pair form. Nothing connects before the user taps Pair. */
    private fun offerPairingLink(intent: Intent?) {
        if (intent == null || intent.action != Intent.ACTION_VIEW) return
        // The system keeps the intent that started this task and sends it again when the task is
        // reopened from the recents list. That link is used up; the app cannot clear the system's copy.
        if (intent.flags and Intent.FLAG_ACTIVITY_LAUNCHED_FROM_HISTORY != 0) return
        val link = intent.dataString ?: return
        viewModel.onPairingLink(link)
        // The link holds the pairing secret: this activity's own copy of the intent drops it.
        intent.data = null
    }
}

@Composable
private fun App(viewModel: MainViewModel, volumeKeyPresses: IntState) {
    val state by viewModel.state.collectAsStateWithLifecycle()
    when (state.screen) {
        Screen.Loading ->
            // A Surface, so that the spinner sits on the colour of the screen that follows.
            Surface(color = MaterialTheme.colorScheme.background) {
                Box(modifier = Modifier.fillMaxSize(), contentAlignment = Alignment.Center) { CircularProgressIndicator() }
            }
        Screen.Pair -> PairScreen(state, viewModel)
        Screen.Desktop ->
            if (state.unpaired || state.wrongKey) {
                // Full screen (docs/UX.md 16): nothing of the shell for a computer this phone cannot trust now.
                TrustLostPage(wrongKey = !state.unpaired, serverName = state.lostServerName, onPairAgain = viewModel::showPairScreen)
            } else {
                CompositionLocalProvider(LocalIconCache provides viewModel.iconCache) {
                    AppShell(state, viewModel, volumeKeyPresses) { padding -> DesktopArea(state, viewModel, padding) }
                }
            }
    }
}
