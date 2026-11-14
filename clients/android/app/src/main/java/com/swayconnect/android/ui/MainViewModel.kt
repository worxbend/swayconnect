package com.swayconnect.android.ui

import androidx.lifecycle.ViewModel
import androidx.lifecycle.ViewModelProvider
import androidx.lifecycle.viewModelScope
import com.swayconnect.android.AppContainer
import com.swayconnect.android.data.IconFetch
import com.swayconnect.android.data.StoredPairing
import com.swayconnect.android.data.StoredPairingLostException
import com.swayconnect.android.ui.state.Area
import com.swayconnect.android.ui.state.ConnectionStatus
import com.swayconnect.android.ui.state.DriveController
import com.swayconnect.android.ui.state.DriveMode
import com.swayconnect.android.ui.state.Granted
import com.swayconnect.android.ui.state.LatestValueSender
import com.swayconnect.android.ui.state.LaunchTarget
import com.swayconnect.android.ui.state.NOT_A_PAIRING_LINK
import com.swayconnect.android.ui.state.PagerFacts
import com.swayconnect.android.ui.state.PairForm
import com.swayconnect.android.ui.state.PairPhase
import com.swayconnect.android.ui.state.PendingControl
import com.swayconnect.android.ui.state.PendingLaunch
import com.swayconnect.android.ui.state.Refusal
import com.swayconnect.android.ui.state.Screen
import com.swayconnect.android.ui.state.ServerTarget
import com.swayconnect.android.ui.state.ServerSummary
import com.swayconnect.android.ui.state.Sheet
import com.swayconnect.android.ui.state.UiState
import com.swayconnect.android.ui.state.ViewMode
import com.swayconnect.android.ui.state.followChip
import com.swayconnect.android.ui.state.isDriving
import com.swayconnect.android.ui.state.msUntilCountdownTick
import com.swayconnect.android.ui.state.withApps
import com.swayconnect.android.ui.state.withAppsStale
import com.swayconnect.android.ui.state.withArea
import com.swayconnect.android.ui.state.withAudio
import com.swayconnect.android.ui.state.withAudioStale
import com.swayconnect.android.ui.state.withInputRefusal
import com.swayconnect.android.ui.state.withKeysFollowingSession
import com.swayconnect.android.ui.state.withKeysRefusalCleared
import com.swayconnect.android.ui.state.withKeysTarget
import com.swayconnect.android.ui.state.withLauncherTarget
import com.swayconnect.android.ui.state.withLaunchStarted
import com.swayconnect.android.ui.state.withLaunchTimedOut
import com.swayconnect.android.ui.state.withMedia
import com.swayconnect.android.ui.state.withMediaStale
import com.swayconnect.android.ui.state.withModifierToggled
import com.swayconnect.android.ui.state.withModifiersReleased
import com.swayconnect.android.ui.state.withNowPlayingHidden
import com.swayconnect.android.ui.state.withPrivateMode
import com.swayconnect.android.ui.state.withTargetFromRefusal
import com.swayconnect.android.ui.state.withTyped
import com.swayconnect.android.ui.state.withVolumeAnswered
import com.swayconnect.android.ui.state.withVolumeDragged
import com.swayconnect.android.ui.state.withVolumeFailed
import com.swayconnect.android.ui.state.withVolumeReleased
import com.swayconnect.android.ui.state.withClock
import com.swayconnect.android.ui.state.withCachedDesktop
import com.swayconnect.android.ui.state.withDesktop
import com.swayconnect.android.ui.state.withDesktopStale
import com.swayconnect.android.ui.state.withLink
import com.swayconnect.android.ui.state.withSession
import com.swayconnect.android.ui.state.withSessionStale
import com.swayconnect.protocol.ByeReaction
import com.swayconnect.protocol.Desktop
import com.swayconnect.protocol.Connection
import com.swayconnect.protocol.ErrorCode
import com.swayconnect.protocol.HandshakeException
import com.swayconnect.protocol.Holder
import com.swayconnect.protocol.InputKey
import com.swayconnect.protocol.InputRefusal
import com.swayconnect.protocol.InputTarget
import com.swayconnect.protocol.InputText
import com.swayconnect.protocol.KEEP_AWAKE_MIN_DURATION_S
import com.swayconnect.protocol.KeyModifier
import com.swayconnect.protocol.MAX_WORKSPACE_NUMBER
import com.swayconnect.protocol.MediaCommand
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
import com.swayconnect.protocol.Topic
import com.swayconnect.protocol.Transport
import com.swayconnect.protocol.WindowLayout
import com.swayconnect.protocol.closeWindow
import com.swayconnect.protocol.controlMedia
import com.swayconnect.protocol.isDisplayLabel
import com.swayconnect.protocol.keepAwakeOff
import com.swayconnect.protocol.keepAwakeOn
import com.swayconnect.protocol.launchApp
import com.swayconnect.protocol.lockScreen
import com.swayconnect.protocol.moveToScratchpad
import com.swayconnect.protocol.moveWindowToNumber
import com.swayconnect.protocol.moveWindowToWorkspace
import com.swayconnect.protocol.openLauncher
import com.swayconnect.protocol.parsePairingUri
import com.swayconnect.protocol.pressKey
import com.swayconnect.protocol.replaceUnsafe
import com.swayconnect.protocol.setDefaultDevice
import com.swayconnect.protocol.setFloating
import com.swayconnect.protocol.setLayout
import com.swayconnect.protocol.setMuted
import com.swayconnect.protocol.setVolume
import com.swayconnect.protocol.showScratchpad
import com.swayconnect.protocol.typeText
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
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.flow.updateAndGet
import kotlinx.coroutines.Job
import kotlinx.coroutines.launch
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.longOrNull

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

        /** The computer presented another certificate than the paired one. No auto-retry (docs/UX.md 16). */
        data object WrongKey : SessionOutcome

        /** `bye` `scopes_changed`: connect again at once, without backoff (docs/PROTOCOL.md 17.15). */
        data object ScopesChanged : SessionOutcome
    }

    /** A live desktop state to write to the cache, with what decides its titles. */
    private class Snapshot(val fp: String, val desktop: Desktop, val seenAtMs: Long, val keepTitles: Boolean)

    private val mutableState = MutableStateFlow(UiState(pair = PairForm(deviceName = container.defaultDeviceName)))
    val state: StateFlow<UiState> = mutableState.asStateFlow()

    /** For `LocalIconCache` (slice A2 provides it around the shell). */
    val iconCache get() = container.iconCache

    private val task = MutableStateFlow<Task>(Task.Idle)

    /** A count, not a flag: a recreated activity may start before the old one has reported its stop. */
    private val startedActivities = MutableStateFlow(0)

    @Volatile private var stored: StoredPairing? = null

    @Volatile private var liveSession: Session? = null

    /** Guarded by itself. */
    private val drive = DriveController()

    @Volatile private var driveJob: Job? = null

    @Volatile private var pagerFacts = PagerFacts()

    private val snapshots = MutableStateFlow<Snapshot?>(null)

    /** The volume sliders' throttle (docs/UX.md 7.2): device id to volume. */
    private val volumeSender = LatestValueSender<Long, Int>(VOLUME_INTERVAL_MS, ::sendVolume)

    private val client =
        SwayConnectClient(
            AnnouncingTransport(container.transport) { setConnection(ConnectionStatus.Authenticating) },
            container.io,
            container.clientInfo,
        )

    init {
        viewModelScope.launch(container.io) { loadStoredPairing() }
        viewModelScope.launch(container.io) { runConnection() }
        viewModelScope.launch(container.io) { saveSnapshots() }
        viewModelScope.launch(container.io) { runCountdown() }
        viewModelScope.launch(container.io) { volumeSender.run() }
        container.iconCache?.let { cache -> viewModelScope.launch(container.io) { cache.run() } }
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

    /** "Done" after a confirmed pairing: the Desktop area follows (UX 11 step 4), whatever area was shown before. */
    fun finishPairing() {
        mutableState.update {
            if (it.pair.phase != PairPhase.Confirmed) {
                it
            } else {
                it.copy(screen = Screen.Desktop, pair = PairForm(deviceName = it.pair.deviceName)).withArea(Area.Desktop)
            }
        }
    }

    fun showPairScreen() {
        mutableState.update { it.copy(screen = Screen.Pair) }
    }

    fun showDesktop() {
        mutableState.update { if (it.server == null) it else it.copy(screen = Screen.Desktop) }
    }

    // ---- Shell ----

    /** A tap on the pill navigation. An open sheet closes; opening Keys captures the focused window as its target. */
    fun selectArea(area: Area) {
        mutableState.update { it.withArea(area) }
    }

    fun openSheet(sheet: Sheet) {
        mutableState.update { it.copy(sheet = sheet) }
    }

    fun closeSheet() {
        mutableState.update { it.copy(sheet = null) }
    }

    /**
     * Back (docs/UX.md 2 rule 2): closes the sheet, else returns to Desktop. False on Desktop with no sheet:
     * the app leaves Back to the system then.
     */
    fun goBack(): Boolean {
        val before = mutableState.value
        return when {
            before.sheet != null -> true.also { closeSheet() }
            before.area != Area.Desktop -> true.also { selectArea(Area.Desktop) }
            else -> false
        }
    }

    fun setViewMode(mode: ViewMode) {
        mutableState.update { it.copy(viewMode = mode) }
    }

    /** The Drive / Browse pill. A waiting settle is dropped when Drive turns off. */
    fun setDriveMode(mode: DriveMode) {
        if (mode == DriveMode.Browse) driveJob?.cancel()
        mutableState.update { it.copy(drive = mode) }
    }

    fun clearDrivePaused() {
        mutableState.update { it.copy(drivePaused = false) }
    }

    // ---- Desktop screen ----

    /** Selects the workspace whose windows are listed, and focuses it when the computer offers that. Used by the v1 screen. */
    fun selectWorkspace(workspaceId: Long) {
        mutableState.update { it.copy(selectedWorkspaceId = workspaceId) }
        showOnComputer(workspaceId)
    }

    /** "Show on computer" (Browse mode) and the workspace sheet: focuses [workspaceId] on the computer. */
    fun showOnComputer(workspaceId: Long) {
        if (mutableState.value.granted?.ops?.contains(Op.WORKSPACE_FOCUS) == true) {
            act(workspaceId) { it.focusWorkspace(workspaceId) }
        }
    }

    /** A finger went down on, or came up from, the pager or the card row. Feeds the follow-focus guard. */
    fun onPagerTouch(down: Boolean) {
        pagerFacts = pagerFacts.copy(fingerDown = down, lastTouchAtMs = monotonicMs())
    }

    /** TalkBack's touch exploration turned on or off. While on, the pager never follows the computer. */
    fun onTouchExplorationChanged(enabled: Boolean) {
        pagerFacts = pagerFacts.copy(touchExploration = enabled)
    }

    /** A user drag of the pager or of the card row began (`DragInteraction.Start`). Never for a programmatic scroll. */
    fun onUserDrag() {
        val shownId = mutableState.value.selectedWorkspaceId
        synchronized(drive) { drive.userDrag(shownId) }
    }

    /**
     * A tap on a workspace card: the phone shows it, and the settle that follows counts as user-initiated.
     * A tap on the card that is already shown settles at once, since the pager does not move.
     */
    fun onCardTap(workspaceId: Long) {
        synchronized(drive) { drive.cardTap() }
        val alreadyShown = mutableState.value.selectedWorkspaceId == workspaceId
        mutableState.update { it.copy(selectedWorkspaceId = workspaceId) }
        if (alreadyShown) onPageSettled(workspaceId)
    }

    /**
     * The pager settled on [workspaceId], read from the pages at settle time. Call it after every scroll that
     * stops, programmatic or not. In Drive mode a user-initiated settle that holds for 250ms sends one
     * `workspace.focus` (docs/UX.md 5.5).
     */
    fun onPageSettled(workspaceId: Long) {
        mutableState.update { it.copy(selectedWorkspaceId = workspaceId) }
        synchronized(drive) { drive.settled(workspaceId, monotonicMs()) }
        driveJob?.cancel()
        driveJob =
            viewModelScope.launch(container.io) {
                delay(DriveController.HOLD_MS)
                driveDueFocus()
            }
    }

    /** A tap on "Computer moved to N": the phone shows the computer's workspace. It sends nothing. */
    fun followMovedChip() {
        mutableState.update { it.followChip() }
    }

    fun dismissListHint() {
        mutableState.update { it.copy(listHintDismissed = true) }
    }

    fun dismissMovedChip() {
        mutableState.update { it.copy(followFocusChip = null) }
    }

    private fun driveDueFocus() {
        val current = mutableState.value
        val focusedId = current.desktop?.workspaces?.firstOrNull { it.focused }?.id
        val pageIds = current.desktop?.workspaces?.map { it.id }.orEmpty()
        val target = synchronized(drive) { drive.due(monotonicMs(), current.selectedWorkspaceId, focusedId, pageIds) }
        if (target == null || !current.isDriving) return
        act(target, ::driveRefusal) { it.focusWorkspace(target) }
    }

    /** docs/UX.md 5.5 rules 4 and 5. Drive never retries. */
    private fun driveRefusal(state: UiState, failure: RequestFailedException): UiState =
        when (failure.code) {
            ErrorCode.RATE_LIMITED, ErrorCode.READ_ONLY -> state.copy(drivePaused = true)
            UNSUPPORTED_TARGET -> state.copy(notice = "This workspace cannot be shown from the phone.")
            else -> actionRefusal(state, failure)
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

    fun moveWindow(windowId: Long, workspaceId: Long) {
        act(windowId) { it.moveWindowToWorkspace(windowId, workspaceId) }
    }

    /** Sway creates the workspace when none has [number]. */
    fun moveWindowToNumber(windowId: Long, number: Int) {
        require(number in 0..MAX_WORKSPACE_NUMBER) { "workspace number must be 0 to $MAX_WORKSPACE_NUMBER" }
        act(windowId) { it.moveWindowToNumber(windowId, number) }
    }

    fun floatWindow(windowId: Long) {
        act(windowId) { it.setFloating(windowId, true) }
    }

    fun tileWindow(windowId: Long) {
        act(windowId) { it.setFloating(windowId, false) }
    }

    fun setWindowLayout(windowId: Long, layout: WindowLayout) {
        act(windowId) { it.setLayout(windowId, layout) }
    }

    fun sendToScratchpad(windowId: Long) {
        act(windowId) { it.moveToScratchpad(windowId) }
    }

    /** "Show" in the Scratchpad sheet. [scratchpadId] is an id of `desktop.scratchpad`, not of `windows`. */
    fun showScratchpadWindow(scratchpadId: Long) {
        act(scratchpadId) { it.showScratchpad(scratchpadId) }
    }

    /** Close asks first (docs/PROTOCOL.md 17.7): this opens the confirm sheet, [confirmClose] sends. */
    fun askToCloseWindow(windowId: Long) {
        openSheet(Sheet.ConfirmClose(windowId))
    }

    /** The yes of the confirm sheet: closes the window that sheet names, and nothing without that sheet. */
    fun confirmClose() {
        val confirmed = mutableState.value.sheet as? Sheet.ConfirmClose ?: return
        closeSheet()
        act(confirmed.windowId) { it.closeWindow(confirmed.windowId) }
    }

    // ---- More area ----

    /**
     * Starts keep-awake or replaces its expiry (docs/UX.md 10.2). [durationS] null takes the server's default;
     * otherwise it is one of the duration sheet's choices, at least [KEEP_AWAKE_MIN_DURATION_S].
     */
    fun keepAwakeOn(durationS: Long? = null) {
        require(durationS == null || durationS >= KEEP_AWAKE_MIN_DURATION_S) { "keep-awake lasts at least a minute" }
        act(ServerTarget.KEEP_AWAKE) { it.keepAwakeOn(durationS) }
    }

    fun keepAwakeOff() {
        act(ServerTarget.KEEP_AWAKE) { it.keepAwakeOff() }
    }

    /** The Lock tile and the round lock button. No confirm: locking is the safe direction (docs/UX.md 10.3). */
    fun lockComputer() {
        act(ServerTarget.LOCK) { session ->
            if (session.lockScreen()) mutableState.update { it.copy(notice = ALREADY_LOCKED) }
        }
    }

    // ---- Audio area ----

    /**
     * The finger moved the slider of [deviceId] to [pct]. The slider shows its own value; at most 10 values a second
     * go out (docs/UX.md 7.2).
     */
    fun dragVolume(deviceId: Long, pct: Int) {
        if (!canCall(Op.AUDIO_SET_VOLUME)) return
        val draft = mutableState.updateAndGet { it.withVolumeDragged(deviceId, pct) }.audio?.drafts?.get(deviceId) ?: return
        volumeSender.offer(deviceId, draft.pct)
    }

    /** `onValueChangeFinished` at [pct]: this value is always sent, and the slider keeps it until the next state. */
    fun finishVolume(deviceId: Long, pct: Int) {
        if (!canCall(Op.AUDIO_SET_VOLUME)) return
        val draft = mutableState.updateAndGet { it.withVolumeReleased(deviceId, pct) }.audio?.drafts?.get(deviceId) ?: return
        volumeSender.offer(deviceId, draft.pct)
    }

    /** A phone volume key while Audio is shown: the default output moves by [deltaPct] (docs/UX.md 7.2). */
    fun stepDefaultVolume(deltaPct: Int) {
        val audio = mutableState.value.audio ?: return
        val sink = audio.defaultSink ?: return
        finishVolume(sink.id, audio.shownVolume(sink) + deltaPct)
    }

    /** Unmuting a source needs `audio.capture`; the server enforces it and the UI shows the lock. */
    fun setMuted(deviceId: Long, muted: Boolean) {
        actOn(PendingControl.Device(deviceId), Op.AUDIO_SET_MUTE) { it.setMuted(deviceId, muted) }
    }

    fun setDefaultDevice(deviceId: Long) {
        actOn(PendingControl.Device(deviceId), Op.AUDIO_SET_DEFAULT) { it.setDefaultDevice(deviceId) }
    }

    fun controlMedia(playerId: Long, command: MediaCommand) {
        actOn(PendingControl.Player(playerId), command.op) { it.controlMedia(playerId, command) }
    }

    /** "Hide until next track" on the now-playing capsule (docs/UX.md 4.3). */
    fun hideNowPlaying() {
        mutableState.update { it.withNowPlayingHidden() }
    }

    /** One send of the throttle. It never throws: a refusal shows its line, and a failed final value shows the server's. */
    private suspend fun sendVolume(deviceId: Long, pct: Int) {
        val session = liveSession
        if (session == null) {
            mutableState.update { it.withVolumeFailed(deviceId, pct) }
            return
        }
        val failure: IOException? =
            try {
                session.setVolume(deviceId, pct)
                null
            } catch (e: IOException) {
                e
            }
        mutableState.update {
            when (failure) {
                null -> it.withVolumeAnswered(deviceId, pct)
                is RequestFailedException -> actionRefusal(it, failure).withVolumeFailed(deviceId, pct)
                else -> it.copy(notice = Refusal.NO_ANSWER).withVolumeFailed(deviceId, pct)
            }
        }
    }

    // ---- Launch area ----

    /**
     * Starts the app [entryId] of the `apps` state, on workspace [number] or, when null, on the computer's focused
     * workspace. After `ok` the app returns to Desktop with the ghost tile (docs/UX.md 8.2). A risky app needs the
     * user's hold first (17.19 item 18); the UI asks before it calls this.
     */
    fun launchApp(entryId: String, number: Int? = null) {
        require(number == null || number in 0..MAX_WORKSPACE_NUMBER) { "workspace number must be 0 to $MAX_WORKSPACE_NUMBER" }
        val before = mutableState.value
        val app = before.apps?.state?.apps?.firstOrNull { it.entryId == entryId } ?: return
        val target =
            number?.let(LaunchTarget::Number)
                ?: before.desktop?.workspaces?.firstOrNull { it.focused }?.let { LaunchTarget.Workspace(it.id) }
                ?: return
        val known = before.desktop?.windows.orEmpty().map { it.id }.toSet()
        actOn(PendingControl.Launch(entryId), Op.APP_LAUNCH) { session ->
            session.launchApp(entryId, number)
            val launch = PendingLaunch(entryId, app.name, app.icon, target, known, monotonicMs())
            mutableState.update { it.withLaunchStarted(launch) }
            viewModelScope.launch(container.io) {
                delay(PendingLaunch.TIMEOUT_MS)
                mutableState.update { it.withLaunchTimedOut(launch, "${replaceUnsafe(app.name)} did not open a window yet.") }
            }
        }
    }

    /**
     * The round ⌕ button: `launcher.open`. When this device holds the launcher and may type, Keys opens with the
     * launcher as its target; otherwise one line says it is open (docs/UX.md 8.2).
     */
    fun openComputerLauncher() {
        actOn(PendingControl.Launcher, Op.LAUNCHER_OPEN) { session ->
            val alreadyOpen = session.openLauncher()
            mutableState.update {
                val holder = !alreadyOpen || it.session?.state?.launcher?.holder == Holder.THIS_DEVICE
                if (holder && canCall(Op.INPUT_TEXT, it)) it.withLauncherTarget() else it.copy(notice = LAUNCHER_IS_OPEN)
            }
        }
    }

    // ---- Keys area ----

    /** Desktop's "Type" on [windowId]: Keys opens with that window as its target. */
    fun typeInto(windowId: Long) {
        mutableState.update { it.withArea(Area.Keys, windowId) }
    }

    /** The target picker; [InputTarget.Launcher] only while `launcherOffered`. */
    fun selectKeysTarget(target: InputTarget) {
        mutableState.update { it.withKeysTarget(target) }
    }

    fun setPrivateMode(enabled: Boolean) {
        mutableState.update { it.withPrivateMode(enabled) }
    }

    fun toggleModifier(modifier: KeyModifier) {
        mutableState.update { it.withModifierToggled(modifier) }
    }

    /**
     * Send (docs/UX.md 9.2). [parts] are the composer's text cut by `TextSplit`: each one passes [InputText.isValid].
     * They go in order and stop at the first error; `keys.typed` then says how much arrived. With [thenReturn] a
     * `Return` follows only when every part arrived.
     */
    fun sendText(parts: List<String>, thenReturn: Boolean) {
        require(parts.isNotEmpty() && parts.all(InputText::isValid)) { "every part must pass InputText.isValid" }
        val target = mutableState.value.keys?.target ?: return
        if (thenReturn && !canCall(Op.INPUT_KEY)) return
        actOn(PendingControl.Typing, Op.INPUT_TEXT) { session ->
            if (typeParts(session, target, parts) && thenReturn) pressOrRefuse(session, target, InputKey.RETURN, emptySet())
        }
    }

    /**
     * A special key or shortcut: [key] with [modifiers] and the armed sticky modifiers, which are released by it.
     * Ctrl+Alt with a function key switches the computer's terminal: it is not sent.
     */
    fun pressKey(key: InputKey, modifiers: Set<KeyModifier> = emptySet()) {
        val before = mutableState.value
        val keys = before.keys ?: return
        val target = keys.target ?: return
        if (before.pendingControl == PendingControl.Typing || !canCall(Op.INPUT_KEY)) return
        val held = keys.sticky + modifiers
        mutableState.update { it.withModifiersReleased() }
        if (key.isFunctionKey && KeyModifier.CTRL in held && KeyModifier.ALT in held) {
            mutableState.update { it.copy(notice = TERMINAL_SWITCH_REFUSED) }
            return
        }
        actOn(PendingControl.Typing, Op.INPUT_KEY) { session -> pressOrRefuse(session, target, key, held) }
    }

    /** "Type there" after `focus_changed`. */
    fun typeThere() {
        mutableState.update { it.withTargetFromRefusal() }
    }

    /** "Back to Alacritty" after `focus_changed`: focuses the target window again, then typing may go on. */
    fun focusKeysTarget() {
        val target = mutableState.value.keys?.target as? InputTarget.Window ?: return
        actOn(PendingControl.Typing, Op.WINDOW_FOCUS) { session ->
            session.focusWindow(target.windowId)
            mutableState.update { it.withKeysRefusalCleared() }
        }
    }

    fun dismissKeysRefusal() {
        mutableState.update { it.withKeysRefusalCleared() }
    }

    /** Types [parts] in order and records how much arrived. False when a refusal of 17.13 stopped it. */
    private suspend fun typeParts(session: Session, target: InputTarget, parts: List<String>): Boolean {
        val totalChars = parts.sumOf { it.codePointCount(0, it.length).toLong() }
        var deliveredChars = 0L
        var refusal: InputRefusal? = null
        try {
            for (part in parts) deliveredChars += typePacedPart(session, target, part)
        } catch (e: RequestFailedException) {
            // Another code goes to the refusal line of act; a broken detail reads as no answer. Both after the record.
            val stopped = InputRefusal.of(e) ?: throw e
            refusal = stopped
            deliveredChars += stopped.deliveredChars
        } finally {
            val stoppedBy = refusal
            mutableState.update { it.withTyped(deliveredChars, totalChars, stoppedBy) }
        }
        return refusal == null
    }

    /**
     * One part. After `rate_limited` it waits `detail.retry_after_ms` once and sends the part again, under the same
     * Typing spinner: the refused part was not delivered, so nothing that ran is sent twice (PROTOCOL 11). A long
     * paste thus outlasts the burst of the `input_text` class (17.14) instead of failing after about 4 KB.
     */
    private suspend fun typePacedPart(session: Session, target: InputTarget, part: String): Long =
        try {
            session.typeText(target, part)
        } catch (e: RequestFailedException) {
            if (e.code != ErrorCode.RATE_LIMITED) throw e
            delay((e.detail.longAt("retry_after_ms") ?: TYPING_RETRY_DEFAULT_MS).coerceIn(0L, TYPING_RETRY_MAX_MS))
            session.typeText(target, part)
        }

    private suspend fun pressOrRefuse(session: Session, target: InputTarget, key: InputKey, modifiers: Set<KeyModifier>) {
        try {
            session.pressKey(target, key, modifiers)
        } catch (e: RequestFailedException) {
            val refusal = InputRefusal.of(e) ?: throw e
            mutableState.update { it.withInputRefusal(refusal) }
        }
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
        container.desktopCache.load(pairing.profile.fp)?.let { cached ->
            // Only before the first live state: a session that is already up has the newer truth.
            mutableState.update { if (it.desktop == null) it.withCachedDesktop(cached.desktop, cached.savedAtMs) else it }
        }
        // Not a plain write: a pairing attempt that began while the profile was read keeps the
        // connection. The session follows when that attempt ends.
        task.compareAndSet(Task.Idle, Task.KeepSession(pairing))
    }

    /** Makes [pairing] the stored one. [also] changes the same state update. */
    private fun adopt(pairing: StoredPairing, also: (UiState) -> UiState = { it }) {
        stored = pairing
        mutableState.update {
            // A link that arrived while the profile was read keeps the pair screen in front.
            val screen = if (it.screen == Screen.Loading) Screen.Desktop else it.screen
            also(it.copy(screen = screen, server = ServerSummary.of(pairing.profile), unpaired = false, lostServerName = null, wrongKey = false))
        }
    }

    private fun taskWithoutPairing(): Task = stored?.let { Task.KeepSession(it) } ?: Task.Idle

    /**
     * Deletes the profile, the key and the desktop snapshot (it may hold window titles). False when the phone
     * refuses; the notice line says so then.
     */
    private fun clearStoredPairing(): Boolean =
        try {
            snapshots.value = null
            // Under the lock of the writer: a snapshot write that is under way cannot bring the file back.
            synchronized(container.desktopCache) { container.desktopCache.clear() }
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
            // The pair screen stays for "What this phone can do" (UX 11 step 4). It reads the scopes from the
            // `welcome` of the new session, so nothing granted by a previous computer may stay. One update with
            // the new server: an observer of Confirmed must never see the previous computer's name.
            adopt(pairing) {
                it.copy(
                    pair = PairForm(deviceName = it.pair.deviceName, phase = PairPhase.Confirmed),
                    granted = null,
                    session = null,
                    audio = null,
                    media = null,
                    apps = null,
                    keys = null,
                    connectionProblem = null,
                )
            }
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
                SessionOutcome.WrongKey -> {
                    mutableState.update { it.copy(connection = ConnectionStatus.Disconnected, wrongKey = true) }
                    // Not retried: another certificate is a possible impersonation, not an outage. The next
                    // start of the activity asks once more, as for a refusal.
                    awaitCancellation()
                }
                SessionOutcome.ScopesChanged -> backoffMs = INITIAL_BACKOFF_MS
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
        mutableState.update {
            it.copy(
                unpaired = true,
                lostServerName = it.server?.name,
                connectionProblem = null,
                server = null,
                session = null,
                audio = null,
                media = null,
                apps = null,
                keys = null,
            )
        }
        task.compareAndSet(request, Task.Idle)
    }

    private suspend fun runOneSession(pairing: StoredPairing): SessionOutcome =
        try {
            when (val end = client.runSession(pairing.profile, pairing.key, ::showSession)) {
                is SessionEnd.ByServer ->
                    when (ByeReaction.of(end.code)) {
                        ByeReaction.FORGET -> SessionOutcome.Unpaired
                        ByeReaction.RECONNECT_NOW -> SessionOutcome.ScopesChanged
                        ByeReaction.RECONNECT_BACKOFF -> {
                            val problem = "The computer closed the connection (${replaceUnsafe(end.code)})."
                            SessionOutcome.Lost(problem, afterWelcome = true)
                        }
                    }
                is SessionEnd.Lost, SessionEnd.PairingConfirmed ->
                    SessionOutcome.Lost("The connection was lost.", afterWelcome = true)
            }
        } catch (e: PinMismatchException) {
            // Its own error, not a network failure (docs/DESIGN.md 8.4). Nothing was sent.
            SessionOutcome.WrongKey
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
            driveJob?.cancel()
            mutableState.update {
                it.withDesktopStale().withSessionStale().withAudioStale().withMediaStale().withAppsStale().copy(pendingControl = null)
            }
        }

    /** Runs next to the reader of a session, after `welcome`. */
    private suspend fun showSession(session: Session) {
        liveSession = session
        val welcome = session.welcome
        val granted = Granted.of(welcome)
        mutableState.update {
            // The notice of an action belongs to the connection that carried the action.
            it.copy(connection = ConnectionStatus.Ready, connectionProblem = null, granted = granted, notice = null, wrongKey = false)
        }
        val icons = container.iconCache
        if (icons != null && Op.ICON_GET in welcome.ops) {
            val fetch: IconFetch = session::iconGet
            icons.attach(fetch)
            try {
                followDesktop(session)
            } finally {
                icons.detach(fetch)
            }
        } else {
            followDesktop(session)
        }
    }

    /**
     * Subscribes to `desktop`, and to `session`, `audio`, `media` and `apps` when the server offers them
     * (docs/PROTOCOL.md 17.1 rule 2), in one request.
     */
    private suspend fun followDesktop(session: Session) {
        val offered = OPTIONAL_TOPICS.filter { it in session.welcome.topics }
        try {
            session.subscribe(listOf(Topic.DESKTOP) + offered)
        } catch (e: RequestFailedException) {
            // The session stays open; this device simply may not read the state.
            mutableState.update { it.copy(notice = "The computer does not let this device read the desktop.") }
            return
        }
        coroutineScope {
            if (Topic.SESSION in offered) {
                launch { follow(session.sessionState) { state, next -> state.withSession(next, monotonicMs()).withKeysFollowingSession() } }
            }
            if (Topic.AUDIO in offered) launch { follow(session.audio, UiState::withAudio) }
            if (Topic.MEDIA in offered) launch { follow(session.media, UiState::withMedia) }
            if (Topic.APPS in offered) launch { follow(session.apps, UiState::withApps) }
            followDesktopStates(session)
        }
    }

    private suspend fun <T : Any> follow(topic: StateFlow<T?>, apply: (UiState, T) -> UiState) {
        topic.collect { next -> if (next != null) mutableState.update { apply(it, next) } }
    }

    /**
     * Moves [UiState.clockMs] each time the shown countdown changes: per minute, per second in the last minute
     * (docs/UX.md 10.2). It sleeps while nothing counts down and starts again with each new `session` state.
     */
    private suspend fun runCountdown() {
        mutableState
            .map { it.session?.let { view -> view.keepAwakeEndsAtMs to view.cooldownEndsAtMs } }
            .distinctUntilChanged()
            .collectLatest {
                while (true) {
                    val waitMs = mutableState.updateAndGet { it.withClock(monotonicMs()) }.msUntilCountdownTick() ?: break
                    delay(waitMs)
                }
            }
    }

    private suspend fun followDesktopStates(session: Session) {
        val welcome = session.welcome
        val fp = stored?.profile?.fp
        session.desktop.collect { desktop ->
            if (desktop == null) return@collect
            val seenAtMs = System.currentTimeMillis()
            mutableState.update {
                val follow = pagerFacts.allowsFollow(ownRequestPending = it.pendingTargetId != null, nowMs = monotonicMs())
                it.withDesktop(desktop, follow, seenAtMs)
            }
            if (fp != null) snapshots.value = Snapshot(fp, desktop, seenAtMs, keepTitles = SCOPE_TITLES in welcome.scopes)
        }
    }

    /** Writes the newest live state to the cache 2 s after it stops changing (docs/UX.md 17.9). */
    private suspend fun saveSnapshots() {
        snapshots.collectLatest { snapshot ->
            if (snapshot == null) return@collectLatest
            delay(SNAPSHOT_DELAY_MS)
            try {
                synchronized(container.desktopCache) {
                    // A forget since the delay ended replaced the snapshot with null.
                    if (snapshots.value === snapshot) {
                        container.desktopCache.save(snapshot.fp, snapshot.desktop, snapshot.seenAtMs, snapshot.keepTitles)
                    }
                }
            } catch (e: IOException) {
                // Only the dimmed preview of the next start is lost; the live state is unaffected.
            }
        }
    }

    /** Where the spinner of an action lives: the sway id space, or [UiState.pendingControl]. */
    private sealed interface Spinner {
        fun isOn(state: UiState): Boolean

        fun on(state: UiState): UiState

        /** Ends only this spinner: a later action may wait by now. */
        fun off(state: UiState): UiState

        class Target(private val id: Long) : Spinner {
            override fun isOn(state: UiState) = state.pendingTargetId == id

            override fun on(state: UiState) = state.copy(pendingTargetId = id)

            override fun off(state: UiState) = if (isOn(state)) state.copy(pendingTargetId = null) else state
        }

        class Control(private val control: PendingControl) : Spinner {
            override fun isOn(state: UiState) = state.pendingControl == control

            override fun on(state: UiState) = state.copy(pendingControl = control)

            override fun off(state: UiState) = if (isOn(state)) state.copy(pendingControl = null) else state
        }
    }

    private fun act(
        targetId: Long,
        onRefused: (UiState, RequestFailedException) -> UiState = ::actionRefusal,
        send: suspend (Session) -> Unit,
    ) = act(Spinner.Target(targetId), onRefused, send)

    /** An action of the M2b areas: sent only when `welcome.ops` lists [op] (docs/PROTOCOL.md 17.1 rule 2). */
    private fun actOn(control: PendingControl, op: String, send: suspend (Session) -> Unit) {
        if (canCall(op)) act(Spinner.Control(control), ::actionRefusal, send)
    }

    private fun canCall(op: String, state: UiState = mutableState.value): Boolean = state.granted?.ops?.contains(op) == true

    /**
     * Sends one action. The target shows a spinner until the answer or the next state arrives,
     * whichever is first. An action is never sent again on its own (docs/PROTOCOL.md section 11),
     * and a tap on a target that still waits sends nothing.
     */
    private fun act(spinner: Spinner, onRefused: (UiState, RequestFailedException) -> UiState, send: suspend (Session) -> Unit) {
        val session = liveSession ?: return
        if (spinner.isOn(mutableState.value)) return
        mutableState.update { spinner.on(it).copy(notice = null) }
        viewModelScope.launch(container.io) {
            // RequestFailedException is an IOException: the server answered with a refusal.
            val failure: IOException? =
                try {
                    send(session)
                    null
                } catch (e: IOException) {
                    e
                }
            mutableState.update {
                // A later action may wait by now: this answer ends only its own spinner and wipes no other notice.
                val answered = spinner.off(it)
                when (failure) {
                    null -> answered
                    is RequestFailedException -> onRefused(answered, failure)
                    else -> answered.copy(notice = Refusal.NO_ANSWER)
                }
            }
        }
    }

    /** docs/UX.md 16: one plain line per code, or the Locked sheet for `forbidden`. Never the raw wire code. */
    private fun actionRefusal(state: UiState, failure: RequestFailedException): UiState {
        val detail = failure.detail
        return when (val refusal = Refusal.of(failure.code, detail.stringAt("scope"), detail.longAt("retry_after_ms"))) {
            is Refusal.Line -> state.copy(notice = refusal.text)
            is Refusal.Locked -> state.copy(sheet = Sheet.Locked(refusal.scope))
        }
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
        const val SNAPSHOT_DELAY_MS = 2_000L
        const val SCOPE_TITLES = "state.titles"
        const val UNSUPPORTED_TARGET = "unsupported_target"
        const val ALREADY_LOCKED = "Already locked."
        const val LAUNCHER_IS_OPEN = "Fuzzel is open on the computer."
        const val TERMINAL_SWITCH_REFUSED = "Ctrl+Alt with a function key would switch the computer's terminal."
        const val VOLUME_INTERVAL_MS = 100L

        /** The `input_text` refill (17.14), when a `rate_limited` answer names no wait. */
        const val TYPING_RETRY_DEFAULT_MS = 1_000L

        /** The longest wait a server hint may ask for: the per-minute character budget. */
        const val TYPING_RETRY_MAX_MS = 60_000L
        val OPTIONAL_TOPICS = listOf(Topic.SESSION, Topic.AUDIO, Topic.MEDIA, Topic.APPS)

        fun monotonicMs(): Long = System.nanoTime() / 1_000_000
    }
}

/** A string member of an untrusted `detail` object; null when absent or of another type. */
private fun JsonObject?.stringAt(name: String): String? = (this?.get(name) as? JsonPrimitive)?.takeIf { it.isString }?.content

/** A whole-number member of an untrusted `detail` object; null when absent, not a number or out of range. */
private fun JsonObject?.longAt(name: String): Long? = (this?.get(name) as? JsonPrimitive)?.takeIf { !it.isString }?.longOrNull

/** Tells the view model when TCP and TLS are up, which is where "Connecting" turns into "Authenticating". */
private class AnnouncingTransport(private val inner: Transport, private val onOpened: () -> Unit) : Transport {
    @Throws(IOException::class)
    override fun open(host: String, port: Int, pinnedFingerprint: ByteArray): Connection =
        inner.open(host, port, pinnedFingerprint).also { onOpened() }
}
