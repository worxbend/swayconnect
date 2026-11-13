package com.swayconnect.android.ui

import androidx.lifecycle.ViewModel
import androidx.lifecycle.ViewModelProvider
import androidx.lifecycle.viewModelScope
import com.swayconnect.android.AppContainer
import com.swayconnect.android.data.StoredPairing
import com.swayconnect.android.data.StoredPairingLostException
import com.swayconnect.protocol.ByeCode
import com.swayconnect.protocol.Connection
import com.swayconnect.protocol.ErrorCode
import com.swayconnect.protocol.HandshakeException
import com.swayconnect.protocol.Op
import com.swayconnect.protocol.PairingNotConfirmedException
import com.swayconnect.protocol.PairingUri
import com.swayconnect.protocol.PairingUriException
import com.swayconnect.protocol.PinMismatchException
import com.swayconnect.protocol.RejectCode
import com.swayconnect.protocol.RejectedException
import com.swayconnect.protocol.RequestFailedException
import com.swayconnect.protocol.Session
import com.swayconnect.protocol.SessionEnd
import com.swayconnect.protocol.SwayConnectClient
import com.swayconnect.protocol.Transport
import com.swayconnect.protocol.isDisplayLabel
import com.swayconnect.protocol.parsePairingUri
import com.swayconnect.protocol.replaceUnsafe
import java.io.IOException
import kotlinx.coroutines.awaitCancellation
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.collectLatest
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

/**
 * The one view model of the app. It owns the single connection: either a pairing attempt or the
 * session of the stored pairing. The UI renders [state] only.
 *
 * Everything that touches a socket, the profile file or the Keystore runs on the I/O dispatcher of
 * the container; closing a TLS socket on the main thread is not allowed on Android.
 */
class MainViewModel(private val container: AppContainer) : ViewModel() {

    /** What the connection loop should do. Changing it cancels the running connection. */
    private sealed interface Task {
        data object Idle : Task

        // Not data classes: instances are compared by identity, and a generated toString would print secrets.
        class Pairing(val uri: PairingUri, val deviceName: String) : Task

        class KeepSession(val pairing: StoredPairing) : Task
    }

    private sealed interface SessionOutcome {
        /** The computer revoked this device or does not know it. Reconnecting cannot help. */
        data object Unpaired : SessionOutcome

        /** The computer answers, but refuses this client for a reason that a retry does not change. */
        class Refused(val problem: String) : SessionOutcome

        class Lost(val problem: String, val afterWelcome: Boolean) : SessionOutcome
    }

    private val mutableState = MutableStateFlow(UiState(pair = PairForm(deviceName = container.defaultDeviceName)))
    val state: StateFlow<UiState> = mutableState.asStateFlow()

    private val task = MutableStateFlow<Task>(Task.Idle)

    /** A count, not a flag: a recreated activity may start before the old one has reported its stop. */
    private val startedActivities = MutableStateFlow(0)

    @Volatile private var stored: StoredPairing? = null

    @Volatile private var liveSession: Session? = null

    private val client =
        SwayConnectClient(
            AnnouncingTransport(container.transport) { setConnection(ConnectionStatus.Authenticating) },
            container.io,
            container.clientInfo,
        )

    init {
        viewModelScope.launch(container.io) { loadStoredPairing() }
        viewModelScope.launch(container.io) { runConnection() }
    }

    /**
     * Lets the session of the stored pairing live until the caller is cancelled. The activity calls
     * it inside `repeatOnLifecycle(STARTED)`, so the session closes when the app goes to the background.
     */
    suspend fun connectWhileStarted() {
        startedActivities.update { it + 1 }
        try {
            awaitCancellation()
        } finally {
            startedActivities.update { it - 1 }
        }
    }

