package com.swayconnect.android.ui.common

import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.foundation.background
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.interaction.collectIsPressedAsState
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.ripple
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import com.swayconnect.android.R
import com.swayconnect.android.ui.theme.LocalHaptics
import com.swayconnect.android.ui.theme.LocalSwayMotion
import com.swayconnect.android.ui.theme.SwayHaptic
import com.swayconnect.android.ui.theme.tabular
import com.swayconnect.protocol.Window

private const val PRESSED_SCALE = 0.96f

/**
 * A window card (docs/UX.md 5.2): icon badge, title, app id line, state pills, and on the right the same-app
 * index or a spinner while an action on the window waits. Taps are ignored while [pending].
 *
 * @param group the window's place in a tabbed or stacked container, for its pill.
 * @param sameAppIndex the tree-order index when two windows on the page share an `app_id`.
 * @param actions buttons under the pills (the focused-window card).
 */
@Composable
fun WindowCard(
    window: Window,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    onClickLabel: String? = null,
    onLongClick: (() -> Unit)? = null,
    enabled: Boolean = true,
    pending: Boolean = false,
    group: GroupPosition? = null,
    sameAppIndex: Int? = null,
    actions: (@Composable () -> Unit)? = null,
) {
    val haptics = LocalHaptics.current
    val interaction = remember { MutableInteractionSource() }
    val pressed by interaction.collectIsPressedAsState()
    val scale by animateFloatAsState(if (pressed) PRESSED_SCALE else 1f, LocalSwayMotion.current.fastSpatialSpec(), label = "press")
    val shape = MaterialTheme.shapes.medium
    val waiting = stringResource(R.string.window_waiting)
    Surface(
        modifier = modifier
            .fillMaxWidth()
            .graphicsLayer {
                scaleX = scale
                scaleY = scale
            }
            .clip(shape)
            .combinedClickable(
                interactionSource = interaction,
                indication = ripple(),
                enabled = enabled && !pending,
                onClickLabel = onClickLabel,
                role = Role.Button,
                onLongClickLabel = onLongClick?.let { stringResource(R.string.window_more_actions) },
                onLongClick = onLongClick?.let { open -> { haptics.perform(SwayHaptic.LongPress); open() } },
                // The app's own LongPress above follows the haptics setting; the built-in one would not.
                hapticFeedbackEnabled = false,
                onClick = onClick,
            )
            .semantics { if (pending) stateDescription = waiting },
        shape = shape,
        color = MaterialTheme.colorScheme.surface,
    ) {
        Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            WindowHeadline(window, pending, sameAppIndex)
            WindowPills(windowMarks(window, group))
            actions?.invoke()
        }
    }
}

@Composable
private fun WindowHeadline(window: Window, pending: Boolean, sameAppIndex: Int?) {
    Row(horizontalArrangement = Arrangement.spacedBy(12.dp), verticalAlignment = Alignment.CenterVertically) {
        Box(
            Modifier.size(44.dp).background(MaterialTheme.colorScheme.surfaceContainer, CircleShape),
            contentAlignment = Alignment.Center,
        ) {
            AppIcon(appId = window.appId, iconHash = window.icon, size = 28.dp)
        }
        Column(Modifier.weight(1f)) {
            // Without scope state.titles the title is absent: the app id takes its place.
            val title = window.title ?: window.appId ?: stringResource(R.string.window_untitled)
            Text(text = title, style = MaterialTheme.typography.titleMedium, maxLines = 2, overflow = TextOverflow.Ellipsis)
            if (window.title != null && window.appId != null) {
                Text(
                    text = window.appId.orEmpty(),
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            }
        }
        if (pending) {
            CircularProgressIndicator(modifier = Modifier.size(16.dp), strokeWidth = 2.dp)
        } else if (sameAppIndex != null) {
            val spoken = stringResource(R.string.window_same_app_index_spoken, sameAppIndex)
            Text(
                text = stringResource(R.string.window_same_app_index, sameAppIndex),
                style = MaterialTheme.typography.labelLarge.tabular(),
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.semantics { contentDescription = spoken },
            )
        }
    }
}

@Composable
private fun WindowPills(marks: List<WindowMark>) {
    if (marks.isEmpty()) return
    FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
        for (mark in marks) {
            when (mark) {
                WindowMark.Focused -> StatePill(
                    text = stringResource(R.string.window_mark_focused),
                    tone = PillTone.Ink,
                    spokenText = stringResource(R.string.window_mark_focused_spoken),
                )
                WindowMark.Urgent -> StatePill(
                    text = stringResource(R.string.window_mark_urgent),
                    tone = PillTone.Coral,
                    spokenText = stringResource(R.string.window_mark_urgent_spoken),
                )
                WindowMark.Fullscreen -> StatePill(stringResource(R.string.window_mark_fullscreen), PillTone.Tinted)
                WindowMark.Floating -> StatePill(stringResource(R.string.window_mark_floating), PillTone.Tinted)
                is WindowMark.InGroup -> StatePill(groupLabel(mark.group), PillTone.Tinted)
            }
        }
    }
}

@Composable
private fun groupLabel(group: GroupPosition): String {
    val res = when (group.kind) {
        GroupKind.Tabbed -> R.string.window_mark_tabbed
        GroupKind.Stacked -> R.string.window_mark_stacked
    }
    return stringResource(res, group.position, group.count)
}
