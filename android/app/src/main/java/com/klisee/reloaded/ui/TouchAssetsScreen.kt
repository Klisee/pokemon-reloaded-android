package com.klisee.reloaded.ui

import android.graphics.Paint
import androidx.activity.compose.BackHandler
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.*
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.IntSize
import androidx.compose.ui.unit.dp
import com.klisee.reloaded.*
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import kotlin.math.roundToInt

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun TouchAssetsScreen(model:TouchAssetsModel,onBack:()->Unit) {
    val context=LocalContext.current.applicationContext
    val prefs=remember {Preferences(context)}
    var profileId by rememberSaveable {mutableStateOf("world")}
    val profile=TouchProfile.entries.first {it.id==profileId}
    var profileMenu by remember {mutableStateOf(false)}
    var selected by rememberSaveable {mutableStateOf("z")}
    var pickingId by rememberSaveable {mutableStateOf("z")}
    var menu by remember {mutableStateOf(false)}
    var includeSprites by rememberSaveable {mutableStateOf(true)}
    var exportSprites by rememberSaveable {mutableStateOf(true)}
    var resetAll by remember {mutableStateOf(false)}
    val stored=remember(model.revision) {prefs.touchSprites}
    val appearance=remember(model.revision) {prefs.touchAppearance}
    val target=TouchSprites.target(selected)
    val selectedKey=TouchSprites.storageKey(selected,profile)
    val effectiveKey=TouchSprites.effectiveKey(selected,profile,stored)
    val pickImage=rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) {uri->uri?.let {model.importSprite(context,pickingId,it)}}
    val pickPack=rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) {uri->uri?.let {model.preparePack(context,it)}}
    val savePack=rememberLauncherForActivityResult(ActivityResultContracts.CreateDocument("application/zip")) {uri->uri?.let {model.exportPack(context,it,exportSprites)}}
    BackHandler(enabled=model.busy) {}
    Scaffold(topBar={TopAppBar(title={Text("Sprites y paquetes")},navigationIcon={TextButton(onClick=onBack,enabled=!model.busy){Text("Volver")}})}) {padding->
        Column(Modifier.fillMaxSize().padding(padding).verticalScroll(rememberScrollState()).padding(20.dp),verticalArrangement=Arrangement.spacedBy(12.dp)) {
            Text("Sprites personalizados",style=MaterialTheme.typography.titleLarge)
            Text("Elegí un PNG para cada botón o para la base y la palanca del joystick. Se guarda en la app y se aplica al iniciar el juego.")
            Box {
                OutlinedButton(onClick={profileMenu=true},enabled=!model.busy,modifier=Modifier.fillMaxWidth().testTag("sprite-profile")){Text("Perfil: ${profile.title}")}
                DropdownMenu(expanded=profileMenu,onDismissRequest={profileMenu=false}){
                    for(item in listOf(TouchProfile.WORLD,TouchProfile.BATTLE))DropdownMenuItem(text={Text(item.title)},onClick={profileId=item.id;profileMenu=false})
                }
            }
            Box {
                OutlinedButton(onClick={menu=true},enabled=!model.busy,modifier=Modifier.fillMaxWidth().testTag("sprite-picker")){Text(target.title)}
                DropdownMenu(expanded=menu,onDismissRequest={menu=false},modifier=Modifier.heightIn(max=300.dp)) {
                    for(item in TouchSprites.targets) DropdownMenuItem(text={Text(item.title+if(TouchSprites.effectiveKey(item.id,profile,stored) in stored)" · personalizado" else "")},onClick={selected=item.id;menu=false})
                }
            }
            SpriteSample(selected,appearance,prefs.touchOpacity,model.revision,profile)
            Text(if(effectiveKey in stored && stored[effectiveKey]!="original")"Sprite personalizado" else "Apariencia original del control",style=MaterialTheme.typography.labelLarge)
            Row(horizontalArrangement=Arrangement.spacedBy(8.dp)) {
                Button(onClick={pickingId=selectedKey;pickImage.launch(arrayOf("image/png"))},enabled=!model.busy,modifier=Modifier.weight(1f).testTag("sprite-import")){Text("Elegir PNG")}
                OutlinedButton(onClick={val id=selectedKey;model.run("Sprite original restaurado."){withContext(Dispatchers.IO){TouchSprites.reset(context,prefs,id)}}},
                    enabled=!model.busy&&effectiveKey in stored,modifier=Modifier.weight(1f).testTag("sprite-reset")){Text("Restaurar original")}
            }
            Text("Hasta 8 MB y 4096 × 4096. Se conserva la proporción con márgenes transparentes si hace falta. A, M, E, P, L, T y PKM tienen sprites independientes en Exploración y Combate. Los otros botones comparten su imagen. No cambia su función ni zona táctil.",style=MaterialTheme.typography.bodySmall)
            Text("Los sprites elegidos tienen prioridad sobre el estilo. El color y la opacidad también se aplican a los botones; elegí Color original para conservar sus colores.",style=MaterialTheme.typography.bodySmall)
            TextButton(onClick={resetAll=true},enabled=!model.busy&&stored.isNotEmpty(),modifier=Modifier.testTag("sprites-reset-all")){Text("Restaurar todos los sprites")}
            HorizontalDivider()
            Text("Importar y exportar controles",style=MaterialTheme.typography.titleLarge)
            Text("Guardá o compartí un ZIP con los botones de cada perfil, posiciones, tamaños, visibilidad, color, estilo, opacidad, animaciones y vibración.")
            Row(verticalAlignment=Alignment.CenterVertically) {
                Text("Incluir sprites personalizados al exportar",Modifier.weight(1f))
                Switch(includeSprites,{includeSprites=it},enabled=!model.busy,modifier=Modifier.testTag("export-sprites"))
            }
            Row(horizontalArrangement=Arrangement.spacedBy(8.dp)) {
                OutlinedButton(onClick={pickPack.launch(arrayOf("application/zip","application/x-zip-compressed","application/octet-stream"))},enabled=!model.busy,modifier=Modifier.weight(1f).testTag("pack-import")){Text("Importar paquete")}
                Button(onClick={exportSprites=includeSprites;savePack.launch(if(includeSprites)"reloaded-controles-completo.zip" else "reloaded-controles-configuracion.zip")},enabled=!model.busy,modifier=Modifier.weight(1f).testTag("pack-export")){Text("Exportar paquete")}
            }
            Text("Al importar podés elegir ajustes, sprites o ambos. Las posiciones se adaptan a la pantalla. Las partidas no forman parte del paquete.",style=MaterialTheme.typography.bodySmall)
            if(model.busy)LinearProgressIndicator(Modifier.fillMaxWidth())
            model.message?.let {Text(it,color=MaterialTheme.colorScheme.primary,modifier=Modifier.testTag("assets-message"))}
        }
    }
    if(resetAll)AlertDialog(onDismissRequest={resetAll=false},title={Text("¿Restaurar todos los sprites?")},text={Text("Se quitarán las imágenes personalizadas. Se conservan posiciones, tamaños y demás ajustes.")},
        confirmButton={TextButton(onClick={resetAll=false;model.run("Sprites originales restaurados."){withContext(Dispatchers.IO){prefs.saveTouchSprites(emptyMap())}}}){Text("Restaurar")}},dismissButton={TextButton(onClick={resetAll=false}){Text("Cancelar")}})
    model.pending?.let {pack->
        var settings by remember(pack){mutableStateOf(true)}
        var sprites by remember(pack){mutableStateOf(pack.sprites!=null)}
        AlertDialog(onDismissRequest={if(!model.busy)model.pending=null},title={Text("Importar controles")},text={Column(verticalArrangement=Arrangement.spacedBy(8.dp)) {
            Text("Se reemplazarán las opciones seleccionadas. Exportá tus controles actuales primero si querés conservar una copia.")
            Row(verticalAlignment=Alignment.CenterVertically){Checkbox(settings,{settings=it},enabled=!model.busy,modifier=Modifier.testTag("import-settings"));Text("Botones, distribución y ajustes de todos los perfiles")}
            if(pack.sprites!=null)Row(verticalAlignment=Alignment.CenterVertically){Checkbox(sprites,{sprites=it},enabled=!model.busy,modifier=Modifier.testTag("import-sprites"));Text("Sprites (${pack.sprites.size} personalizados)")}
            else Text("Este paquete no incluye sprites; se conservan los actuales.")
            model.message?.takeIf{it.startsWith("No se pudo")}?.let {Text(it)}
        }},confirmButton={TextButton(onClick={model.applyPack(context,settings,sprites)},enabled=!model.busy&&(settings||sprites),modifier=Modifier.testTag("pack-apply")){Text("Importar")}},dismissButton={TextButton(onClick={model.pending=null},enabled=!model.busy){Text("Cancelar")}})
    }
}

