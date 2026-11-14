package com.swayconnect.android.ui.more

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Text
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import com.swayconnect.android.R
import com.swayconnect.android.ui.MainViewModel
import com.swayconnect.android.ui.common.AppIcon
import com.swayconnect.android.ui.desktop.GatedButton
import com.swayconnect.android.ui.desktop.displayTitle
import com.swayconnect.android.ui.state.Sheet
import com.swayconnect.android.ui.state.UiState
import com.swayconnect.android.ui.state.durationChoicesS
import com.swayconnect.android.ui.state.oneHourFromNowS
import com.swayconnect.android.ui.theme.SwayIcons
import com.swayconnect.protocol.KeepAwake
import com.swayconnect.protocol.Op
import com.swayconnect.protocol.OpAvailability
import com.swayconnect.protocol.ScratchpadWindow
import com.swayconnect.protocol.replaceUnsafe

/** Above this font scale the four duration choices stand in a column (UX 15). */
private const val STACKED_FONT_SCALE = 1.5f
private const val DISABLED_ALPHA = 0.38f
private val CHOICE_GAP = 8.dp
private val CHOICE_PADDING = 8.dp

/**
 * Renders [UiState.sheet] when it is one of the More area's kinds: Keep-awake duration (UX 10.2) or
 * Scratchpad (UX 10.3). The shell calls it on every area, since the status pill opens the duration sheet too.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun MoreSheet(state: UiState, viewModel: MainViewModel) {
    val sheet = state.sheet
    if (sheet != Sheet.KeepAwakeDuration && sheet != Sheet.Scratchpad) return
    ModalBottomSheet(onDismissRequest = viewModel::closeSheet, sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true)) {
        Column(
            modifier = Modifier.fillMaxWidth().verticalScroll(rememberScrollState()).padding(start = 24.dp, end = 24.dp, bottom = 24.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            if (sheet == Sheet.KeepAwakeDuration) DurationSheet(state, viewModel) else ScratchpadSheet(state, viewModel)
        }
    }
}

/**
 * The choices of UX 10.2 up to `max_duration_s`; each one replaces the expiry. "1 h from now" while this phone
 * holds it. Nothing to choose before the `session` state, in the cooldown or while the op is not available.
 */
@Composable
private fun DurationSheet(state: UiState, viewModel: MainViewModel) {
    SheetTitle(stringResource(R.string.keep_awake_sheet_title))
    val tile = keepAwakeTile(state)
    val keepAwake = state.session?.state?.keepAwake
    val face = tile?.face
    val choose = { seconds: Long ->
        viewModel.closeSheet()
        viewModel.keepAwakeOn(seconds)
    }
    when {
        tile == null || keepAwake == null || face == KeepAwakeFace.Waiting -> SheetLine(stringResource(R.string.more_waiting))
        tile.access is OpAvailability.Locked -> SheetLine(stringResource(R.string.more_off_for_phone))
        face is KeepAwakeFace.Cooldown -> SheetLine(stringResource(R.string.keep_awake_available_again, minutesUp(face.leftMs)))
        else -> Choices(keepAwake, sendable = tile.access == OpAvailability.Available && tile.live && !tile.pending, face, choose)
    }
    SheetLine(stringResource(R.string.keep_awake_wakes_screens))
    SheetLine(stringResource(R.string.keep_awake_locks_after))
}

@Composable
private fun Choices(keepAwake: KeepAwake, sendable: Boolean, face: KeepAwakeFace?, choose: (Long) -> Unit) {
    val choices = keepAwake.durationChoicesS()
    val labels = choices.map { choiceLabel(it) }
    val buttons: @Composable (Modifier) -> Unit = { modifier ->
        choices.zip(labels).forEach { (seconds, label) -> ChoiceButton(label, sendable, modifier) { choose(seconds) } }
    }
    BoxWithConstraints {
        if (LocalDensity.current.fontScale > STACKED_FONT_SCALE || !labelsFitInRow(labels, maxWidth)) {
            Column(verticalArrangement = Arrangement.spacedBy(CHOICE_GAP)) { buttons(Modifier.fillMaxWidth()) }
        } else {
            Row(horizontalArrangement = Arrangement.spacedBy(CHOICE_GAP)) { buttons(Modifier.weight(1f)) }
        }
    }
    val oneHour = keepAwake.oneHourFromNowS()
    if (oneHour != null && face is KeepAwakeFace.On && face.fromThisPhone) {
        ChoiceButton(stringResource(R.string.keep_awake_one_hour_from_now), sendable, Modifier.fillMaxWidth()) { choose(oneHour) }
    }
}

/** Whether every label fits one of the equal row slots in [width], unit included ("15 min", UX 10.2). */
@Composable
private fun labelsFitInRow(labels: List<String>, width: Dp): Boolean {
    if (labels.isEmpty()) return true
    val measurer = rememberTextMeasurer()
    val style = MaterialTheme.typography.labelLarge
    val widest = with(LocalDensity.current) { labels.maxOf { measurer.measure(it, style).size.width }.toDp() }
    val slot = (width - CHOICE_GAP * (labels.size - 1)) / labels.size - CHOICE_PADDING * 2
    return widest <= slot
}

