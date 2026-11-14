package com.swayconnect.android.ui.shell

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.WindowInsetsSides
import androidx.compose.foundation.layout.asPaddingValues
import androidx.compose.foundation.layout.calculateEndPadding
import androidx.compose.foundation.layout.calculateStartPadding
import androidx.compose.foundation.layout.consumeWindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.only
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.ime
import androidx.compose.foundation.layout.safeDrawing
import androidx.compose.foundation.layout.systemGestures
import androidx.compose.foundation.layout.union
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.runtime.Composable
import androidx.compose.runtime.IntState
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.runtime.MutableState
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.platform.LocalAccessibilityManager
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalLayoutDirection
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import com.swayconnect.android.R
import com.swayconnect.android.ui.MainViewModel
import com.swayconnect.android.ui.audio.AudioArea
import com.swayconnect.android.ui.common.PillTone
import com.swayconnect.android.ui.common.StatePill
import com.swayconnect.android.ui.common.screenGutter
import com.swayconnect.android.ui.keys.KeysArea
import com.swayconnect.android.ui.keys.KeysPrivateButton
import com.swayconnect.android.ui.launch.LaunchArea
import com.swayconnect.android.ui.launch.LaunchTargetPill
import com.swayconnect.android.ui.launch.shownPageTarget
import com.swayconnect.android.ui.more.MoreArea
import com.swayconnect.android.ui.more.MoreSheet
import com.swayconnect.android.ui.state.Area
import com.swayconnect.android.ui.state.DriveMode
import com.swayconnect.android.ui.state.Sheet
import com.swayconnect.android.ui.state.UiState
import com.swayconnect.android.ui.theme.SwayIcon
import com.swayconnect.android.ui.theme.SwayIcons
import kotlinx.coroutines.delay

private val NAV_HEIGHT = NavGeometry.BAR_HEIGHT.dp
private val NAV_SIDE = 16.dp
private val NAV_LIFT = 12.dp
private val CONTENT_CLEARANCE = 24.dp
private const val TIMED_PILL_BASE_MS = 2000L
private const val VOLUME_KEY_PILL_MS = 1200L

/**
 * The paired app (docs/UX.md 2, 4, 17.2): title row, the content of the selected area and the floating pill nav.
 * [desktop] renders the Desktop area; the padding it gets keeps its last item clear of the nav and the now-playing
 * capsule. [volumeKeyPresses] counts the phone volume keys the activity handled; each one shows "PC 45 %" (UX 7.2).
 * Back closes the shell sheet, then returns to Desktop; on Desktop with no sheet the system handles it
 * (predictive back to home, `enableOnBackInvokedCallback`).
 */
