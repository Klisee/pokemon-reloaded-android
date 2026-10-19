package com.klisee.reloaded.ui

import android.graphics.BitmapFactory
import android.graphics.Paint
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.tween
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.gestures.waitForUpOrCancellation
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.*
import androidx.compose.ui.graphics.drawscope.DrawScope
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.IntSize
import androidx.compose.ui.unit.dp
import com.klisee.reloaded.*
import kotlin.math.roundToInt

@OptIn(ExperimentalMaterial3Api::class, ExperimentalLayoutApi::class)
@Composable
fun TouchAppearanceScreen(prefs: Preferences, onBack: () -> Unit) {
    val context = LocalContext.current
    val feedback = remember { TouchFeedback(context) }
    var appearance by remember(touchResumeRevision()) { mutableStateOf(prefs.touchAppearance) }
    var customHex by rememberSaveable { mutableStateOf(appearance.colorHex.takeUnless { it=="original" }.orEmpty()) }
    fun update(next: TouchAppearance) { appearance = next; prefs.touchAppearance = next }
    Scaffold(topBar = { TopAppBar(title = { Text("Apariencia de controles") }, navigationIcon = {
        TextButton(onClick = onBack) { Text("Volver") }
    }) }) { insets ->
        Column(Modifier.fillMaxSize().padding(insets).verticalScroll(rememberScrollState()).padding(20.dp),
            verticalArrangement = Arrangement.spacedBy(16.dp)) {
            Text("Se guarda automáticamente y se aplica al iniciar el juego. La opacidad se mantiene en Ajustes.",
                style=MaterialTheme.typography.bodyMedium, color=MaterialTheme.colorScheme.onSurfaceVariant)
            AppearanceSample(appearance, prefs.touchOpacityPercent / 100f, feedback)
            Text("Tocá un botón para probar su respuesta.", style=MaterialTheme.typography.bodySmall)
            Text("Estilo gráfico",style=MaterialTheme.typography.titleMedium)
            FlowRow(horizontalArrangement=Arrangement.spacedBy(8.dp)) {
                for (style in TouchStyle.entries) FilterChip(selected=appearance.style==style,
                    onClick={update(appearance.copy(style=style))}, label={Text(style.title)},
                    modifier=Modifier.testTag("style-${style.id}"))
            }
            Text("Moderno usa los sprites semitransparentes; Clasico, los botones suaves con anillos.",style=MaterialTheme.typography.bodySmall)
            OutlinedButton(onClick = { context.startActivity(android.content.Intent(context, TouchAssetsActivity::class.java)) }, modifier = Modifier.fillMaxWidth()) {
                Text("Cambiar sprites · Importar y exportar")
            }
            Text("Color de los botones",style=MaterialTheme.typography.titleMedium)
            FlowRow(horizontalArrangement=Arrangement.spacedBy(8.dp),verticalArrangement=Arrangement.spacedBy(4.dp)) {
                FilterChip(selected=appearance.tint==null,onClick={update(appearance.copy(colorHex="original"));customHex=""},label={Text("Original")},modifier=Modifier.testTag("color-original"))
                for ((hex,label) in listOf("63E0CA" to "Menta","59A8FF" to "Azul","FF786E" to "Rojo","FFD166" to "Dorado","C69CFF" to "Violeta","FFFFFF" to "Blanco")) {
                    FilterChip(selected=appearance.colorHex==hex,onClick={customHex=hex;update(appearance.copy(colorHex=hex))},
                        label={Text(label)},leadingIcon={Box(Modifier.size(16.dp).background(Color(0xFF000000 or hex.toLong(16)),CircleShape))},
                        modifier=Modifier.testTag("color-$hex"))
                }
            }
            OutlinedTextField(value=customHex,onValueChange={raw ->
                customHex=raw.filter { it in '0'..'9' || it.lowercaseChar() in 'a'..'f' }.take(6).uppercase(java.util.Locale.ROOT)
                if(customHex.length==6)update(appearance.copy(colorHex=customHex))
            },label={Text("Color personalizado · HEX")},prefix={Text("#")},singleLine=true,
                supportingText={Text("Seis caracteres, por ejemplo 63E0CA")},modifier=Modifier.fillMaxWidth().testTag("custom-color"))
            PreferenceToggle("Animaciones al pulsar","Reducción y cambio de brillo rápidos al tocar.",appearance.animations,"animations") { update(appearance.copy(animations=it)) }
            PreferenceToggle("Vibración táctil","Un pulso breve al presionar. Respeta la respuesta táctil de Android.",appearance.haptics,"haptics",feedback.available) { update(appearance.copy(haptics=it)) }
            if (!feedback.available) Text("Este dispositivo no tiene vibrador disponible.",style=MaterialTheme.typography.bodySmall)
            Text("Intensidad",style=MaterialTheme.typography.titleMedium)
            FlowRow(horizontalArrangement=Arrangement.spacedBy(8.dp)) {
                for (strength in TouchStrength.entries) FilterChip(selected=appearance.strength==strength,
                    enabled=appearance.haptics && feedback.supportsIntensity,
                    onClick={update(appearance.copy(strength=strength))},label={Text(strength.title)},
                    modifier=Modifier.testTag("intensity-${strength.name}"))
            }
            if(!feedback.supportsIntensity) Text("Este dispositivo usa la intensidad de respuesta táctil del sistema.",style=MaterialTheme.typography.bodySmall)
            TextButton(onClick={customHex="";update(TouchAppearance())},modifier=Modifier.testTag("appearance-reset")) { Text("Restablecer apariencia y respuesta") }
        }
    }
}

