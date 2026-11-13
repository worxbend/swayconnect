package com.swayconnect.android.ui.shell

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.WindowInsetsSides
import androidx.compose.foundation.layout.asPaddingValues
import androidx.compose.foundation.layout.calculateEndPadding
import androidx.compose.foundation.layout.calculateStartPadding
import androidx.compose.foundation.layout.consumeWindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.only
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawing
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.systemGestures
import androidx.compose.foundation.layout.union
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalAccessibilityManager
import androidx.compose.ui.platform.LocalLayoutDirection
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.max
import com.swayconnect.android.R
import com.swayconnect.android.ui.MainViewModel
import com.swayconnect.android.ui.common.DashedCard
import com.swayconnect.android.ui.state.Area
import com.swayconnect.android.ui.state.DriveMode
import com.swayconnect.android.ui.state.ServerSummary
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

/**
 * The paired app (docs/UX.md 2, 4, 17.2): title row, the content of the selected area and the floating pill nav.
 * [desktop] renders the Desktop area; the padding it gets keeps its last item clear of the nav.
 * Back closes the shell sheet, then returns to Desktop; on Desktop with no sheet the system handles it
 * (predictive back to home, `enableOnBackInvokedCallback`).
 */
@Composable
fun AppShell(state: UiState, viewModel: MainViewModel, desktop: @Composable (PaddingValues) -> Unit) {
    BackHandler(enabled = state.sheet != null || state.area != Area.Desktop) { viewModel.goBack() }
    DrivePausedTimeout(state.drivePaused, viewModel::clearDrivePaused)

    val items = navItems(state)
    val showLocked = { scope: String -> viewModel.openSheet(Sheet.Locked(scope)) }
    Surface(color = MaterialTheme.colorScheme.background, modifier = Modifier.fillMaxSize()) {
        BoxWithConstraints(Modifier.fillMaxSize()) {
            val landscape = maxWidth > maxHeight
            val direction = LocalLayoutDirection.current
            val safe = WindowInsets.safeDrawing.asPaddingValues()
            val gestures = WindowInsets.systemGestures.asPaddingValues()
            // One gutter for the title, the cards and the pages (UX 5.5): 16dp, or the part of the back-gesture
            // zone the safe area leaves inside the content. In landscape the nav already stands past it at the start.
            val gutterStart = if (landscape) NAV_SIDE else max(NAV_SIDE, gestures.calculateStartPadding(direction) - safe.calculateStartPadding(direction))
            val gutterEnd = max(NAV_SIDE, gestures.calculateEndPadding(direction) - safe.calculateEndPadding(direction))
            val gutter = PaddingValues(start = gutterStart, end = gutterEnd)
            val content: @Composable (Modifier, PaddingValues) -> Unit = { modifier, padding ->
                Column(modifier) {
                    ShellTitle(state, viewModel, gutter, showLocked)
                    // The area below draws no inset of its own: the shell already placed it.
                    Box(Modifier.weight(1f).fillMaxWidth().consumeWindowInsets(WindowInsets.safeDrawing)) {
                        AreaContent(state, padding, viewModel, desktop)
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
                    PaddingValues(start = gutterStart, end = gutterEnd, bottom = safe.calculateBottomPadding() + 16.dp),
                )
                PillNavBar(items, state.area, viewModel::selectArea, vertical = true, Modifier.align(Alignment.CenterStart).padding(start = navStart))
            } else {
                content(
                    Modifier.fillMaxSize().padding(
                        start = safe.calculateStartPadding(direction),
                        top = safe.calculateTopPadding(),
                        end = safe.calculateEndPadding(direction),
                    ),
                    PaddingValues(start = gutterStart, end = gutterEnd, bottom = safe.calculateBottomPadding() + NAV_LIFT + NAV_HEIGHT + CONTENT_CLEARANCE),
                )
                PillNavBar(
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
private fun ShellTitle(state: UiState, viewModel: MainViewModel, gutter: PaddingValues, showLocked: (String) -> Unit) {
    val area = state.area
    // Desktop shows the computer's name (UX 4.1); the other areas their own name.
    val title = if (area == Area.Desktop) state.server?.name ?: areaLabel(area) else areaLabel(area)
    TitleRow(
        title = title,
        status = statusKind(state),
        drivePaused = state.drivePaused,
        gutter = gutter,
        onStatusClick = { viewModel.openSheet(Sheet.Computer) },
        contextPill = {
            if (area == Area.Desktop) {
                DrivePill(
                    drive = state.drive == DriveMode.Drive,
                    availability = state.granted?.availabilityOf(DRIVE_OP),
                    onDriveChange = { on -> viewModel.setDriveMode(if (on) DriveMode.Drive else DriveMode.Browse) },
                    onLocked = showLocked,
                    onReadOnly = { viewModel.openSheet(Sheet.Computer) },
                )
            }
        },
        roundButton = { AreaRoundButton(state, viewModel, showLocked) },
    )
}

@Composable
private fun AreaRoundButton(state: UiState, viewModel: MainViewModel, showLocked: (String) -> Unit) {
    val op = roundButtonOp(state.area) ?: return
    val (icon, label) = when (state.area) {
        Area.Desktop -> SwayIcons.Plus to R.string.action_open_launch
        Area.Audio -> SwayIcons.VolumeOff to R.string.action_mute
        Area.Launch -> SwayIcons.Search to R.string.action_launcher
        Area.Keys, Area.More -> SwayIcons.Lock to R.string.action_lock
    }
    // M1 wires only the jump to Launch. The other ops have no client call before M2a/M2b: hidden when available.
    val onClick: (() -> Unit)? = if (state.area == Area.Desktop) ({ viewModel.selectArea(Area.Launch) }) else null
    RoundButton(icon.outline, stringResource(label), state.granted?.availabilityOf(op), onClick, showLocked)
}

@Composable
private fun AreaContent(
    state: UiState,
    padding: PaddingValues,
    viewModel: MainViewModel,
    desktop: @Composable (PaddingValues) -> Unit,
) {
    when (val area = state.area) {
        Area.Desktop -> desktop(padding)
        else -> AreaLater(area, state, padding, viewModel::openSheet)
    }
}

/**
 * The page of an area that M1 does not fill yet (UX 2 rule 4): the nav keeps its shape. More shows the Computer
 * card (UX 10.6); every area says plainly that its controls come later, or how to turn a locked one on.
 */
@Composable
private fun AreaLater(area: Area, state: UiState, padding: PaddingValues, onShowSheet: (Sheet) -> Unit) {
    val lockedScope = lockedScopeOf(state, area)
    Column(
        modifier = Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(padding).padding(top = 8.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        if (area == Area.More) ComputerCard(state.server) { onShowSheet(Sheet.Computer) }
        if (lockedScope != null) {
            DashedCard(text = stringResource(R.string.area_locked_how), icon = SwayIcons.Lock.outline, onClick = { onShowSheet(Sheet.Locked(lockedScope)) })
        } else {
            DashedCard(text = stringResource(if (area == Area.More) R.string.area_more_later else R.string.area_later), icon = area.icon.outline)
        }
    }
}

/** The computer as a card, like a window card: badge, name, short fingerprint. A tap opens the Computer sheet. */
@Composable
private fun ComputerCard(server: ServerSummary?, onClick: () -> Unit) {
    Surface(
        onClick = onClick,
        shape = MaterialTheme.shapes.medium,
        color = MaterialTheme.colorScheme.surface,
        modifier = Modifier.fillMaxWidth(),
    ) {
        Row(Modifier.padding(16.dp), horizontalArrangement = Arrangement.spacedBy(12.dp), verticalAlignment = Alignment.CenterVertically) {
            Box(Modifier.size(44.dp).background(MaterialTheme.colorScheme.surfaceContainer, CircleShape), contentAlignment = Alignment.Center) {
                Icon(SwayIcons.Desktop.outline, contentDescription = null, modifier = Modifier.size(24.dp))
            }
            Column(Modifier.weight(1f)) {
                Text(server?.name ?: stringResource(R.string.area_computer), style = MaterialTheme.typography.titleMedium, maxLines = 1, overflow = TextOverflow.Ellipsis)
                server?.let {
                    Text(
                        stringResource(R.string.computer_fingerprint, it.shortFingerprint),
                        style = MaterialTheme.typography.bodyMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                }
            }
        }
    }
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
