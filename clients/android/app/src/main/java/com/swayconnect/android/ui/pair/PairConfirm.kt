package com.swayconnect.android.ui.pair

import androidx.compose.animation.core.InfiniteRepeatableSpec
import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.State
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.swayconnect.android.R
import com.swayconnect.android.ui.state.Granted
import com.swayconnect.android.ui.theme.LocalReduceMotion
import com.swayconnect.android.ui.theme.SwayIcons
import com.swayconnect.android.ui.theme.tabular

private const val CODE_GROUP = 3
private const val BREATH_MS = 2400
private const val BREATH_MIN = 0.92f

/** Step 3 (UX 11): a spinner until the code arrives, then the six digits inside a slowly breathing ring. */
@Composable
internal fun ConfirmCodeCard(confirmCode: String?, onCancel: () -> Unit) {
    PairCard {
        if (confirmCode == null) {
            Row(horizontalArrangement = Arrangement.spacedBy(12.dp), verticalAlignment = Alignment.CenterVertically) {
                CircularProgressIndicator(modifier = Modifier.size(24.dp), strokeWidth = 3.dp)
                Text(stringResource(R.string.pair_connecting), style = MaterialTheme.typography.bodyMedium)
            }
        } else {
            CardHeading(stringResource(R.string.pair_code_heading))
            CodeRing(confirmCode)
            Text(stringResource(R.string.pair_code_explained), style = MaterialTheme.typography.bodyMedium)
        }
        TextButton(onClick = onCancel, modifier = Modifier.heightIn(min = 48.dp)) {
            Text(stringResource(R.string.pair_cancel))
        }
    }
}

/**
 * 64sp tabular digits split 3 + 3. The two groups wrap to two lines when a large font scale needs it.
 * The ring breathes (scale and alpha); under reduce motion it stands still.
 */
@Composable
private fun CodeRing(code: String) {
    val ringColor = MaterialTheme.colorScheme.outline
    val breath = breathing(enabled = !LocalReduceMotion.current)
    Box(
        modifier =
            Modifier.fillMaxWidth()
                .heightIn(min = 280.dp)
                // Digit by digit: a screen reader would read one number of six digits.
                .clearAndSetSemantics { contentDescription = code.toList().joinToString(" ") }
                .drawBehind {
                    val stroke = 3.dp.toPx()
                    val radius = (minOf(size.width, size.height) / 2 - stroke) * breath.value
                    drawCircle(
                        color = ringColor,
                        radius = radius,
                        alpha = 0.55f + 0.45f * (breath.value - BREATH_MIN) / (1f - BREATH_MIN),
                        style = Stroke(stroke),
                    )
                },
        contentAlignment = Alignment.Center,
    ) {
        FlowRow(horizontalArrangement = Arrangement.spacedBy(16.dp, Alignment.CenterHorizontally)) {
            code.chunked(CODE_GROUP).forEach { group ->
                Text(
                    group,
                    style = MaterialTheme.typography.displayLarge.tabular(),
                    fontSize = 64.sp,
                    fontWeight = FontWeight.Bold,
                )
            }
        }
    }
}

/** 1 at rest; between [BREATH_MIN] and 1 while breathing. Read in the draw phase only, so no recomposition per frame. */
@Composable
private fun breathing(enabled: Boolean): State<Float> {
    if (!enabled) return remember { mutableFloatStateOf(1f) }
    val spec: InfiniteRepeatableSpec<Float> = infiniteRepeatable(tween(BREATH_MS), RepeatMode.Reverse)
    return rememberInfiniteTransition(label = "code ring").animateFloat(BREATH_MIN, 1f, spec, label = "breath")
}

/**
 * Step 4 (UX 11): ink pills for the granted features, outline pills with a lock for the others, from the
 * `welcome` of the first session after `pairing.confirmed`. [granted] is null until that `welcome` arrives.
 */