@Composable
private fun PreferenceToggle(title:String,description:String,checked:Boolean,tag:String,enabled:Boolean=true,onChange:(Boolean)->Unit) {
    Row(Modifier.fillMaxWidth(),verticalAlignment=Alignment.CenterVertically,horizontalArrangement=Arrangement.spacedBy(16.dp)) {
        Column(Modifier.weight(1f)) { Text(title,style=MaterialTheme.typography.titleMedium);Text(description,style=MaterialTheme.typography.bodySmall) }
        Switch(checked=checked,onCheckedChange=onChange,enabled=enabled,modifier=Modifier.testTag(tag).semantics { contentDescription=title })
    }
}

@Composable
internal fun touchResumeRevision(): Int {
    val activity = LocalContext.current as? androidx.activity.ComponentActivity
    var revision by remember { mutableStateOf(0) }
    DisposableEffect(activity) {
        val observer = androidx.lifecycle.LifecycleEventObserver { _, event ->
            if (event == androidx.lifecycle.Lifecycle.Event.ON_RESUME) revision++
        }
        activity?.lifecycle?.addObserver(observer)
        onDispose { activity?.lifecycle?.removeObserver(observer) }
    }
    return revision
}

@Composable
internal fun controlImages(appearance: TouchAppearance, revision: Int = 0, profile: TouchProfile = TouchProfile.WORLD): Map<String,ImageBitmap> {
    val context=LocalContext.current
    val resumed=touchResumeRevision()
    return remember(appearance.colorHex,revision,resumed,profile) {
        val images=mutableMapOf<String,ImageBitmap>()
        for (target in TouchSprites.targets) {
            val name = target.asset ?: continue
            val asset = TouchSprites.bundledAsset(target.id, profile) ?: continue
            runCatching {context.assets.open(asset).use {stream -> BitmapFactory.decodeStream(stream)?.let {bitmap ->
                images[name]=TouchAppearance.image(bitmap, appearance.tint)
            }}}
        }
        val custom=Preferences(context).touchSprites
        for (target in TouchSprites.targets) runCatching {TouchSprites.read(context,TouchSprites.effectiveKey(target.id,profile,custom),custom)?.let {bytes ->
            BitmapFactory.decodeByteArray(bytes,0,bytes.size)?.let {bitmap ->
                val image=TouchAppearance.image(bitmap, appearance.tint)
                images["custom/"+target.id]=image
                if(target.id.startsWith("joystick"))images[target.asset!!]=image
            }
        }}
        images
    }
}

