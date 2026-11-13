package com.swayconnect.android.ui.desktop

import android.content.Context
import android.view.accessibility.AccessibilityManager
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.interaction.DragInteraction
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.calculateEndPadding
import androidx.compose.foundation.layout.calculateStartPadding
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.pager.HorizontalPager
import androidx.compose.foundation.pager.PagerState
import androidx.compose.foundation.pager.rememberPagerState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.MutableState
import androidx.compose.runtime.State
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.input.pointer.PointerEventPass
import androidx.compose.ui.input.pointer.PointerInputScope
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalAccessibilityManager
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalLayoutDirection
import androidx.compose.ui.platform.LocalWindowInfo
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.paneTitle
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.unit.dp
import com.swayconnect.android.R
import com.swayconnect.android.ui.MainViewModel
import com.swayconnect.android.ui.state.Area
import com.swayconnect.android.ui.state.Sheet
import com.swayconnect.android.ui.state.UiState
import com.swayconnect.android.ui.state.isDriving
import com.swayconnect.android.ui.state.shownPageIndex
import com.swayconnect.android.ui.theme.LocalHaptics
import com.swayconnect.android.ui.theme.LocalReduceMotion
import com.swayconnect.android.ui.theme.LocalSwayMotion
import com.swayconnect.android.ui.theme.SwayHaptic
import com.swayconnect.protocol.Desktop
import com.swayconnect.protocol.Workspace
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.collectLatest
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.drop
import kotlinx.coroutines.launch

private const val STALE_ALPHA = 0.6f
private const val CHIP_BASE_MS = 5_000L
private const val LANDSCAPE_MAX_HEIGHT_DP = 480

/**
 * The Desktop area (docs/UX.md 5): the workspace card row, then a pager with one page per workspace. The row
 * and the pager stay in step both ways: the ring follows the pager, the row keeps the shown card centred, and
 * a user drag of the row moves the pager. [contentPadding] comes from the shell and keeps the last item of each
 * page clear of the floating nav.
 */
@Composable
fun DesktopArea(state: UiState, viewModel: MainViewModel, contentPadding: PaddingValues) {
    val desktop = remember(state.desktop) { state.desktop?.forDisplay() }
    val padding = pagePadding(contentPadding)
    // The ids of the last "+N small windows" chip; the sheet lists the whole workspace when they are lost.
    val smallWindowIds = remember { mutableStateOf<List<Long>>(emptyList()) }
    val actions = remember(viewModel) { pageActions(viewModel, smallWindowIds) }
    val facts = pageFacts(state, desktop)
    when {
        desktop == null -> SkeletonDesktop(state, padding, viewModel::retryNow)
        desktop.workspaces.isEmpty() -> NoWorkspaces(state, desktop, facts, padding, viewModel::retryNow)
        else -> DesktopPager(state, desktop, facts, actions, viewModel, padding)
    }
    if (desktop != null) {
        DesktopSheet(state.sheet, desktop, facts, actions, smallWindowIds.value, viewModel::closeSheet)
    }
    ActionHaptics(state)
}

private fun pageActions(viewModel: MainViewModel, smallWindowIds: MutableState<List<Long>>) =
    PageActions(
        onFocusWindow = viewModel::focusWindow,
        onWindowSheet = { viewModel.openSheet(Sheet.WindowActions(it)) },
        onFullscreen = { window -> if (window.fullscreen) viewModel.leaveFullscreen(window.id) else viewModel.enterFullscreen(window.id) },
        onSetView = viewModel::setViewMode,
        onShowOnComputer = viewModel::showOnComputer,
        onFollowChip = viewModel::followMovedChip,
        onDismissListHint = viewModel::dismissListHint,
        onOpenGroup = { workspaceId, ids -> viewModel.openSheet(Sheet.TabGroup(workspaceId, ids)) },
        onSmallWindows = { workspaceId, ids ->
            smallWindowIds.value = ids
            viewModel.openSheet(Sheet.SmallWindows(workspaceId))
        },
        onOpenLaunch = { viewModel.selectArea(Area.Launch) },
        onLocked = { viewModel.openSheet(Sheet.Locked(it)) },
    )

