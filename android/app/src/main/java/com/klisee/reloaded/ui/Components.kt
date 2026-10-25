package com.klisee.reloaded.ui

import androidx.compose.animation.core.animateDpAsState
import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.interaction.collectIsPressedAsState
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ExperimentalMaterial3ExpressiveApi
import androidx.compose.material3.Icon
import androidx.compose.material3.ListItem
import androidx.compose.material3.ListItemDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.RadioButton
import androidx.compose.material3.Slider
import androidx.compose.material3.Switch
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.unit.dp
import java.text.DateFormat
import java.util.Date

/** Human-readable size, e.g. "324 MB". */
fun formatBytes(bytes: Long): String = when {
    bytes <= 0 -> "sin datos"
    bytes >= 1L shl 30 -> "%.1f GB".format(bytes / (1L shl 30).toDouble())
    bytes >= 1L shl 20 -> "%.0f MB".format(bytes / (1L shl 20).toDouble())
    else -> "%.0f KB".format(bytes / 1024.0)
}

fun formatDate(millis: Long): String =
    if (millis <= 0) "sin datos" else DateFormat.getDateInstance(DateFormat.MEDIUM).format(Date(millis))

/**
 * Opens a link in whatever browser the device has. Downloading is handed to the browser on purpose:
 * the mirrors compute their real file URL with their own scripts, which cannot be reproduced here,
 * so the launcher only does what it can do reliably, which is point at the page.
 */
fun openInBrowser(context: android.content.Context, url: String) {
    runCatching {
        context.startActivity(android.content.Intent(android.content.Intent.ACTION_VIEW, android.net.Uri.parse(url)))
    }
}

/**
 * The row everything else is built from.
 *
 * Corners carry the group: the ends of a block get 20dp, the rows between them stay at 4dp, and the
 * row you are touching expands to 40dp with the theme's fast spatial spring. That last part is what
 * makes a settings list feel alive instead of inert.
 */
@OptIn(ExperimentalMaterial3Api::class, ExperimentalMaterial3ExpressiveApi::class)
@Composable
fun GroupListItem(
    headlineContent: @Composable () -> Unit,
    modifier: Modifier = Modifier,
    supportingContent: (@Composable () -> Unit)? = null,
    leadingContent: (@Composable () -> Unit)? = null,
    trailingContent: (@Composable () -> Unit)? = null,
    items: Int,
    index: Int,
    onClick: (() -> Unit)? = null,
) {
    val interactionSource = remember { MutableInteractionSource() }
    val isPressed by interactionSource.collectIsPressedAsState()
    val spec = MaterialTheme.motionScheme.fastSpatialSpec<androidx.compose.ui.unit.Dp>()

    val atEdge = items <= 1
    val top by animateDpAsState(
        targetValue = if (isPressed) 40.dp else if (atEdge || index == 0) 20.dp else 4.dp,
        animationSpec = spec,
        label = "top",
    )
    val bottom by animateDpAsState(
        targetValue = if (isPressed) 40.dp else if (atEdge || index == items - 1) 20.dp else 4.dp,
        animationSpec = spec,
        label = "bottom",
    )
    val shape = RoundedCornerShape(
        topStart = top.coerceAtLeast(0.dp),
        topEnd = top.coerceAtLeast(0.dp),
        bottomEnd = bottom.coerceAtLeast(0.dp),
        bottomStart = bottom.coerceAtLeast(0.dp),
    )

    ListItem(
        headlineContent = headlineContent,
        supportingContent = supportingContent,
        leadingContent = leadingContent,
        trailingContent = trailingContent,
        colors = ListItemDefaults.colors(containerColor = MaterialTheme.colorScheme.surfaceContainerHigh),
        modifier = modifier
            .fillMaxWidth()
            .clip(shape)
            .then(
                if (onClick != null) {
                    Modifier.clickable(onClick = onClick, interactionSource = interactionSource)
                } else {
                    Modifier
                },
            ),
    )
}