@Composable
fun AppShell(state: UiState, viewModel: MainViewModel, volumeKeyPresses: IntState, desktop: @Composable (PaddingValues) -> Unit) {
    BackHandler(enabled = state.sheet != null || state.area != Area.Desktop) { viewModel.goBack() }
    DrivePausedTimeout(state.drivePaused, viewModel::clearDrivePaused)
    // Launch's target workspace (UX 8.1), shared by its context pill and its grid. Null: the computer's focused
    // workspace. Desktop's "+" aims it at the shown page; leaving Launch resets it.
    val launchTarget = rememberSaveable { mutableStateOf<Int?>(null) }
    LaunchedEffect(state.area) { if (state.area != Area.Launch) launchTarget.value = null }

    val items = navItems(state)
    val showLocked = { scope: String -> viewModel.openSheet(Sheet.Locked(scope)) }
    Surface(color = MaterialTheme.colorScheme.background, modifier = Modifier.fillMaxSize()) {
        BoxWithConstraints(Modifier.fillMaxSize()) {
            val landscape = maxWidth > maxHeight
            // The pill nav hides while the IME is open (UX 9.1); the content then ends just above the keyboard.
            val imeOpen = WindowInsets.ime.getBottom(LocalDensity.current) > 0
            val direction = LocalLayoutDirection.current
            val safe = WindowInsets.safeDrawing.asPaddingValues()
            // One gutter for the title, the cards and the pages (UX 5.5). In landscape the nav stands at the start.
            val gutter = screenGutter(navAtStart = landscape)
            val gutterStart = gutter.calculateStartPadding(direction)
            val gutterEnd = gutter.calculateEndPadding(direction)
            // The capsule hides with the nav while the IME is open (UX 4.3); while shown, the content clears it too.
            val capsule = capsuleOf(state).takeUnless { imeOpen }
            val capsuleRoom = if (capsule != null) CAPSULE_HEIGHT + CAPSULE_GAP else 0.dp
            val capsuleActions = CapsuleActions(
                onCommand = { command -> capsule?.let { viewModel.controlMedia(it.player.id, command) } },
                onOpenAudio = { viewModel.selectArea(Area.Audio) },
                onHide = viewModel::hideNowPlaying,
            )
            val content: @Composable (Modifier, PaddingValues) -> Unit = { modifier, padding ->
                Column(modifier) {
                    ShellTitle(state, viewModel, gutter, showLocked, launchTarget)
                    // The area below draws no inset of its own: the shell already placed it.
                    Box(Modifier.weight(1f).fillMaxWidth().consumeWindowInsets(WindowInsets.safeDrawing)) {
                        AreaContent(state, padding, viewModel, launchTarget.value, desktop)
                        VolumeKeyPill(state, volumeKeyPresses, Modifier.align(Alignment.TopCenter).padding(top = 4.dp))
                    }
                }
            }
            if (landscape) {
                // The vertical nav stands outside the back-gesture zone (UX 4.2).
                val navStart = WindowInsets.safeDrawing.union(WindowInsets.systemGestures).only(WindowInsetsSides.Start)
                    .asPaddingValues().calculateStartPadding(direction)
                content(
                    Modifier.fillMaxSize().padding(
                        start = navStart + NAV_HEIGHT + 8.dp,
                        top = safe.calculateTopPadding(),
                        end = safe.calculateEndPadding(direction),
                    ),
                    PaddingValues(start = gutterStart, end = gutterEnd, bottom = safe.calculateBottomPadding() + 16.dp + capsuleRoom),
                )
                capsule?.let {
                    NowPlayingCapsule(
                        it,
                        capsuleActions,
                        Modifier.align(Alignment.BottomCenter).padding(
                            start = navStart + NAV_HEIGHT + 8.dp + gutterStart,
                            end = safe.calculateEndPadding(direction) + gutterEnd,
                            bottom = safe.calculateBottomPadding() + NAV_LIFT,
                        ),
                    )
                }
                PillNavBar(items, state.area, viewModel::selectArea, vertical = true, Modifier.align(Alignment.CenterStart).padding(start = navStart))
            } else {
                content(
                    Modifier.fillMaxSize().padding(
                        start = safe.calculateStartPadding(direction),
                        top = safe.calculateTopPadding(),
                        end = safe.calculateEndPadding(direction),
                    ),
                    PaddingValues(
                        start = gutterStart,
                        end = gutterEnd,
                        bottom = safe.calculateBottomPadding() + (if (imeOpen) 8.dp else NAV_LIFT + NAV_HEIGHT + CONTENT_CLEARANCE + capsuleRoom),
                    ),
                )
                if (!imeOpen) NavScrim(safe.calculateBottomPadding() + NAV_LIFT + NAV_HEIGHT + capsuleRoom, Modifier.align(Alignment.BottomCenter))
                // Before the nav, so TalkBack reads the capsule first (UX 15).
                capsule?.let {
                    NowPlayingCapsule(
                        it,
                        capsuleActions,
                        Modifier.align(Alignment.BottomCenter).padding(
                            start = safe.calculateStartPadding(direction) + NAV_SIDE,
                            end = safe.calculateEndPadding(direction) + NAV_SIDE,
                            bottom = safe.calculateBottomPadding() + NAV_LIFT + NAV_HEIGHT + CAPSULE_GAP,
                        ),
                    )
                }
                if (!imeOpen) PillNavBar(
                    items,
                    state.area,
                    viewModel::selectArea,
                    vertical = false,
                    Modifier.align(Alignment.BottomCenter).padding(
                        start = safe.calculateStartPadding(direction) + NAV_SIDE,
                        end = safe.calculateEndPadding(direction) + NAV_SIDE,
                        bottom = safe.calculateBottomPadding() + NAV_LIFT,
                    ),
                )
            }
        }
    }
    ShellSheet(
        state,
        ShellSheetActions(
            onClose = viewModel::closeSheet,
            onShowLocked = showLocked,
            onRetry = viewModel::retryNow,
            onForget = viewModel::forget,
        ),
    )
    MoreSheet(state, viewModel)
}