@Composable
private fun pageFacts(state: UiState, desktop: Desktop?): PageFacts {
    val moved = state.followFocusChip?.let { id -> desktop?.workspaces?.firstOrNull { it.id == id } }
    return PageFacts(
        viewMode = state.viewMode,
        browse = !state.isDriving,
        live = state.desktopLive,
        pendingId = state.pendingTargetId,
        granted = state.granted,
        movedTo = moved?.let { cardText(it).number },
        lines = stateLines(state, desktop),
        listHintDismissed = state.listHintDismissed,
    )
}

/** The one-line states of UX 5.9 and the last refusal. Connection lines are in [StaleBanner]. */
@Composable
private fun stateLines(state: UiState, desktop: Desktop?): List<String> =
    listOfNotNull(
        state.notice,
        stringResource(R.string.desktop_read_only).takeIf { state.granted?.readOnly == true },
        stringResource(R.string.desktop_screens_off).takeIf { desktop != null && screensOff(desktop) },
        stringResource(R.string.desktop_truncated).takeIf { desktop?.truncated == true },
    )

/** Every active output has `power == false` (UX 5.9). False without outputs (an older server). */
private fun screensOff(desktop: Desktop): Boolean {
    val active = desktop.outputs.orEmpty().filter { it.active }
    return active.isNotEmpty() && active.none { it.power }
}

/**
 * The shell's gutter on both sides (`max(16dp, system gesture inset)`, UX 5.5, so a swipe rarely starts in the
 * back zone) and its bottom clearance of the nav.
 */
@Composable
private fun pagePadding(shell: PaddingValues): PaddingValues {
    val direction = LocalLayoutDirection.current
    return PaddingValues(
        start = shell.calculateStartPadding(direction),
        end = shell.calculateEndPadding(direction),
        top = 4.dp,
        bottom = shell.calculateBottomPadding(),
    )
}

@Composable
private fun DesktopPager(state: UiState, desktop: Desktop, facts: PageFacts, actions: PageActions, viewModel: MainViewModel, padding: PaddingValues) {
    val pages = desktop.workspaces
    val currentPages = rememberUpdatedState(pages)
    val startIndex = state.shownPageIndex.coerceAtLeast(0)
    val pagerState = rememberPagerState(initialPage = startIndex) { currentPages.value.size }
    val rowState = rememberLazyListState(initialFirstVisibleItemIndex = startIndex)
    val rowGesture = remember { RowGesture() }
    val scope = rememberCoroutineScope()
    val reduceMotion = LocalReduceMotion.current
    val motion = LocalSwayMotion.current
    // Every scroll that stops is reported with the id read now (A3 pager contract rule 2).
    val settle: () -> Unit = { currentPages.value.getOrNull(pagerState.currentPage)?.let { viewModel.onPageSettled(it.id) } }

    PagerEffects(pagerState, currentPages, state.selectedWorkspaceId, viewModel, settle)
    KeepCardInView(pagerState, rowState, rowGesture)
    TouchExploration(viewModel::onTouchExplorationChanged)
    MovedChipTimeout(state.followFocusChip, viewModel::dismissMovedChip)

    val alpha by animateFloatAsState(if (state.desktopLive) 1f else STALE_ALPHA, motion.slowEffectsSpec(), label = "stale")
    val staleSpoken = stringResource(R.string.desktop_stale_spoken)
    val compact = isLandscapePhone()
    Column(
        Modifier
            .fillMaxSize()
            .pointerInput(viewModel) { reportFingers(viewModel::onPagerTouch) },
    ) {
        WorkspaceRow(
            desktop = desktop,
            pagerState = pagerState,
            rowState = rowState,
            rowGesture = rowGesture,
            pendingId = state.pendingTargetId,
            enabled = state.desktopLive,
            compact = compact,
            actions = CardActions(
                onTap = viewModel::onCardTap,
                onLongPress = { viewModel.openSheet(Sheet.WorkspaceActions(it)) },
                onShowOnComputer = viewModel::showOnComputer.takeIf { facts.browse && facts.live },
            ),
            onRowDragStart = viewModel::onUserDrag,
            onRowSettled = { index ->
                scope.launch {
                    pagerState.scrollToPage(index)
                    settle()
                }
            },
            edges = padding,
            onStep = { step ->
                scope.launch {
                    val target = (pagerState.currentPage + step).coerceIn(0, currentPages.value.lastIndex)
                    if (reduceMotion) {
                        pagerState.scrollToPage(target)
                        settle()
                    } else {
                        pagerState.animateScrollToPage(target, animationSpec = motion.defaultSpatialSpec())
                    }
                }
            },
            modifier = Modifier.graphicsLayer { this.alpha = alpha },
        )
        val direction = LocalLayoutDirection.current
        StaleBanner(
            state,
            viewModel::retryNow,
            Modifier.padding(start = padding.calculateStartPadding(direction), end = padding.calculateEndPadding(direction)),
        )
        HorizontalPager(
            state = pagerState,
            key = { currentPages.value[it].id },
            beyondViewportPageCount = 1,
            modifier = Modifier
                .weight(1f)
                .fillMaxWidth()
                .graphicsLayer { this.alpha = alpha }
                .semantics { if (!state.desktopLive) stateDescription = staleSpoken },
        ) { index ->
            val workspace = pages.getOrNull(index) ?: return@HorizontalPager
            val current = index == pagerState.currentPage
            val liveTitle = pageTitle(desktop, workspace)
            // Kept from the moment the page became current: a pane title that changed with every window opened or
            // closed would announce the page again each time (UX 15: once per settle).
            val title = remember(workspace.id, current) { liveTitle }
            // Only the current page is in the semantics tree: TalkBack never walks into a neighbour (UX 15).
            val semantics = if (current) Modifier.semantics { paneTitle = title } else Modifier.clearAndSetSemantics {}
            WorkspacePage(desktop, workspace, facts, actions, padding, semantics)
        }
    }
}

