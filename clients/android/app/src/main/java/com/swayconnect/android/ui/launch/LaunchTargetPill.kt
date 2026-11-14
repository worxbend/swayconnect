package com.swayconnect.android.ui.launch

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.unit.dp
import com.swayconnect.android.R
import com.swayconnect.android.ui.state.UiState
import com.swayconnect.android.ui.theme.SwayIcons
import com.swayconnect.protocol.Op
import com.swayconnect.protocol.OpAvailability

/**
 * The context pill of Launch (docs/UX.md 4.1, 8.1): "On 3 ▾", the workspace a tap starts an app on. [number] null
 * is the default, the computer's focused workspace; the pill then shows that workspace's number. Shown only while
 * the phone may start apps and the desktop has a numbered workspace.
 */
@Composable
fun LaunchTargetPill(state: UiState, number: Int?, onNumberChange: (Int?) -> Unit) {
    if (state.granted?.availabilityOf(Op.APP_LAUNCH) != OpAvailability.Available) return
    val targets = launchTargets(state.desktop)
    if (targets.isEmpty()) return
    val shown = shownTarget(number, state.desktop)
    var open by remember { mutableStateOf(false) }
    val label = if (shown != null) stringResource(R.string.launch_on, shown) else stringResource(R.string.launch_on_focused)
    val spoken = stringResource(R.string.launch_target_spoken)
    val targetState =
        if (number != null) stringResource(R.string.launch_target_state_number, number) else stringResource(R.string.launch_target_state_focused)
    Box {
        Surface(
            onClick = { open = true },
            shape = CircleShape,
            color = MaterialTheme.colorScheme.surface,
            border = BorderStroke(1.dp, MaterialTheme.colorScheme.outline),
            modifier = Modifier.semantics {
                role = Role.DropdownList
                contentDescription = spoken
                stateDescription = targetState
            },
        ) {
            Row(
                modifier = Modifier.heightIn(min = 48.dp).padding(horizontal = 16.dp).clearAndSetSemantics {},
                horizontalArrangement = Arrangement.spacedBy(6.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(label, style = MaterialTheme.typography.labelLarge, maxLines = 1)
                Text("▾", style = MaterialTheme.typography.labelLarge)
            }
        }
        DropdownMenu(expanded = open, onDismissRequest = { open = false }) {
            TargetItem(stringResource(R.string.launch_focused_workspace), selected = number == null) {
                open = false
                onNumberChange(null)
            }
            for (target in targets) {
                TargetItem(stringResource(R.string.launch_on, target), selected = number == target) {
                    open = false
                    onNumberChange(target)
                }
            }
        }
    }
}

@Composable
private fun TargetItem(text: String, selected: Boolean, onClick: () -> Unit) {
    DropdownMenuItem(
        text = { Text(text) },
        onClick = onClick,
        leadingIcon = { Box(Modifier.size(20.dp)) { if (selected) Icon(SwayIcons.Check.outline, contentDescription = null, modifier = Modifier.size(20.dp)) } },
        modifier = Modifier.semantics { this.selected = selected },
    )
}
