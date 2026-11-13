package com.swayconnect.android.ui.shell

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.Crossfade
import androidx.compose.animation.EnterTransition
import androidx.compose.animation.ExitTransition
import androidx.compose.animation.animateColorAsState
import androidx.compose.animation.core.animateDpAsState
import androidx.compose.animation.core.snap
import androidx.compose.animation.core.tween
import androidx.compose.animation.expandHorizontally
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.shrinkHorizontally
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.luminance
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import com.swayconnect.android.R
import com.swayconnect.android.ui.state.Area
import com.swayconnect.android.ui.theme.LocalReduceMotion
import com.swayconnect.android.ui.theme.LocalSwayMotion
import com.swayconnect.android.ui.theme.LocalSwayTokens
import com.swayconnect.android.ui.theme.SwayIcon
import com.swayconnect.android.ui.theme.SwayIcons

/** One slot of the pill nav. [locked] keeps the slot and adds a lock badge (UX 4.2, P8). */
data class NavItem(val area: Area, val label: String, val icon: SwayIcon, val locked: Boolean, val urgent: Boolean)

private const val LABEL_FADE_IN_MS = 120
private const val LABEL_FADE_IN_DELAY_MS = 60
private const val LABEL_FADE_OUT_MS = 90
private const val ICON_CROSSFADE_MS = 150

/**
 * The floating pill navigation (docs/UX.md 4.2, reference 1 and 2): a 64dp pill with one inner pill for the
 * selected area. [vertical] stands it up for landscape; it then shows icons only, labels live in semantics.
 */
@Composable
fun PillNavBar(items: List<NavItem>, selected: Area, onSelect: (Area) -> Unit, vertical: Boolean, modifier: Modifier = Modifier) {
    val tokens = LocalSwayTokens.current
    // Light: an 8dp shadow. Dark: no shadow, a 1dp outline stroke instead.
    val dark = tokens.navContainer.luminance() < 0.5f
    Surface(
        modifier = modifier,
        shape = CircleShape,
        color = tokens.navContainer,
        shadowElevation = if (dark) 0.dp else 8.dp,
        border = if (dark) BorderStroke(1.dp, MaterialTheme.colorScheme.outline) else null,
    ) {
        if (vertical) {
            Column(
                modifier = Modifier.width(NavGeometry.BAR_HEIGHT.dp).selectableGroup().padding(vertical = NavGeometry.BAR_PADDING.dp),
                verticalArrangement = Arrangement.spacedBy(NavGeometry.VERTICAL_GAP.dp),
                horizontalAlignment = Alignment.CenterHorizontally,
            ) {
                for (item in items) NavItemView(item, item.area == selected, showLabel = false, onSelect)
            }
        } else {
            HorizontalItems(items, selected, onSelect)
        }
    }
}

@Composable
private fun HorizontalItems(items: List<NavItem>, selected: Area, onSelect: (Area) -> Unit) {
    val measurer = rememberTextMeasurer()
    val labelStyle = MaterialTheme.typography.labelLarge
    val density = LocalDensity.current
    BoxWithConstraints(Modifier.fillMaxWidth().height(NavGeometry.BAR_HEIGHT.dp)) {
        val label = items.firstOrNull { it.area == selected }?.label.orEmpty()
        // Measured at the current font scale, so the label drops before it can push an item out of the bar.
        val labelWidth = with(density) { measurer.measure(label, labelStyle, maxLines = 1).size.width.toDp() }
        val showLabel = NavGeometry.labelFits(maxWidth.value, labelWidth.value, items.size)
        Row(
            modifier = Modifier.fillMaxWidth().height(NavGeometry.BAR_HEIGHT.dp).selectableGroup()
                .padding(horizontal = NavGeometry.BAR_PADDING.dp),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically,
        ) {
            for (item in items) NavItemView(item, item.area == selected, showLabel, onSelect)
        }
    }
}

