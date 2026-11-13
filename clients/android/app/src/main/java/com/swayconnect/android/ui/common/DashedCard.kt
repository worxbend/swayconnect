package com.swayconnect.android.ui.common

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.graphics.PathEffect
import androidx.compose.ui.graphics.drawOutline
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.unit.dp

/**
 * The dashed placeholder card of docs/UX.md 5.2 and 5.7 ("+ Open an app on 3", "Tap a window to focus it").
 * With [onClick] it is a button; without, a hint.
 */
@Composable
fun DashedCard(text: String, modifier: Modifier = Modifier, icon: ImageVector? = null, onClick: (() -> Unit)? = null) {
    val shape = MaterialTheme.shapes.medium
    val outline = MaterialTheme.colorScheme.outline
    val content = MaterialTheme.colorScheme.onSurfaceVariant
    val clickModifier = if (onClick != null) Modifier.clickable(role = Role.Button, onClick = onClick) else Modifier
    Row(
        modifier = modifier
            .fillMaxWidth()
            .heightIn(min = 56.dp)
            .drawBehind {
                val dash = 6.dp.toPx()
                drawOutline(
                    outline = shape.createOutline(size, layoutDirection, this),
                    color = outline,
                    style = Stroke(width = 1.dp.toPx(), pathEffect = PathEffect.dashPathEffect(floatArrayOf(dash, dash * 2 / 3))),
                )
            }
            .clip(shape)
            .then(clickModifier)
            .padding(horizontal = 16.dp, vertical = 12.dp),
        horizontalArrangement = Arrangement.spacedBy(12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        if (icon != null) Icon(imageVector = icon, contentDescription = null, tint = content, modifier = Modifier.size(20.dp))
        Text(text = text, color = content, style = MaterialTheme.typography.bodyMedium)
    }
}