@Composable
private fun ChoiceButton(label: String, enabled: Boolean, modifier: Modifier, onClick: () -> Unit) {
    FilledTonalButton(
        onClick = onClick,
        enabled = enabled,
        // The tonal fill alone is 1.1:1 on the sheet; the outline keeps the edge at 3:1 (UX 15).
        border = BorderStroke(1.dp, MaterialTheme.colorScheme.outline.copy(alpha = if (enabled) 1f else DISABLED_ALPHA)),
        contentPadding = PaddingValues(horizontal = CHOICE_PADDING),
        modifier = modifier.heightIn(min = 48.dp),
    ) {
        Text(label, maxLines = 1, softWrap = false, overflow = TextOverflow.Ellipsis)
    }
}

@Composable
private fun choiceLabel(seconds: Long): String =
    when (val words = DurationWords.of(seconds)) {
        is DurationWords.Hours -> stringResource(R.string.keep_awake_choice_hours, words.hours.toInt())
        is DurationWords.Minutes -> stringResource(R.string.keep_awake_choice_minutes, words.minutes.toInt())
    }

/** The windows hidden in the scratchpad, each with Show (`scratchpad.show`, UX 10.3). Show closes the sheet. */
@Composable
private fun ScratchpadSheet(state: UiState, viewModel: MainViewModel) {
    SheetTitle(stringResource(R.string.scratchpad))
    val windows = state.desktop?.scratchpad
    val access = state.granted?.availabilityOf(Op.SCRATCHPAD_SHOW)
    when {
        windows == null -> SheetLine(stringResource(R.string.more_waiting))
        windows.isEmpty() -> SheetLine(stringResource(R.string.scratchpad_sheet_empty))
    }
    for (window in windows.orEmpty()) {
        ScratchpadRow(
            window = window,
            access = access,
            enabled = state.desktopLive && state.pendingTargetId != window.id,
            onLocked = { scope -> viewModel.openSheet(Sheet.Locked(scope)) },
        ) {
            viewModel.closeSheet()
            viewModel.showScratchpadWindow(window.id)
        }
    }
}

@Composable
private fun ScratchpadRow(window: ScratchpadWindow, access: OpAvailability?, enabled: Boolean, onLocked: (String) -> Unit, onShow: () -> Unit) {
    // Titles and app ids come from applications (PROTOCOL 9.3): shown with unsafe characters replaced.
    val appId = window.appId?.let(::replaceUnsafe)
    val title = window.title?.let(::displayTitle) ?: appId ?: stringResource(R.string.scratchpad_untitled)
    Row(horizontalArrangement = Arrangement.spacedBy(12.dp), verticalAlignment = Alignment.CenterVertically) {
        Box(Modifier.size(44.dp).background(MaterialTheme.colorScheme.surfaceContainer, CircleShape), contentAlignment = Alignment.Center) {
            AppIcon(appId = appId, iconHash = window.icon, size = 28.dp)
        }
        Column(Modifier.weight(1f)) {
            Text(title, style = MaterialTheme.typography.titleMedium, maxLines = 2, overflow = TextOverflow.Ellipsis)
            if (appId != null && appId != title) {
                Text(appId, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 1, overflow = TextOverflow.Ellipsis)
            }
        }
        GatedButton(
            availability = access,
            icon = SwayIcons.Desktop.outline,
            label = stringResource(R.string.scratchpad_show),
            iconOnly = false,
            enabled = enabled,
            onLocked = onLocked,
            onClick = onShow,
            spoken = stringResource(R.string.scratchpad_show_spoken, title),
        )
    }
}

/**
 * The idle.inhibit sentence of UX 10.5 with the server's limits from topic `session`; the plain sentence
 * ("for a limited time") without them. For the Locked sheet.
 */
@Composable
fun idleInhibitSentence(keepAwake: KeepAwake?): String {
    if (keepAwake == null) return stringResource(R.string.scope_idle_inhibit)
    return stringResource(R.string.scope_idle_inhibit_limits, durationWords(keepAwake.maxDurationS), durationWords(keepAwake.maxContinuousS))
}

@Composable
private fun durationWords(seconds: Long): String =
    when (val words = DurationWords.of(seconds)) {
        is DurationWords.Hours -> pluralStringResource(R.plurals.duration_hours, words.hours.toInt(), words.hours.toInt())
        is DurationWords.Minutes -> pluralStringResource(R.plurals.duration_minutes, words.minutes.toInt(), words.minutes.toInt())
    }

@Composable
private fun SheetTitle(text: String) {
    Text(text, style = MaterialTheme.typography.titleMedium, modifier = Modifier.semantics { heading() })
}

@Composable
private fun SheetLine(text: String) {
    Text(text, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
}
