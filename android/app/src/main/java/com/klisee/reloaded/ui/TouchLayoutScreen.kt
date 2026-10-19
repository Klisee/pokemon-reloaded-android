package com.klisee.reloaded.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.gestures.detectDragGestures
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.RestartAlt
import androidx.compose.material.icons.filled.Tune
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.klisee.reloaded.Placement
import com.klisee.reloaded.Preferences
import com.klisee.reloaded.TOUCH_CONTROLS
import com.klisee.reloaded.TouchOverlayStyle
import com.klisee.reloaded.defaultPlacement
import kotlin.math.roundToInt

/**
 * The controls editor: a preview of the game window with every control on it, dragged into place by
 * hand, plus a size for each one. What is set here lands in `touch.conf`, which the engine reads at
 * startup.
 *
 * It only paints the new overlay, so it says so when another style is chosen.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun TouchLayoutScreen(prefs: Preferences, onBack: () -> Unit) {
    var layout by remember { mutableStateOf(prefs.touchLayout) }
    var selected by remember { mutableStateOf<String?>(null) }
    val style = remember { prefs.touchStyle }

    fun place(name: String, cx: Float, cy: Float, scale: Float) {
        val clamped = Placement(cx.coerceIn(0.03f, 0.97f), cy.coerceIn(0.03f, 0.97f), scale.coerceIn(0.5f, 2.2f))
        layout = layout + (name to clamped)
        prefs.touchLayout = layout
    }

    fun placementOf(name: String): Placement = layout[name] ?: defaultPlacement(name)

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("Editor de controles") },
                navigationIcon = {
                    IconButton(onClick = onBack) { Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "Volver") }
                },
            )
        },
    ) { padding ->
        Column(
            modifier = Modifier.padding(padding).fillMaxSize().verticalScroll(rememberScrollState()),
            verticalArrangement = Arrangement.spacedBy(16.dp),
        ) {
            Text(
                text = "Arrastra cada boton a su sitio. Toca uno para cambiar su tamano. Solo afecta a los controles Nuevos.",
                style = MaterialTheme.typography.bodyMedium,
                modifier = Modifier.padding(horizontal = 16.dp),
            )
            if (style != TouchOverlayStyle.New) {
                Text(
                    text = "Ahora mismo tienes los controles en estilo ${style.label}: elige Nuevos en Ajustes para verlo.",
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.primary,
                    modifier = Modifier.padding(horizontal = 16.dp),
                )
            }

            Preview(selected, ::placementOf, ::place) { selected = it }
            selected?.let { name ->
                val placement = placementOf(name)
                SettingsGroup("Boton seleccionado: ${TOUCH_CONTROLS.firstOrNull { it.name == name }?.label ?: name}") {
                    SliderRow(
                        icon = Icons.Filled.Tune,
                        title = "Tamano",
                        subtitle = "${(placement.scale * 100).roundToInt()}%",
                        value = placement.scale,
                        items = 2,
                        index = 0,
                        onValueChange = { place(name, placement.cx, placement.cy, it) },
                    )
                    NavRow(
                        icon = Icons.Filled.RestartAlt,
                        title = "Devolverlo a su sitio",
                        subtitle = null,
                        items = 2,
                        index = 1,
                        onClick = {
                            layout = layout - name
                            prefs.touchLayout = layout
                        },
                    )
                }
            }

            SettingsGroup("Botones") {
                TOUCH_CONTROLS.forEachIndexed { index, control ->
                    NavRow(
                        icon = Icons.Filled.Tune,
                        title = control.label,
                        subtitle = control.name,
                        items = TOUCH_CONTROLS.size,
                        index = index,
                        onClick = { selected = control.name },
                    )
                }
            }

            Row(modifier = Modifier.padding(horizontal = 16.dp), verticalAlignment = Alignment.CenterVertically) {
                TextButton(onClick = {
                    layout = emptyMap()
                    prefs.touchLayout = emptyMap()
                    selected = null
                }) {
                    Icon(Icons.Filled.RestartAlt, contentDescription = null)
                    Text("  Restaurar todo")
                }
            }
        }
    }
}

/**
 * The window mock-up: a 16:9 box with one circle per control, dragged with the finger. Positions are
 * kept normalised, so they land the same on any phone.
 */
@Composable
private fun Preview(
    selected: String?,
    placementOf: (String) -> Placement,
    place: (String, Float, Float, Float) -> Unit,
    onSelect: (String) -> Unit,
) {
    val density = LocalDensity.current
    var width by remember { mutableStateOf(0) }
    var height by remember { mutableStateOf(0) }
    Box(
        modifier = Modifier
            .fillMaxWidth()
            .padding(horizontal = 16.dp)
            .aspectRatio(16f / 9f)
            .clip(MaterialTheme.shapes.medium)
            .background(MaterialTheme.colorScheme.surfaceVariant.copy(alpha = 0.4f))
            .border(1.dp, MaterialTheme.colorScheme.outline.copy(alpha = 0.5f), MaterialTheme.shapes.medium)
            .onSizeChanged { size ->
                width = size.width
                height = size.height
            },
    ) {
        val w = width.toFloat()
        val h = height.toFloat()
        if (w <= 0f || h <= 0f) return@Box
        TOUCH_CONTROLS.forEach { control ->
            val placement = placementOf(control.name)
            val diameter = 42.dp * placement.scale
            val diameterPx = with(density) { diameter.toPx() }
            val isSelected = control.name == selected
            Box(
                modifier = Modifier
                    .offset {
                        IntOffset(
                            (placement.cx * w - diameterPx / 2).roundToInt(),
                            (placement.cy * h - diameterPx / 2).roundToInt(),
                        )
                    }
                    .size(diameter)
                    .clip(CircleShape)
                    .background(
                        if (isSelected) MaterialTheme.colorScheme.primary.copy(alpha = 0.65f)
                        else Color.White.copy(alpha = 0.14f)
                    )
                    .border(1.dp, Color.White.copy(alpha = 0.55f), CircleShape)
                    .pointerInput(control.name, w, h) {
                        detectDragGestures(
                            onDragStart = { onSelect(control.name) },
                            onDrag = { change, drag ->
                                change.consume()
                                val current = placementOf(control.name)
                                place(control.name, current.cx + drag.x / w, current.cy + drag.y / h, current.scale)
                            },
                        )
                    }
                    .pointerInput(control.name) {
                        detectTapGestures { onSelect(control.name) }
                    },
                contentAlignment = Alignment.Center,
            ) {
                Text(
                    text = control.label,
                    fontSize = 8.sp,
                    textAlign = TextAlign.Center,
                    maxLines = 2,
                    color = Color.White,
                )
            }
        }
    }
}
