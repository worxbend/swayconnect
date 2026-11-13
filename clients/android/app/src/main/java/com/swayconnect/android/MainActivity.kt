package com.swayconnect.android

import android.content.Intent
import android.os.Bundle
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
import androidx.compose.runtime.getValue
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
import com.swayconnect.android.ui.state.Screen
import com.swayconnect.android.ui.theme.SwayConnectTheme
import kotlinx.coroutines.launch

class MainActivity : ComponentActivity() {

    private val viewModel: MainViewModel by viewModels { MainViewModel.Factory(AppContainer(applicationContext)) }

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
        setContent { SwayConnectTheme { App(viewModel) } }
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        offerPairingLink(intent)
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
private fun App(viewModel: MainViewModel) {
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
                    AppShell(state, viewModel) { padding -> DesktopArea(state, viewModel, padding) }
                }
            }
    }
}