@Composable
internal fun CapabilitiesCard(serverName: String?, granted: Granted?, problem: String?, onDone: () -> Unit) {
    PairCard {
        CardHeading(stringResource(R.string.pair_done_heading))
        serverName?.let { Text(stringResource(R.string.pair_done_paired, it), style = MaterialTheme.typography.bodyMedium) }
        if (granted == null) {
            if (problem != null) {
                ProblemText(problem)
            } else {
                Row(horizontalArrangement = Arrangement.spacedBy(12.dp), verticalAlignment = Alignment.CenterVertically) {
                    CircularProgressIndicator(modifier = Modifier.size(24.dp), strokeWidth = 3.dp)
                    Text(stringResource(R.string.pair_done_checking), style = MaterialTheme.typography.bodyMedium)
                }
            }
        } else {
            CapabilityPills(PairCapabilities.of(granted))
        }
        InkButton(stringResource(R.string.pair_done_button), onClick = onDone)
    }
}

@Composable
private fun CapabilityPills(capabilities: PairCapabilities) {
    PillGroup(stringResource(R.string.pair_done_granted)) {
        if (capabilities.granted.isEmpty()) {
            Text(stringResource(R.string.pair_done_nothing), style = MaterialTheme.typography.bodyMedium)
        }
        capabilities.granted.forEach { GrantedPill(scopeText(it)) }
    }
    if (capabilities.canRunCode) {
        Text(stringResource(R.string.pair_done_can_run_code), style = MaterialTheme.typography.bodyMedium)
        Text(
            text = stringResource(R.string.pair_done_revoke_command),
            fontFamily = FontFamily.Monospace,
            style = MaterialTheme.typography.bodyMedium,
            modifier = Modifier
                .background(MaterialTheme.colorScheme.surfaceContainer, RoundedCornerShape(12.dp))
                .padding(horizontal = 12.dp, vertical = 6.dp),
        )
    }
    if (capabilities.readOnly) {
        Text(stringResource(R.string.pair_done_read_only), style = MaterialTheme.typography.bodyMedium)
    }
    if (capabilities.locked.isNotEmpty()) {
        PillGroup(stringResource(R.string.pair_done_locked)) {
            capabilities.locked.forEach { LockedPill(scopeText(it)) }
        }
        Text(
            stringResource(R.string.pair_done_ask),
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}

@Composable
private fun scopeText(scope: String): String = scopeLabel(scope)?.let { stringResource(it) } ?: scope

@Composable
private fun PillGroup(title: String, pills: @Composable () -> Unit) {
    Text(title, style = MaterialTheme.typography.labelLarge, color = MaterialTheme.colorScheme.onSurfaceVariant)
    FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
        pills()
    }
}

@Composable
private fun GrantedPill(text: String) {
    val colors = MaterialTheme.colorScheme
    Pill(text, SwayIcons.Check.outline, iconDescription = null, color = colors.primary, content = colors.onPrimary, border = null)
}

@Composable
private fun LockedPill(text: String) {
    val colors = MaterialTheme.colorScheme
    Pill(
        text,
        SwayIcons.Lock.outline,
        // The lock is the word "off" for TalkBack, so the colour carries no meaning alone (UX 15).
        iconDescription = stringResource(R.string.pair_done_locked_mark),
        color = colors.surface,
        content = colors.onSurface,
        border = BorderStroke(1.dp, colors.outline),
    )
}

@Composable
private fun Pill(
    text: String,
    icon: ImageVector,
    iconDescription: String?,
    color: Color,
    content: Color,
    border: BorderStroke?,
) {
    Surface(shape = CircleShape, color = color, contentColor = content, border = border) {
        Row(
            modifier = Modifier.heightIn(min = 36.dp).padding(start = 10.dp, end = 14.dp, top = 6.dp, bottom = 6.dp),
            horizontalArrangement = Arrangement.spacedBy(6.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Icon(icon, contentDescription = iconDescription, modifier = Modifier.size(18.dp))
            Text(text, style = MaterialTheme.typography.labelLarge)
        }
    }
}
