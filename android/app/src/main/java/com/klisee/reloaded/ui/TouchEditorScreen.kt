package com.klisee.reloaded.ui

import android.graphics.BitmapFactory
import android.graphics.Paint
import androidx.activity.compose.BackHandler
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.gestures.detectDragGestures
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.Saver
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.*
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.IntSize
import androidx.compose.ui.unit.dp
import com.klisee.reloaded.*
import java.io.File
import kotlin.math.hypot
import kotlin.math.roundToInt
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

private val LayoutSaver = Saver<Map<String, TouchPlacement>, String>(
    save = { TouchLayouts.encode(it) }, restore = { TouchLayouts.parse(it) },
)

@Composable
fun TouchEditorScreen(prefs: Preferences, filesDir: File, onExit: () -> Unit) {
    var values by rememberSaveable(stateSaver = LayoutSaver) { mutableStateOf(prefs.touchLayouts()) }
    var savedText by rememberSaveable { mutableStateOf(TouchLayouts.encode(prefs.touchLayouts())) }
    var profileId by rememberSaveable { mutableStateOf(TouchProfile.WORLD.id) }
    val profile = TouchProfile.entries.first { it.id == profileId }
    val controls = remember(profile, values) { TouchLayouts.controlsFor(profile, values) }
    var selectedId by rememberSaveable { mutableStateOf("joystick") }
    val selected = controls.firstOrNull { it.id == selectedId } ?: controls.firstOrNull()
    var profileMenu by remember { mutableStateOf(false) }
    var controlMenu by remember { mutableStateOf(false) }
    var addMenu by remember { mutableStateOf(false) }
    var confirmReset by remember { mutableStateOf(false) }
    var confirmExit by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    var saving by remember { mutableStateOf(false) }
    val scope = rememberCoroutineScope()
    val dirty = TouchLayouts.encode(values) != savedText

    fun update(spec: TouchControlSpec, placement: TouchPlacement) {
        val key = TouchLayouts.key(profile, spec)
        values = if (placement == TouchLayouts.original(spec, profile) && !TouchLayouts.added(profile, spec)) values - key else values + (key to placement)
    }
    fun exit() { if (dirty) confirmExit = true else onExit() }
    fun save() {
        if (saving) return
        saving = true
        val snapshot = values
        scope.launch {
            try {
                withContext(Dispatchers.IO) { prefs.saveTouchLayouts(snapshot, filesDir) }
                savedText = TouchLayouts.encode(snapshot)
                onExit()
            } catch (failure: Exception) {
                error = failure.message ?: "No se pudo guardar. Vuelve a intentarlo."
            } finally { saving = false }
        }
    }
    BackHandler { if (!saving) exit() }

    Surface(Modifier.fillMaxSize(), color = MaterialTheme.colorScheme.background) {
    BoxWithConstraints(Modifier.fillMaxSize()) {
        val gameAspect = (maxWidth.value / maxHeight.value).coerceAtLeast(1f)
        Row(Modifier.fillMaxSize().padding(12.dp), horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            Column(Modifier.weight(1f).fillMaxHeight(), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    TextButton(onClick = { exit() }, enabled = !saving) { Text("Volver") }
                    Box {
                        OutlinedButton(onClick = { profileMenu = true }, enabled = !saving,
                            modifier = Modifier.testTag("profile-picker")) { Text("Perfil: ${profile.title}") }
                        DropdownMenu(expanded = profileMenu, onDismissRequest = { profileMenu = false }) {
                            for (item in TouchProfile.entries) {
                                DropdownMenuItem(text = { Text(item.title) }, onClick = {
                                    profileId = item.id
                                    selectedId = TouchLayouts.controlsFor(item, values).firstOrNull()?.id.orEmpty()
                                    profileMenu = false
                                })
                            }
                        }
                    }
                }
                Text(profile.description, style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant)
                BoxWithConstraints(Modifier.weight(1f).fillMaxWidth(), contentAlignment = Alignment.Center) {
                    val previewWidth = minOf(maxWidth, maxHeight * gameAspect)
                    ControlPreview(
                        profile = profile, values = values, selectedId = selected?.id, enabled = !saving,
                        keepAspect = prefs.keepAspect, opacity = prefs.touchOpacityPercent / 100f, appearance = prefs.touchAppearance,
                        onSelect = { selectedId = it }, onChange = { spec, value -> update(spec, value) },
                        modifier = Modifier.width(previewWidth).aspectRatio(gameAspect)
                            .clip(RoundedCornerShape(16.dp)).border(1.dp, MaterialTheme.colorScheme.outlineVariant, RoundedCornerShape(16.dp)),
                    )
                }
                Text(if (controls.isEmpty()) "Este modo conserva el toque directo."
                    else "Arrastrá un control para moverlo. También podés elegirlo en la lista.",
                    style = MaterialTheme.typography.bodySmall)
                Text("El juego cambia de perfil automáticamente.", style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.primary)
            }

            Surface(Modifier.width(224.dp).fillMaxHeight(), shape = RoundedCornerShape(20.dp),
                color = MaterialTheme.colorScheme.surfaceContainer) {
                Column(Modifier.padding(12.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                    Column(Modifier.weight(1f).verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                        Text("Personalizar controles", style = MaterialTheme.typography.titleMedium)
                        if (profile != TouchProfile.MENU) Box {
                            val candidates = TouchLayouts.availableFor(profile).filter { !it.joystick &&
                                (it !in controls || !TouchLayouts.placement(values, profile, it).visible) }
                            OutlinedButton(onClick = { addMenu = true }, enabled = !saving && candidates.isNotEmpty(),
                                modifier = Modifier.fillMaxWidth().testTag("add-control")) { Text("Agregar botón") }
                            DropdownMenu(expanded = addMenu, onDismissRequest = { addMenu = false }, modifier = Modifier.heightIn(max = 240.dp)) {
                                for (spec in candidates) DropdownMenuItem(text = { Text(spec.title) },
                                    modifier = Modifier.testTag("add-${spec.id}"), onClick = {
                                        val p = if (spec in controls) TouchLayouts.placement(values, profile, spec).copy(visible = true)
                                            else TouchPlacement(.5f, .5f)
                                        values = values + (TouchLayouts.key(profile, spec) to p)
                                        selectedId = spec.id; addMenu = false
                                    })
                            }
                        }
                        if (selected == null) {
                            Text("Sin controles para editar en este perfil.", style = MaterialTheme.typography.bodyMedium)
                        } else {
                            val placement = TouchLayouts.placement(values, profile, selected)
                            Box {
                                OutlinedButton(onClick = { controlMenu = true }, enabled = !saving,
                                    modifier = Modifier.fillMaxWidth().testTag("control-picker")) { Text(selected.title) }
                                DropdownMenu(expanded = controlMenu, onDismissRequest = { controlMenu = false },
                                    modifier = Modifier.heightIn(max = 240.dp)) {
                                    for (spec in controls) {
                                        val shown = TouchLayouts.placement(values, profile, spec).visible
                                        DropdownMenuItem(text = { Text(spec.title + if (shown) "" else " · oculto") }, onClick = {
                                            selectedId = spec.id
                                            controlMenu = false
                                        })
                                    }
                                }
                            }
                            Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically,
                                horizontalArrangement = Arrangement.SpaceBetween) {
                                Text("Visible", style = MaterialTheme.typography.bodyMedium)
                                Switch(checked = placement.visible, onCheckedChange = { update(selected, placement.copy(visible = it)) },
                                    enabled = !saving, modifier = Modifier.testTag("control-visible").semantics {
                                        contentDescription = "Mostrar ${selected.title} en ${profile.title}"
                                    })
                            }
                            Text("Tamaño: ${(placement.scale * 100).roundToInt()} %", style = MaterialTheme.typography.bodyMedium)
                            Slider(value = placement.scale, onValueChange = { update(selected, placement.copy(scale = it)) },
                                valueRange = .5f..2f, steps = 29, enabled = !saving,
                                modifier = Modifier.testTag("control-size").semantics { contentDescription = "Tamaño de ${selected.title}" })
                            Text("Horizontal: ${(placement.x * 100).roundToInt()} %", style = MaterialTheme.typography.labelMedium)
                            Slider(value = placement.x, onValueChange = { update(selected, placement.copy(x = it)) },
                                valueRange = 0f..1f, enabled = !saving,
                                modifier = Modifier.testTag("control-x").semantics { contentDescription = "Posición horizontal de ${selected.title}" })
                            Text("Vertical: ${(placement.y * 100).roundToInt()} %", style = MaterialTheme.typography.labelMedium)
                            Slider(value = placement.y, onValueChange = { update(selected, placement.copy(y = it)) },
                                valueRange = 0f..1f, enabled = !saving,
                                modifier = Modifier.testTag("control-y").semantics { contentDescription = "Posición vertical de ${selected.title}" })
                            TextButton(onClick = { update(selected, TouchLayouts.original(selected, profile)) }, enabled = !saving) {
                                Text("Restablecer este control")
                            }
                            if (TouchLayouts.added(profile, selected)) TextButton(onClick = {
                                values = values - TouchLayouts.key(profile, selected)
                                selectedId = TouchLayouts.controlsFor(profile, values).firstOrNull()?.id.orEmpty()
                            }, enabled = !saving, modifier = Modifier.testTag("remove-added-control")) { Text("Quitar de este perfil") }
                            OutlinedButton(onClick = { confirmReset = true }, enabled = !saving, modifier = Modifier.fillMaxWidth()) {
                                Text("Restablecer perfil")
                            }
                        }
                    }
                    Button(onClick = { save() }, enabled = !saving, modifier = Modifier.fillMaxWidth().testTag("save-exit")) {
                        Text(if (saving) "Guardando…" else "Guardar y salir")
                    }
                }
            }
        }
    }
    }
    if (confirmReset) AlertDialog(onDismissRequest = { confirmReset = false },
        title = { Text("Restablecer ${profile.title}") },
        text = { Text("Se recuperarán las posiciones, tamaños y visibilidad originales de este perfil. Se quitarán los botones agregados a este perfil. Los otros perfiles se conservan. Pulsa Guardar y salir para aplicar los cambios.") },
        confirmButton = { TextButton(onClick = { values = TouchLayouts.resetProfile(values, profile); confirmReset = false }) { Text("Restablecer") } },
        dismissButton = { TextButton(onClick = { confirmReset = false }) { Text("Cancelar") } })
    if (confirmExit) AlertDialog(onDismissRequest = { confirmExit = false },
        title = { Text("¿Salir sin guardar?") }, text = { Text("Hay cambios en los controles que todavía no guardaste.") },
        confirmButton = { TextButton(onClick = { confirmExit = false; onExit() }) { Text("Descartar y salir") } },
        dismissButton = { TextButton(onClick = { confirmExit = false }) { Text("Seguir editando") } })
    error?.let { message -> AlertDialog(onDismissRequest = { error = null }, title = { Text("No se pudo guardar") },
        text = { Text(message) }, confirmButton = { TextButton(onClick = { error = null }) { Text("Aceptar") } }) }
}