    /**
     * Runs the task of the moment for the life of the view model. A session needs a started activity.
     * A pairing attempt does not: the first proof uses up its window on the computer, so a screen
     * timeout or a rotation while the owner answers must not end it.
     */
    private suspend fun runConnection() {
        task
            .combine(startedActivities) { current, started ->
                if (current is Task.KeepSession && started == 0) Task.Idle else current
            }
            .distinctUntilChanged()
            .collectLatest { current ->
                // collectLatest starts this block on the thread that delivers the next task, and a
                // socket read blocks its thread. Run inline, a session would hold that thread until
                // the connection ends by itself: no stop, no forget and no new pairing would reach it.
                coroutineScope { launch { runTask(current) } }
            }
    }

    private suspend fun runTask(current: Task) {
        when (current) {
            Task.Idle -> setConnection(ConnectionStatus.Disconnected)
            is Task.Pairing -> pair(current)
            is Task.KeepSession -> keepSession(current)
        }
    }

    // ---- Pair screen ----

    /** A link from another app. It only fills the form. */
    fun onPairingLink(link: String) {
        mutableState.update {
            if (it.pair.phase != PairPhase.Editing) {
                it
            } else {
                // An open "Replace?" dialog closes: it was answered for the link it was opened with.
                it.copy(screen = Screen.Pair, pair = it.pair.withLink(link).copy(confirmingReplace = false))
            }
        }
    }

    fun onLinkEdited(text: String) {
        editPair { it.withLink(text) }
    }

    fun onDeviceNameEdited(text: String) {
        editPair { it.copy(deviceName = text) }
    }

    /** The Pair button. With a stored pairing it first asks for the second confirmation. */
    fun requestPairing() {
        val form = mutableState.value.pair
        if (form.phase != PairPhase.Editing) return
        if (!isDisplayLabel(form.deviceName)) {
            editPair { it.copy(error = "The device name needs 1 to 64 bytes and no control characters.") }
            return
        }
        if (stored != null) editPair { it.copy(confirmingReplace = true) } else beginPairing(form)
    }

    fun confirmReplace() {
        val form = mutableState.value.pair
        if (form.phase == PairPhase.Editing && form.confirmingReplace) beginPairing(form)
    }

    fun dismissReplace() {
        editPair { it.copy(confirmingReplace = false) }
    }

    /**
     * The form comes back at once, not when the attempt ends: a connect that blocks already cannot
     * be interrupted and returns up to 20 s later.
     */
    fun cancelPairing() {
        val current = task.value
        if (current is Task.Pairing) endPairing(current, problem = null)
    }

    fun showPairScreen() {
        mutableState.update { it.copy(screen = Screen.Pair) }
    }

    fun showDesktop() {
        mutableState.update { if (it.server == null) it else it.copy(screen = Screen.Desktop) }
    }

    // ---- Desktop screen ----

    /** Selects the workspace whose windows are listed, and focuses it when the computer offers that. */
    fun selectWorkspace(workspaceId: Long) {
        mutableState.update { it.copy(selectedWorkspaceId = workspaceId) }
        val offered = mutableState.value.granted?.ops?.contains(Op.WORKSPACE_FOCUS) == true
        if (offered) act(workspaceId) { it.focusWorkspace(workspaceId) }
    }

    fun focusWindow(windowId: Long) {
        act(windowId) { it.focusWindow(windowId) }
    }

    fun enterFullscreen(windowId: Long) {
        act(windowId) { it.setFullscreen(windowId, true) }
    }

    fun leaveFullscreen(windowId: Long) {
        act(windowId) { it.setFullscreen(windowId, false) }
    }

    /** Ends the wait between two connection attempts: the backoff grows to 30 s. */
    fun retryNow() {
        val waiting = task.value
        if (waiting is Task.KeepSession && mutableState.value.connection == ConnectionStatus.Backoff) {
            // A new instance is a new task for the connection loop, which starts again at once.
            task.compareAndSet(waiting, Task.KeepSession(waiting.pairing))
        }
    }