@Composable
private fun AppearanceSample(appearance:TouchAppearance,opacity:Float,feedback:TouchFeedback) {
    val view=LocalView.current
    val images=controlImages(appearance)
    val specs=remember { listOf("z","x","alpha1","b").map { id->TouchLayouts.controls.first { it.id==id } } }
    var pressed by remember { mutableStateOf<Int?>(null) }
    val current by rememberUpdatedState(appearance)
    val progress by animateFloatAsState(if(pressed!=null && appearance.animations)1f else 0f,tween(if(pressed!=null)55 else 80),label="button-press")
    var releasing by remember { mutableStateOf(0) }
    val paint=remember { Paint(Paint.ANTI_ALIAS_FLAG).apply {textAlign=Paint.Align.CENTER;typeface=android.graphics.Typeface.create("sans-serif-black", android.graphics.Typeface.BOLD)} }
    Canvas(Modifier.fillMaxWidth().height(140.dp).background(Color(0xFF131B25),RoundedCornerShape(20.dp))
        .testTag("appearance-preview").semantics {contentDescription="Botones de prueba Z, X, 1 y B"}
        .pointerInput(Unit) {
            awaitEachGesture {
                val down=awaitFirstDown();val index=(down.position.x/size.width*specs.size).toInt().coerceIn(0,specs.lastIndex)
                pressed=index;releasing=index;down.consume();feedback.press(view,current)
                waitForUpOrCancellation();pressed=null
            }
        }) {
        for((i,spec) in specs.withIndex()) {
            val p=if(i==(pressed?:releasing) && appearance.animations)progress else 0f
            val radius=minOf(size.height*.27f,size.width/specs.size*.32f)
            drawControl(spec,TouchGeometry(size.width*(i+.5f)/specs.size,size.height*.5f,radius),appearance,images,opacity,paint,1f-.08f*p,1f-.22f*p)
        }
    }
}

internal fun DrawScope.drawControl(spec:TouchControlSpec,g:TouchGeometry,appearance:TouchAppearance,images:Map<String,ImageBitmap>,alpha:Float,paint:Paint,scale:Float=1f,brightness:Float=1f) {
    val radius=g.radius*scale;val halfWidth=g.halfWidth*scale;val center=Offset(g.x,g.y)
    val sprite = images["custom/"+spec.id] ?: spec.asset?.let { images[it] }
    // Moderno: the artwork we already had.
    if (appearance.style == TouchStyle.MODERN && sprite != null) {
        drawImage(sprite,dstOffset=IntOffset((g.x-halfWidth).roundToInt(),(g.y-radius).roundToInt()),
            dstSize=IntSize((halfWidth*2).roundToInt().coerceAtLeast(1),(radius*2).roundToInt().coerceAtLeast(1)),
            alpha=alpha,filterQuality=FilterQuality.None,
            colorFilter=ColorFilter.tint(Color(brightness,brightness,brightness),BlendMode.Modulate))
        return
    }
    // Clasico, and the fallback for a Modern control without art: soft discs with rings.
    val tint = appearance.tint?.let { Color(0xFF000000.toInt() or it) }
    fun shade(g: Float, a: Float) = tint?.let { Color(it.red*g*brightness, it.green*g*brightness, it.blue*g*brightness, a) }
        ?: Color(g*brightness, g*brightness, (g+.01f)*brightness, a)
    val base=shade(.20f,.38f*alpha)
    val groove=shade(.115f,.42f*alpha)
    val raised=shade(.26f,.40f*alpha)
    val inner=shade(.235f,.40f*alpha)
    val ink=Color(.66f*brightness,.66f*brightness,.69f*brightness,.95f*alpha)
    if (kotlin.math.abs(spec.aspect-1f)>.12f) {
        drawRoundRect(groove,Offset(g.x-halfWidth*1.05f,g.y-radius*1.05f),Size(halfWidth*2.10f,radius*2.10f),CornerRadius(radius*.46f))
        drawRoundRect(base,Offset(g.x-halfWidth,g.y-radius),Size(halfWidth*2,radius*2),CornerRadius(radius*.40f))
        drawRoundRect(groove,Offset(g.x-halfWidth*.92f,g.y-radius*.92f),Size(halfWidth*1.84f,radius*1.84f),CornerRadius(radius*.36f))
        drawRoundRect(raised,Offset(g.x-halfWidth*.86f,g.y-radius*.86f),Size(halfWidth*1.72f,radius*1.72f),CornerRadius(radius*.34f))
        drawRoundRect(groove,Offset(g.x-halfWidth*.74f,g.y-radius*.74f),Size(halfWidth*1.48f,radius*1.48f),CornerRadius(radius*.30f))
        drawRoundRect(inner,Offset(g.x-halfWidth*.66f,g.y-radius*.66f),Size(halfWidth*1.32f,radius*1.32f),CornerRadius(radius*.26f))
    } else {
        drawCircle(base,radius,center)
        drawCircle(groove,radius*.92f,center,style=Stroke(maxOf(2f,radius*.05f)))
        drawCircle(raised,radius*.82f,center,style=Stroke(maxOf(2f,radius*.055f)))
        drawCircle(groove,radius*.70f,center,style=Stroke(maxOf(2f,radius*.04f)))
        drawCircle(inner,radius*.62f,center)
        drawCircle(shade(.34f,.20f*alpha),radius*.995f,center,style=Stroke(maxOf(1f,radius*.02f)))
    }
    paint.color=android.graphics.Color.rgb((ink.red*255).roundToInt(),(ink.green*255).roundToInt(),(ink.blue*255).roundToInt())
    paint.alpha=(ink.alpha*255).roundToInt();paint.textSize=radius*if(spec.label.length>2).5f else .8f
    drawContext.canvas.nativeCanvas.drawText(spec.label,g.x,g.y-(paint.ascent()+paint.descent())/2f,paint)
}