@Composable
private fun ControlPreview(
    profile: TouchProfile, values: Map<String, TouchPlacement>, selectedId: String?, enabled: Boolean,
    keepAspect: Boolean, opacity: Float, appearance: TouchAppearance, onSelect: (String) -> Unit,
    onChange: (TouchControlSpec, TouchPlacement) -> Unit, modifier: Modifier = Modifier,
) {
    val images = controlImages(appearance,profile=profile)
    val specs = remember(profile, values) { TouchLayouts.controlsFor(profile, values) }
    val currentSpecs by rememberUpdatedState(specs)
    val currentValues by rememberUpdatedState(values)
    val currentSelection by rememberUpdatedState(selectedId)
    val select by rememberUpdatedState(onSelect)
    val update by rememberUpdatedState(onChange)
    val touchRadius = with(LocalDensity.current) { 20.dp.toPx() }
    val paint = remember { Paint(Paint.ANTI_ALIAS_FLAG).apply { textAlign = Paint.Align.CENTER; typeface = android.graphics.Typeface.create("sans-serif-black", android.graphics.Typeface.BOLD) } }

    fun pick(point: Offset, width: Float, height: Float): TouchControlSpec? = currentSpecs
        .filter { TouchLayouts.placement(currentValues, profile, it).visible || it.id == currentSelection }
        .map { spec ->
            val g = TouchLayouts.geometry(spec, TouchLayouts.placement(currentValues, profile, spec), width, height, keepAspect, TouchLayouts.added(profile, spec))
            val dx = kotlin.math.abs(point.x - g.x)
            val dy = kotlin.math.abs(point.y - g.y)
            val inside = if (spec.asset != null && !spec.joystick)
                dx <= maxOf(touchRadius, g.halfWidth) && dy <= maxOf(touchRadius, g.radius)
            else hypot(dx, dy) <= maxOf(touchRadius, g.radius)
            Triple(spec, hypot(dx, dy), inside)
        }.filter { it.third }.minByOrNull { it.second }?.first

    Canvas(modifier.testTag("control-preview").semantics { contentDescription = "Vista previa de ${profile.title}; arrastra para mover los controles" }
        .pointerInput(profile, keepAspect, enabled) {
            if (!enabled) return@pointerInput
            detectTapGestures { point -> pick(point, size.width.toFloat(), size.height.toFloat())?.let { select(it.id) } }
        }.pointerInput(profile, keepAspect, enabled) {
            if (!enabled) return@pointerInput
            // Keep the in-flight position inside the gesture. Several motion events can arrive
            // before Compose recomposes; reading the last rendered value would lose their deltas.
            var dragging: TouchControlSpec? = null
            var dragPlacement: TouchPlacement? = null
            detectDragGestures(onDragStart = { point ->
                dragging = pick(point, size.width.toFloat(), size.height.toFloat())
                dragging?.let {
                    dragPlacement = TouchLayouts.placement(currentValues, profile, it)
                    select(it.id)
                }
            }, onDragEnd = { dragging = null; dragPlacement = null },
                onDragCancel = { dragging = null; dragPlacement = null }) { change, amount ->
                dragging?.let { spec ->
                    change.consume()
                    val p = dragPlacement ?: TouchLayouts.placement(currentValues, profile, spec)
                    val next = TouchLayouts.moved(spec, p, amount.x, amount.y, size.width.toFloat(), size.height.toFloat(), keepAspect, TouchLayouts.added(profile, spec))
                    dragPlacement = next
                    update(spec, next)
                }
            }
        }) {
        drawRect(Color(0xFF131B25))
        val viewport = TouchLayouts.viewport(size.width, size.height, keepAspect)
        if (keepAspect) {
            drawRect(Color(0xFF090C12))
            drawRect(Color(0xFF131B25), Offset(viewport[0] * size.width, viewport[1] * size.height),
                Size(viewport[2] * size.width, viewport[3] * size.height))
        }
        for (i in 1..9) {
            drawLine(Color.White.copy(alpha = .035f), Offset(size.width * i / 10f, 0f), Offset(size.width * i / 10f, size.height))
        }
        for (i in 1..5) drawLine(Color.White.copy(alpha = .035f), Offset(0f,size.height * i / 6f), Offset(size.width,size.height * i / 6f))
        for (spec in specs) {
            val p = TouchLayouts.placement(values, profile, spec)
            val selected = spec.id == selectedId
            if (!p.visible && !selected) continue
            val g = TouchLayouts.geometry(spec, p, size.width, size.height, keepAspect, TouchLayouts.added(profile, spec))
            val center = Offset(g.x,g.y)
            val alpha = if (!p.visible) .18f else if (spec.joystick || spec.asset == null && images["custom/"+spec.id] == null) 1f else opacity
            fun sprite(name: String, half: Float) {
                images[name]?.let { bitmap ->
                    drawImage(bitmap, dstOffset = IntOffset((g.x-half).roundToInt(),(g.y-half).roundToInt()),
                        dstSize = IntSize((half*2).roundToInt().coerceAtLeast(1),(half*2).roundToInt().coerceAtLeast(1)),
                        alpha = alpha, filterQuality = FilterQuality.None)
                }
            }
            if(spec.joystick) {
                if (appearance.style == TouchStyle.MODERN && images["joystick-base.png"] != null) {
                    sprite("joystick-base.png",g.radius*1.05f);sprite("joystick-knob.png",g.radius*.88f)
                } else drawModernMovement(g, alpha, paint, tint = appearance.tint)
            }
            else drawControl(spec,g,appearance,images,alpha,paint)
            if (selected) drawCircle(Color(0xFF63E0CA),g.radius + 3.dp.toPx(),center,style=Stroke(2.dp.toPx()))
            if (!p.visible) drawLine(Color(0xFF63E0CA),center-Offset(g.radius,g.radius),center+Offset(g.radius,g.radius),2.dp.toPx())
        }
    }
}