    /** Removes the pairing from this phone. The computer keeps the device until it is revoked there. */
    fun forget() {
        viewModelScope.launch(container.io) {
            if (!clearStoredPairing()) return@launch
            stored = null
            task.value = Task.Idle
            mutableState.value = UiState(pair = PairForm(deviceName = container.defaultDeviceName), screen = Screen.Pair)
        }
    }

    // ---- Stored pairing ----

    private fun loadStoredPairing() {
        val pairing =
            try {
                container.profileStore.load()
            } catch (e: StoredPairingLostException) {
                val error = "The stored pairing can no longer be read. Pair again."
                mutableState.update { it.copy(screen = Screen.Pair, pair = it.pair.copy(error = error)) }
                return
            }
        if (pairing == null) {
            mutableState.update { it.copy(screen = Screen.Pair) }
            return
        }
        adopt(pairing)
        // Not a plain write: a pairing attempt that began while the profile was read keeps the
        // connection. The session follows when that attempt ends.
        task.compareAndSet(Task.Idle, Task.KeepSession(pairing))
    }

    private fun adopt(pairing: StoredPairing) {
        stored = pairing
        mutableState.update {
            // A link that arrived while the profile was read keeps the pair screen in front.
            val screen = if (it.screen == Screen.Loading) Screen.Desktop else it.screen
            it.copy(screen = screen, server = ServerSummary.of(pairing.profile), unpaired = false)
        }
    }

    private fun taskWithoutPairing(): Task = stored?.let { Task.KeepSession(it) } ?: Task.Idle

    /** Deletes the profile and the key. False when the phone refuses; the notice line says so then. */
    private fun clearStoredPairing(): Boolean =
        try {
            container.profileStore.clear()
            true
        } catch (e: IOException) {
            mutableState.update { it.copy(notice = "The pairing could not be removed from this phone.") }
            false
        }

    // ---- Pairing ----

    private fun beginPairing(form: PairForm) {
        val uri =
            try {
                parsePairingUri(form.link.trim())
            } catch (e: PairingUriException) {
                editPair { it.copy(error = NOT_A_PAIRING_LINK, confirmingReplace = false) }
                return
            }
        editPair { it.copy(phase = PairPhase.Connecting, error = null, confirmingReplace = false) }
        task.value = Task.Pairing(uri, form.deviceName)
    }

    private suspend fun pair(request: Task.Pairing) {
        try {
            val paired = client.pair(request.uri, request.deviceName, ::showConfirmCode)
            val pairing = StoredPairing(paired.profile, paired.key)
            // The old profile and key are replaced only here, after `pairing.confirmed`.
            try {
                container.profileStore.save(pairing)
            } catch (e: IOException) {
                val problem =
                    "The computer accepted this device, but the key could not be stored on this phone. " +
                        "Revoke the device on the computer and pair again."
                endPairing(request, problem)
                return
            }
            mutableState.update {
                it.copy(screen = Screen.Desktop, pair = PairForm(deviceName = it.pair.deviceName), connectionProblem = null)
            }
            adopt(pairing)
            task.value = Task.KeepSession(pairing)
        } catch (e: IOException) {
            endPairing(request, pairingProblem(e))
        }
    }

    private fun showConfirmCode(code: String) {
        editPair {
            // Only a form that still waits takes the code: the user may have cancelled a moment ago.
            if (it.phase == PairPhase.Connecting) {
                it.copy(phase = PairPhase.AwaitingConfirmation, confirmCode = replaceUnsafe(code))
            } else {
                it
            }
        }
    }

    /**
     * Back to the form with [problem]. The link is dropped: its secret is single use and must not
     * outlive the attempt (docs/PROTOCOL.md section 4 rule 5).
     *
     * Only the first call for [request] changes anything. A cancelled attempt can still fail many
     * seconds later, and that must not wipe what the user typed since.
     */
    private fun endPairing(request: Task.Pairing, problem: String?) {
        if (!task.compareAndSet(request, taskWithoutPairing())) return
        mutableState.update { it.copy(pair = PairForm(deviceName = it.pair.deviceName, error = problem)) }
    }