/**
 * The background behind the capsule and nav stack: solid for [solid], then fading out over [CONTENT_CLEARANCE]
 * above it, so scrolled content fades away instead of peeking between the capsule and the nav. Draws only; touches
 * pass through to the content.
 */
@Composable
private fun NavScrim(solid: Dp, modifier: Modifier) {
    val background = MaterialTheme.colorScheme.background
    val total = solid + CONTENT_CLEARANCE
    val fadeEnd = CONTENT_CLEARANCE / total
    Box(
        modifier
            .fillMaxWidth()
            .height(total)
            .background(Brush.verticalGradient(0f to background.copy(alpha = 0f), fadeEnd to background)),
    )
}

@Composable
private fun navItems(state: UiState): List<NavItem> {
    val urgent = anyUrgent(state)
    return Area.entries.map { area ->
        NavItem(area, areaLabel(area), area.icon, locked = lockedScopeOf(state, area) != null, urgent = area == Area.Desktop && urgent)
    }
}

@Composable
private fun areaLabel(area: Area): String =
    stringResource(
        when (area) {
            Area.Desktop -> R.string.area_desktop
            Area.Audio -> R.string.area_audio
            Area.Launch -> R.string.area_launch
            Area.Keys -> R.string.area_keys
            Area.More -> R.string.area_more
        },
    )

private val Area.icon: SwayIcon
    get() = when (this) {
        Area.Desktop -> SwayIcons.Desktop
        Area.Audio -> SwayIcons.Audio
        Area.Launch -> SwayIcons.Launch
        Area.Keys -> SwayIcons.Keyboard
        Area.More -> SwayIcons.More
    }

@Composable
private fun ShellTitle(state: UiState, viewModel: MainViewModel, gutter: PaddingValues, showLocked: (String) -> Unit, launchTarget: MutableState<Int?>) {
    val area = state.area
    // Desktop shows the computer's name (UX 4.1); the other areas their own name.
    val title = if (area == Area.Desktop) state.server?.name ?: areaLabel(area) else areaLabel(area)
    TitleRow(
        title = title,
        status = statusKind(state),
        pills = StatusPills(
            awake = awakePill(state),
            typing = typingPill(state),
            drivePaused = state.drivePaused,
            onAwakeClick = { viewModel.openSheet(Sheet.KeepAwakeDuration) },
        ),
        gutter = gutter,
        onStatusClick = { viewModel.openSheet(Sheet.Computer) },
        contextPill = { compact ->
            if (area == Area.Desktop) {
                DrivePill(
                    drive = state.drive == DriveMode.Drive,
                    availability = state.granted?.availabilityOf(DRIVE_OP),
                    compact = compact,
                    actions = DrivePillActions(
                        onDriveChange = { on -> viewModel.setDriveMode(if (on) DriveMode.Drive else DriveMode.Browse) },
                        onLocked = showLocked,
                        onReadOnly = { viewModel.openSheet(Sheet.Computer) },
                    ),
                )
            }
            if (area == Area.Launch) LaunchTargetPill(state, launchTarget.value) { launchTarget.value = it }
        },
        roundButton = { AreaRoundButton(state, viewModel, showLocked, launchTarget) },
    )
}