@Composable
private fun pageTitle(desktop: Desktop, workspace: Workspace): String {
    val text = cardText(workspace)
    val count = windowCount(desktop, workspace)
    val countText = if (count == 0) stringResource(R.string.desktop_count_empty) else pluralStringResource(R.plurals.desktop_window_count, count, count)
    return listOfNotNull(stringResource(R.string.desktop_card_spoken, text.number), text.name.takeIf { it.isNotEmpty() }, countText).joinToString(", ")
}

/** Landscape layout of UX 5.3: wider than tall and under 480dp high. */
@Composable
private fun isLandscapePhone(): Boolean {
    val size = LocalWindowInfo.current.containerSize
    val density = LocalDensity.current.density
    return size.width > size.height && size.height / density < LANDSCAPE_MAX_HEIGHT_DP
}

/**
 * The pager side of the A3 contract (docs/UX.md 5.5, 17.3):
 * 1. a finger drag of the pager (`DragInteraction.Start`) is user-initiated; programmatic and accessibility
 *    scrolls make no drag interaction;
 * 2. every stop of a scroll is reported, with the page id read at that moment;
 * 3. the pager follows [selectedId]: without animation after the pages changed (re-sync) or under reduce
 *    motion, else with the spatial spring.
 */
@Composable
private fun PagerEffects(
    pagerState: PagerState,
    pages: State<List<Workspace>>,
    selectedId: Long?,
    viewModel: MainViewModel,
    settle: () -> Unit,
) {
    val haptics = LocalHaptics.current
    val reduceMotion = LocalReduceMotion.current
    val motion = LocalSwayMotion.current
    LaunchedEffect(pagerState) {
        pagerState.interactionSource.interactions.collect { if (it is DragInteraction.Start) viewModel.onUserDrag() }
    }
    LaunchedEffect(pagerState) {
        snapshotFlow { pagerState.isScrollInProgress }.drop(1).collect { scrolling ->
            // A stop that a new scroll already replaced is not a settle.
            if (!scrolling && !pagerState.isScrollInProgress) settle()
        }
    }
    LaunchedEffect(pagerState) {
        snapshotFlow { pagerState.settledPage }.drop(1).collect { haptics.perform(SwayHaptic.SegmentTick) }
    }
    val programmatic = remember { ProgrammaticScroll() }
    var lastIds by remember { mutableStateOf(pages.value.map { it.id }) }
    val ids = pages.value.map { it.id }
    LaunchedEffect(selectedId, ids) {
        val resync = ids != lastIds
        lastIds = ids
        val target = ids.indexOf(selectedId)
        if (target < 0) return@LaunchedEffect
        val there = pagerState.currentPage == target && pagerState.currentPageOffsetFraction == 0f
        // A user swipe or fling is never interrupted: its own settle reports where it ends.
        val userScroll = pagerState.isScrollInProgress && !programmatic.running
        when {
            there && !pagerState.isScrollInProgress -> Unit
            resync -> {
                pagerState.scrollToPage(target)
                settle()
            }
            userScroll -> Unit
            reduceMotion -> {
                pagerState.scrollToPage(target)
                settle()
            }
            else -> programmatic.run { pagerState.animateScrollToPage(target, animationSpec = motion.defaultSpatialSpec()) }
        }
    }
}