    private fun pairingProblem(failure: IOException): String =
        when (failure) {
            is PinMismatchException ->
                "The computer presented a different certificate than the link names. Nothing was sent."
            is RejectedException ->
                if (failure.code == RejectCode.PAIRING_FAILED) {
                    "The computer refused the pairing. Open a new pairing window and use the new link."
                } else {
                    rejectionProblem(failure.code)
                }
            is PairingNotConfirmedException ->
                if (failure.end is SessionEnd.Lost) {
                    // Also when Android closed the socket of this app in the background.
                    "The connection ended before the computer answered. Pair again and keep this app open until then."
                } else {
                    "The pairing was not confirmed on the computer."
                }
            is HandshakeException -> HANDSHAKE_PROBLEM
            else -> "The computer could not be reached."
        }

    private fun rejectionProblem(code: String): String =
        if (code == RejectCode.UNSUPPORTED_VERSION) {
            "The computer speaks another protocol version."
        } else {
            "The computer refused the connection (${replaceUnsafe(code)})."
        }

    // ---- Session ----

    private suspend fun keepSession(request: Task.KeepSession) {
        var backoffMs = INITIAL_BACKOFF_MS
        while (true) {
            setConnection(ConnectionStatus.Connecting)
            when (val outcome = runOneSession(request.pairing)) {
                SessionOutcome.Unpaired -> {
                    forgetRevoked(request)
                    return
                }
                is SessionOutcome.Refused -> {
                    mutableState.update {
                        it.copy(connection = ConnectionStatus.Disconnected, connectionProblem = outcome.problem)
                    }
                    // No reconnect loop: the same frame would be refused again. The task stays, so
                    // the next start of the activity asks once more.
                    awaitCancellation()
                }
                is SessionOutcome.Lost -> {
                    if (outcome.afterWelcome) backoffMs = INITIAL_BACKOFF_MS
                    mutableState.update {
                        it.copy(connection = ConnectionStatus.Backoff, connectionProblem = outcome.problem)
                    }
                    delay(backoffMs)
                    backoffMs = (backoffMs * 2).coerceAtMost(MAX_BACKOFF_MS)
                }
            }
        }
    }

    /**
     * docs/PROTOCOL.md 6.4: a revoked credential is forgotten and not sent again. It leaves the
     * memory even when the files resist, so that this run cannot offer the dead key once more.
     */
    private fun forgetRevoked(request: Task.KeepSession) {
        stored = null
        clearStoredPairing()
        mutableState.update { it.copy(unpaired = true, connectionProblem = null, server = null) }
        task.compareAndSet(request, Task.Idle)
    }

    private suspend fun runOneSession(pairing: StoredPairing): SessionOutcome =
        try {
            when (val end = client.runSession(pairing.profile, pairing.key, ::showSession)) {
                is SessionEnd.ByServer ->
                    if (end.code == ByeCode.REVOKED) {
                        SessionOutcome.Unpaired
                    } else {
                        val problem = "The computer closed the connection (${replaceUnsafe(end.code)})."
                        SessionOutcome.Lost(problem, afterWelcome = true)
                    }
                is SessionEnd.Lost, SessionEnd.PairingConfirmed ->
                    SessionOutcome.Lost("The connection was lost.", afterWelcome = true)
            }
        } catch (e: PinMismatchException) {
            // Its own error, not a network failure (docs/DESIGN.md 8.4).
            val problem = "The computer presented a different certificate than the paired one. Nothing was sent."
            SessionOutcome.Lost(problem, afterWelcome = false)
        } catch (e: RejectedException) {
            if (e.code == RejectCode.AUTH_FAILED) {
                SessionOutcome.Unpaired
            } else {
                // Every other code, known or not, means that the same `auth` is refused again
                // (docs/PROTOCOL.md 5.2 and 13.2 rule 5).
                SessionOutcome.Refused(rejectionProblem(e.code))
            }
        } catch (e: HandshakeException) {
            // Among others: `hello` offers no version this app speaks. No `auth` was sent then.
            SessionOutcome.Refused(HANDSHAKE_PROBLEM)
        } catch (e: IOException) {
            SessionOutcome.Lost("The computer cannot be reached.", afterWelcome = false)
        } finally {
            liveSession = null
            mutableState.update { it.copy(granted = null).withDesktop(null) }
        }