@Composable
private fun AreaRoundButton(state: UiState, viewModel: MainViewModel, showLocked: (String) -> Unit, launchTarget: MutableState<Int?>) {
    if (state.area == Area.Keys) {
        // Keys toggles a phone-side mode, Private (UX 9.1): no op of its own.
        KeysPrivateButton(state, viewModel, showLocked)
        return
    }
    val op = roundButtonOp(state.area) ?: return
    val sink = state.audio?.defaultSink
    val (icon, label) = when (state.area) {
        Area.Desktop -> SwayIcons.Plus to R.string.action_open_launch
        Area.Audio -> if (sink?.muted == true) SwayIcons.Volume to R.string.audio_unmute_default else SwayIcons.VolumeOff to R.string.action_mute
        Area.Launch -> SwayIcons.Search to R.string.action_launcher
        Area.Keys, Area.More -> SwayIcons.Lock to R.string.action_lock
    }
    // Audio has no call without a default output: hidden when available.
    val onClick: (() -> Unit)? = when (state.area) {
        Area.Desktop -> ({
            launchTarget.value = shownPageTarget(state)
            viewModel.selectArea(Area.Launch)
        })
        Area.Launch -> viewModel::openComputerLauncher
        Area.Audio -> sink?.takeIf { state.audio?.live == true }?.let { { viewModel.setMuted(it.id, !it.muted) } }
        Area.More -> viewModel::lockComputer
        else -> null
    }
    RoundButton(icon.outline, stringResource(label), state.granted?.availabilityOf(op), onClick, showLocked)
}

@Composable
private fun AreaContent(
    state: UiState,
    padding: PaddingValues,
    viewModel: MainViewModel,
    launchTarget: Int?,
    desktop: @Composable (PaddingValues) -> Unit,
) {
    when (state.area) {
        Area.Desktop -> desktop(padding)
        Area.Audio -> AudioArea(state, viewModel, padding)
        Area.Launch -> LaunchArea(state, viewModel, padding, launchTarget)
        Area.More -> MoreArea(state, viewModel, padding)
        Area.Keys -> KeysArea(state, viewModel, padding)
    }
}

/**
 * "PC 45 %" under the title row for 1.2 s after each phone volume key (UX 7.2). It shows the default output's value as
 * the slider would (the step's draft, then the server's), and only while Audio is shown.
 */
@Composable
private fun VolumeKeyPill(state: UiState, presses: IntState, modifier: Modifier) {
    val count = presses.intValue
    var shown by remember { mutableStateOf(false) }
    LaunchedEffect(count) {
        if (count == 0) return@LaunchedEffect
        shown = true
        delay(VOLUME_KEY_PILL_MS)
        shown = false
    }
    val audio = state.audio
    val sink = audio?.defaultSink
    if (!shown || state.area != Area.Audio || audio == null || sink == null) return
    val pct = audio.shownVolume(sink)
    StatePill(stringResource(if (sink.muted) R.string.volume_key_pill_muted else R.string.volume_key_pill, pct), PillTone.Ink, modifier)
}

/** "Drive paused" lasts the recommended timeout for a text pill, base 2 s (UX 4.1), then clears. */
@Composable
private fun DrivePausedTimeout(paused: Boolean, clear: () -> Unit) {
    val accessibility = LocalAccessibilityManager.current
    LaunchedEffect(paused) {
        if (!paused) return@LaunchedEffect
        val timeout = accessibility?.calculateRecommendedTimeoutMillis(
            TIMED_PILL_BASE_MS,
            containsIcons = false,
            containsText = true,
            containsControls = false,
        ) ?: TIMED_PILL_BASE_MS
        delay(timeout)
        clear()
    }
}