@Composable
private fun NavItemView(item: NavItem, selected: Boolean, showLabel: Boolean, onSelect: (Area) -> Unit) {
    val tokens = LocalSwayTokens.current
    val motion = LocalSwayMotion.current
    val reduceMotion = LocalReduceMotion.current
    val labelShown = selected && showLabel
    val fill by animateColorAsState(
        if (selected) tokens.navSelected else tokens.navSelected.copy(alpha = 0f),
        motion.defaultEffectsSpec(),
        label = "navFill",
    )
    // 12dp each side centres the 24dp icon in 48dp; with a label the pill is 16 + icon + 8 + label + 20.
    val start by animateDpAsState(if (labelShown) NavGeometry.PILL_START.dp else 12.dp, motion.defaultSpatialSpec(), label = "navStart")
    val end by animateDpAsState(if (labelShown) NavGeometry.PILL_END.dp else 12.dp, motion.defaultSpatialSpec(), label = "navEnd")
    val spoken = when {
        item.locked -> stringResource(R.string.nav_locked, item.label)
        item.urgent -> stringResource(R.string.nav_urgent, item.label)
        else -> item.label
    }
    Row(
        modifier = Modifier
            .height(NavGeometry.ITEM.dp)
            .widthIn(min = NavGeometry.ITEM.dp)
            .clip(CircleShape)
            .background(fill)
            .selectable(selected = selected, role = Role.Tab, onClick = { onSelect(item.area) })
            .semantics { contentDescription = spoken }
            .padding(start = start, end = end),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        NavIcon(item, selected, reduceMotion)
        AnimatedVisibility(
            visible = labelShown,
            enter = if (reduceMotion) {
                EnterTransition.None
            } else {
                fadeIn(tween(LABEL_FADE_IN_MS, delayMillis = LABEL_FADE_IN_DELAY_MS)) +
                    expandHorizontally(motion.defaultSpatialSpec(), expandFrom = Alignment.Start)
            },
            exit = if (reduceMotion) {
                ExitTransition.None
            } else {
                fadeOut(tween(LABEL_FADE_OUT_MS)) + shrinkHorizontally(motion.defaultSpatialSpec(), shrinkTowards = Alignment.Start)
            },
        ) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Spacer(Modifier.width(NavGeometry.ICON_GAP.dp))
                Text(
                    text = item.label,
                    style = MaterialTheme.typography.labelLarge,
                    color = tokens.navOnSelected,
                    maxLines = 1,
                    overflow = TextOverflow.Clip,
                    // The item's own description already says the label.
                    modifier = Modifier.clearAndSetSemantics {},
                )
            }
        }
    }
}

@Composable
private fun NavIcon(item: NavItem, selected: Boolean, reduceMotion: Boolean) {
    val tokens = LocalSwayTokens.current
    Box(Modifier.size(NavGeometry.ICON.dp)) {
        Crossfade(
            targetState = selected,
            animationSpec = if (reduceMotion) snap() else tween(ICON_CROSSFADE_MS),
            label = "navIcon",
        ) { filled ->
            Icon(
                imageVector = if (filled) item.icon.filled else item.icon.outline,
                contentDescription = null,
                tint = if (filled) tokens.navOnSelected else tokens.navIcon,
            )
        }
        when {
            item.locked -> LockBadge(Modifier.align(Alignment.BottomEnd).offset(x = 4.dp, y = 4.dp))
            item.urgent -> UrgentDot(Modifier.align(Alignment.TopEnd).offset(x = 3.dp, y = (-3).dp))
        }
    }
}

/** 12dp lock on the nav colour, so it reads on the icon and on the selected pill. */
@Composable
private fun LockBadge(modifier: Modifier) {
    val tokens = LocalSwayTokens.current
    Box(
        modifier = modifier.size(12.dp).background(tokens.navContainer, CircleShape),
        contentAlignment = Alignment.Center,
    ) {
        Icon(imageVector = SwayIcons.Lock.filled, contentDescription = null, tint = tokens.navIcon, modifier = Modifier.size(10.dp))
    }
}

/** The `error` dot inside a 2dp `surface` keyline: the dark error on the white selected pill alone is 2.06:1. */
@Composable
private fun UrgentDot(modifier: Modifier) {
    val scheme = MaterialTheme.colorScheme
    Box(
        modifier = modifier
            .size(10.dp)
            .border(2.dp, scheme.surface, CircleShape)
            .padding(2.dp)
            .background(scheme.error, CircleShape),
    )
}