    /** Runs next to the reader of a session, after `welcome`. */
    private suspend fun showSession(session: Session) {
        liveSession = session
        val welcome = session.welcome
        val granted = Granted(welcome.ops.toSet(), welcome.scopes.map(::replaceUnsafe), welcome.readOnly)
        mutableState.update {
            // The notice of an action belongs to the connection that carried the action.
            it.copy(connection = ConnectionStatus.Ready, connectionProblem = null, granted = granted, notice = null)
        }
        try {
            session.subscribeDesktop()
        } catch (e: RequestFailedException) {
            // The session stays open; this device simply may not read the state.
            mutableState.update { it.copy(notice = "The computer does not let this device read the desktop.") }
            return
        }
        session.desktop.collect { desktop -> mutableState.update { it.withDesktop(desktop) } }
    }

    /**
     * Sends one action. The target shows a spinner until the answer or the next state arrives,
     * whichever is first. An action is never sent again on its own (docs/PROTOCOL.md section 11),
     * and a tap on a target that still waits sends nothing.
     */
    private fun act(targetId: Long, send: suspend (Session) -> Unit) {
        val session = liveSession ?: return
        if (mutableState.value.pendingTargetId == targetId) return
        mutableState.update { it.copy(pendingTargetId = targetId, notice = null) }
        viewModelScope.launch(container.io) {
            val refusal =
                try {
                    send(session)
                    null
                } catch (e: RequestFailedException) {
                    actionRefusal(e)
                } catch (e: IOException) {
                    "The computer did not answer. The action is not sent again."
                }
            mutableState.update {
                // A later action may wait by now: this answer ends only its own spinner and wipes no other notice.
                val stillWaiting = it.pendingTargetId.takeIf { pending -> pending != targetId }
                it.copy(pendingTargetId = stillWaiting, notice = refusal ?: it.notice)
            }
        }
    }

    private fun actionRefusal(failure: RequestFailedException): String =
        when (failure.code) {
            ErrorCode.READ_ONLY -> "The computer is in read-only mode."
            ErrorCode.RATE_LIMITED -> "Too many actions. Wait a moment."
            else -> "The computer refused the action (${replaceUnsafe(failure.code)})."
        }

    private fun setConnection(status: ConnectionStatus) {
        mutableState.update { it.copy(connection = status) }
    }

    private fun editPair(change: (PairForm) -> PairForm) {
        mutableState.update { it.copy(pair = change(it.pair)) }
    }

    class Factory(private val container: AppContainer) : ViewModelProvider.Factory {
        @Suppress("UNCHECKED_CAST")
        override fun <T : ViewModel> create(modelClass: Class<T>): T {
            require(modelClass.isAssignableFrom(MainViewModel::class.java)) { "unknown view model ${modelClass.name}" }
            return MainViewModel(container) as T
        }
    }

    private companion object {
        const val INITIAL_BACKOFF_MS = 1_000L
        const val MAX_BACKOFF_MS = 30_000L
        const val HANDSHAKE_PROBLEM = "The computer did not complete the handshake correctly."
    }
}

/** Tells the view model when TCP and TLS are up, which is where "Connecting" turns into "Authenticating". */
private class AnnouncingTransport(private val inner: Transport, private val onOpened: () -> Unit) : Transport {
    @Throws(IOException::class)
    override fun open(host: String, port: Int, pinnedFingerprint: ByteArray): Connection =
        inner.open(host, port, pinnedFingerprint).also { onOpened() }
}