/** Keep in view (UX 17.3): the row centres the pager's target card, unless the user drags the row. */
@Composable
private fun KeepCardInView(pagerState: PagerState, rowState: LazyListState, rowGesture: RowGesture) {
    val reduceMotion = LocalReduceMotion.current
    val spec = LocalSwayMotion.current.defaultSpatialSpec<Float>()
    LaunchedEffect(pagerState, rowState) {
        snapshotFlow { pagerState.targetPage to rowState.layoutInfo.viewportSize.width }
            .distinctUntilChanged()
            .collectLatest { (page, width) ->
                if (width > 0 && !rowGesture.active) centreCard(rowState, page, animate = !reduceMotion, spec)
            }
    }
}

/** Finger down and up anywhere on the row or the pager, seen first and never consumed (follow-focus guard). */
private suspend fun PointerInputScope.reportFingers(onTouch: (Boolean) -> Unit) {
    awaitEachGesture {
        awaitFirstDown(requireUnconsumed = false, pass = PointerEventPass.Initial)
        onTouch(true)
        try {
            do {
                val event = awaitPointerEvent(PointerEventPass.Initial)
            } while (event.changes.any { it.pressed })
        } finally {
            onTouch(false)
        }
    }
}

/** TalkBack's touch exploration, from the platform AccessibilityManager (a system service, not a View). */
@Composable
private fun TouchExploration(onChanged: (Boolean) -> Unit) {
    val context = LocalContext.current
    DisposableEffect(context) {
        val manager = context.getSystemService(Context.ACCESSIBILITY_SERVICE) as? AccessibilityManager
        if (manager == null) return@DisposableEffect onDispose {}
        val listener = AccessibilityManager.TouchExplorationStateChangeListener(onChanged)
        onChanged(manager.isTouchExplorationEnabled)
        manager.addTouchExplorationStateChangeListener(listener)
        onDispose { manager.removeTouchExplorationStateChangeListener(listener) }
    }
}

/** "Computer moved to 7 →" lasts the recommended timeout for a control, base 5 s (UX 4.1, 17.3). */
@Composable
private fun MovedChipTimeout(chip: Long?, dismiss: () -> Unit) {
    val accessibility = LocalAccessibilityManager.current
    LaunchedEffect(chip) {
        if (chip == null) return@LaunchedEffect
        val timeout = accessibility?.calculateRecommendedTimeoutMillis(CHIP_BASE_MS, containsIcons = false, containsText = true, containsControls = true)
            ?: CHIP_BASE_MS
        delay(timeout)
        dismiss()
    }
}

/**
 * Haptics of UX 5.5 and 14: `Confirm` when a waiting action ends with the state that confirms it, `Reject`
 * when a refusal line or "Drive paused" appears.
 */
@Composable
private fun ActionHaptics(state: UiState) {
    val haptics = LocalHaptics.current
    var waited by remember { mutableStateOf<Long?>(null) }
    LaunchedEffect(state.pendingTargetId) {
        val ended = waited != null && state.pendingTargetId == null
        if (ended && state.desktopLive && state.notice == null && !state.drivePaused) haptics.perform(SwayHaptic.Confirm)
        waited = state.pendingTargetId
    }
    var shownNotice by remember { mutableStateOf(state.notice) }
    LaunchedEffect(state.notice) {
        if (state.notice != null && state.notice != shownNotice) haptics.perform(SwayHaptic.Reject)
        shownNotice = state.notice
    }
    var paused by remember { mutableStateOf(state.drivePaused) }
    LaunchedEffect(state.drivePaused) {
        if (state.drivePaused && !paused) haptics.perform(SwayHaptic.Reject)
        paused = state.drivePaused
    }
}

/** Marks the pager scrolls that follow the state, so they are not taken for a user swipe. */
private class ProgrammaticScroll {
    var running = false
        private set

    suspend fun run(scroll: suspend () -> Unit) {
        running = true
        try {
            scroll()
        } finally {
            running = false
        }
    }
}