/** A labelled block of rows. */
@Composable
fun SettingsGroup(
    title: String,
    modifier: Modifier = Modifier,
    content: @Composable ColumnScope.() -> Unit,
) {
    Column(modifier.fillMaxWidth()) {
        Text(
            text = title,
            style = MaterialTheme.typography.titleMedium,
            color = MaterialTheme.colorScheme.primary,
            modifier = Modifier.padding(start = 16.dp, bottom = 8.dp),
        )
        Column(verticalArrangement = Arrangement.spacedBy(2.dp), content = content)
    }
}

@Composable
fun NavRow(
    icon: ImageVector,
    title: String,
    items: Int,
    index: Int,
    modifier: Modifier = Modifier,
    subtitle: String? = null,
    enabled: Boolean = true,
    onClick: () -> Unit,
) {
    GroupListItem(
        headlineContent = { Text(title, style = MaterialTheme.typography.titleMedium) },
        supportingContent = subtitle?.let { { Text(it, style = MaterialTheme.typography.bodyMedium) } },
        leadingContent = { Icon(icon, contentDescription = null) },
        items = items,
        index = index,
        modifier = modifier,
        onClick = if (enabled) onClick else null,
    )
}

@Composable
fun InfoRow(
    icon: ImageVector,
    title: String,
    value: String,
    items: Int,
    index: Int,
    modifier: Modifier = Modifier,
) {
    GroupListItem(
        headlineContent = { Text(title, style = MaterialTheme.typography.titleMedium) },
        supportingContent = { Text(value, style = MaterialTheme.typography.bodyMedium) },
        leadingContent = { Icon(icon, contentDescription = null) },
        items = items,
        index = index,
        modifier = modifier,
    )
}

/** A row that picks one option out of a group, the way a radio button does. */
@Composable
fun ChoiceRow(
    icon: ImageVector,
    title: String,
    subtitle: String,
    selected: Boolean,
    items: Int,
    index: Int,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
    onSelect: () -> Unit,
) {
    GroupListItem(
        headlineContent = { Text(title, style = MaterialTheme.typography.titleMedium) },
        supportingContent = { Text(subtitle, style = MaterialTheme.typography.bodyMedium) },
        leadingContent = { Icon(icon, contentDescription = null) },
        trailingContent = { RadioButton(selected = selected, onClick = null, enabled = enabled) },
        items = items,
        index = index,
        modifier = modifier.alpha(if (enabled) 1f else 0.45f),
        onClick = if (enabled) onSelect else null,
    )
}

@Composable
fun SwitchRow(
    icon: ImageVector,
    title: String,
    subtitle: String,
    checked: Boolean,
    items: Int,
    index: Int,
    modifier: Modifier = Modifier,
    onCheckedChange: (Boolean) -> Unit,
) {
    GroupListItem(
        headlineContent = { Text(title, style = MaterialTheme.typography.titleMedium) },
        supportingContent = { Text(subtitle, style = MaterialTheme.typography.bodyMedium) },
        leadingContent = { Icon(icon, contentDescription = null) },
        trailingContent = { Switch(checked = checked, onCheckedChange = onCheckedChange) },
        items = items,
        index = index,
        modifier = modifier,
        onClick = { onCheckedChange(!checked) },
    )
}

/** A row whose value is picked off a slider. */
@Composable
fun SliderRow(
    icon: ImageVector,
    title: String,
    subtitle: String,
    value: Float,
    items: Int,
    index: Int,
    modifier: Modifier = Modifier,
    onValueChange: (Float) -> Unit,
) {
    GroupListItem(
        headlineContent = { Text(title, style = MaterialTheme.typography.titleMedium) },
        supportingContent = {
            Column {
                Text(subtitle, style = MaterialTheme.typography.bodyMedium)
                Slider(value = value, onValueChange = onValueChange, valueRange = 0.15f..1f)
            }
        },
        leadingContent = { Icon(icon, contentDescription = null) },
        items = items,
        index = index,
        modifier = modifier,
    )
}