/** The modern movement control: a big translucent disc with a rounded cross or a knob. */
internal fun DrawScope.drawModernMovement(g: TouchGeometry, alpha: Float, paint: Paint, dpad: Boolean = true, tint: Int? = null) {
    val center = Offset(g.x, g.y)
    val r = g.radius
    val t = tint?.let { Color(0xFF000000.toInt() or it) }
    fun shade(gr: Float, a: Float) = t?.let { Color(it.red*gr, it.green*gr, it.blue*gr, a) } ?: Color(gr, gr, gr + .01f, a)
    drawCircle(Color(0f, 0f, 0f, .18f * alpha), r * 1.05f, center)
    drawCircle(shade(.21f, .36f * alpha), r, center)
    if (dpad) {
        val arm = r * .74f; val thick = r * .255f; val rad = r * .13f; val border = r * .065f
        fun crossPath(a: Float, t: Float, radius: Float): Path {
            val path = Path()
            path.addRoundRect(androidx.compose.ui.geometry.RoundRect(center.x - t, center.y - a, center.x + t, center.y + a, CornerRadius(radius)))
            path.addRoundRect(androidx.compose.ui.geometry.RoundRect(center.x - a, center.y - t, center.x + a, center.y + t, CornerRadius(radius)))
            path.fillType = PathFillType.NonZero
            return path
        }
        drawPath(crossPath(arm + border, thick + border, rad + border * .6f), shade(.10f, .66f * alpha))
        drawPath(crossPath(arm, thick, rad), shade(.30f, .55f * alpha))
        val ink = shade(.62f, .92f * alpha)
        fun arrow(dx: Float, dy: Float, up: Boolean) {
            val s = r * .115f; val px = center.x + dx; val py = center.y + dy
            val path = Path()
            if (up) { path.moveTo(px, py - s); path.lineTo(px - s, py + s); path.lineTo(px + s, py + s) }
            else if (dx < 0f) { path.moveTo(px - s, py); path.lineTo(px + s, py - s); path.lineTo(px + s, py + s) }
            else if (dx > 0f) { path.moveTo(px + s, py); path.lineTo(px - s, py - s); path.lineTo(px - s, py + s) }
            else { path.moveTo(px, py + s); path.lineTo(px - s, py - s); path.lineTo(px + s, py - s) }
            path.close(); drawPath(path, ink)
        }
        arrow(0f, -r * .45f, true); arrow(0f, r * .45f, false)
        arrow(-r * .45f, 0f, false); arrow(r * .45f, 0f, false)
    } else {
        drawCircle(shade(.27f, .72f * alpha), r * .34f, center)
        drawCircle(shade(.40f, .65f * alpha), r * .34f, center, style = Stroke(maxOf(2f, r * .04f)))
    }
}
