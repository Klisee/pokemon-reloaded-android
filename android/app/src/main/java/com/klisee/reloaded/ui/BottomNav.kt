package com.klisee.reloaded.ui

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.WindowInsetsSides
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.only
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawing
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Description
import androidx.compose.material.icons.filled.Home
import androidx.compose.material.icons.filled.Settings
import androidx.compose.material.icons.outlined.Description
import androidx.compose.material.icons.outlined.Home
import androidx.compose.material.icons.outlined.Settings
import androidx.compose.material3.ExperimentalMaterial3ExpressiveApi
import androidx.compose.material3.FloatingToolbarDefaults
import androidx.compose.material3.HorizontalFloatingToolbar
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ToggleButton
import androidx.compose.material3.ToggleButtonDefaults
import androidx.compose.material3.ToggleButtonShapes
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.unit.dp

/** The three places the app has. */
enum class Tab(val label: String, val icon: ImageVector, val selectedIcon: ImageVector) {
    Home("Inicio", Icons.Outlined.Home, Icons.Filled.Home),
    Log("Logs", Icons.Outlined.Description, Icons.Filled.Description),
    Settings("Ajustes", Icons.Outlined.Settings, Icons.Filled.Settings),
}

/**
 * How much room the floating bar needs at the bottom of a page. The bar hovers over the content
 * rather than sitting in a reserved strip (a strip painted its own background and cut whatever was
 * underneath), so each page leaves this much space for it.
 */
val BottomNavSpace = 104.dp

/**
 * The navigation is Material 3 Expressive's floating toolbar rather than a plain navigation bar: a
 * vibrant capsule that hovers above the bottom edge, where the destination you are on expands into
 * a pill carrying its name and the other two stay as bare icons.
 */
@OptIn(ExperimentalMaterial3ExpressiveApi::class)
@Composable
fun ReloadedBottomNav(current: Tab, onSelect: (Tab) -> Unit, modifier: Modifier = Modifier) {
    // The toolbar hugs its items, so the box is what centres it and what gives it the gap from the
    // bottom edge.
    Box(
        modifier = modifier
            .fillMaxWidth()
            .windowInsetsPadding(WindowInsets.safeDrawing.only(WindowInsetsSides.Bottom))
            .padding(bottom = 16.dp),
        contentAlignment = Alignment.BottomCenter,
    ) {
        HorizontalFloatingToolbar(
            expanded = true,
            colors = FloatingToolbarDefaults.vibrantFloatingToolbarColors(
                toolbarContainerColor = MaterialTheme.colorScheme.primaryContainer,
                toolbarContentColor = MaterialTheme.colorScheme.onPrimaryContainer,
            ),
        ) {
            Tab.entries.forEach { tab ->
                val selected = current == tab
                ToggleButton(
                    checked = selected,
                    onCheckedChange = { if (!selected) onSelect(tab) },
                    modifier = Modifier.height(56.dp),
                    shapes = ToggleButtonShapes(
                        shape = CircleShape,
                        pressedShape = CircleShape,
                        checkedShape = CircleShape,
                    ),
                    colors = ToggleButtonDefaults.colors(
                        checkedContainerColor = MaterialTheme.colorScheme.primary,
                        checkedContentColor = MaterialTheme.colorScheme.onPrimary,
                        containerColor = Color.Transparent,
                        contentColor = MaterialTheme.colorScheme.onPrimaryContainer,
                    ),
                ) {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Icon(if (selected) tab.selectedIcon else tab.icon, contentDescription = null)
                        AnimatedVisibility(visible = selected) {
                            Row {
                                Spacer(Modifier.width(8.dp))
                                Text(tab.label, style = MaterialTheme.typography.titleSmall)
                            }
                        }
                    }
                }
            }
        }
    }
}