@Composable
private fun SpriteSample(id:String,appearance:TouchAppearance,opacity:Float,revision:Int,profile:TouchProfile) {
    val images=controlImages(appearance,revision,profile)
    val spec=TouchLayouts.controls.first {it.id==if(id=="joystick-knob")"joystick" else id}
    val paint=remember {Paint(Paint.ANTI_ALIAS_FLAG).apply {textAlign=Paint.Align.CENTER;typeface=android.graphics.Typeface.DEFAULT_BOLD}}
    Canvas(Modifier.fillMaxWidth().height(132.dp).background(Color(0xFF131B25),RoundedCornerShape(20.dp)).testTag("sprite-preview")) {
        val r=minOf(size.height*.34f,size.width/(spec.aspect*2.5f))
        if(spec.joystick)for((name,factor) in listOf("joystick-base.png" to 1.05f,"joystick-knob.png" to .88f))images[name]?.let {image->
            val half=r*factor
            drawImage(image,dstOffset=IntOffset((size.width/2-half).roundToInt(),(size.height/2-half).roundToInt()),dstSize=IntSize((half*2).roundToInt(),(half*2).roundToInt()),alpha=1f,filterQuality=FilterQuality.None)
        } else drawControl(spec,TouchGeometry(size.width/2,size.height/2,r,r*spec.aspect),appearance,images,opacity,paint)
    }
}
