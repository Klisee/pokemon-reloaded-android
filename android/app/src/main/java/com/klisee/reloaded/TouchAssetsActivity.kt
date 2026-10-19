package com.klisee.reloaded

import android.content.Context
import android.net.Uri
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.lifecycle.ViewModel
import androidx.lifecycle.ViewModelProvider
import androidx.lifecycle.viewModelScope
import androidx.compose.runtime.*
import com.klisee.reloaded.ui.ReloadedTheme
import com.klisee.reloaded.ui.TouchAssetsScreen
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import java.io.IOException

class TouchAssetsModel:ViewModel() {
    var busy by mutableStateOf(false);private set
    var revision by mutableStateOf(0);private set
    var message by mutableStateOf<String?>(null)
    var pending by mutableStateOf<TouchControlPack.Prepared?>(null)
    fun run(success:String,task:suspend()->Unit) {
        if(busy)return
        busy=true;message=null
        viewModelScope.launch {
            try { task();revision++;message=success }
            catch(e:Exception) {message="No se pudo completar: ${e.message ?: "intentá nuevamente."}"}
            finally {busy=false}
        }
    }
    fun importSprite(context:Context,id:String,uri:Uri)=run("Sprite guardado. Se aplicará al iniciar el juego.") {
        withContext(Dispatchers.IO) {(context.contentResolver.openInputStream(uri) ?: throw IOException("No se pudo abrir el PNG.")).use {TouchSprites.importImage(context,Preferences(context),id,it)}}
    }
    fun preparePack(context:Context,uri:Uri)=run("Paquete validado.") {
        pending=withContext(Dispatchers.IO) {(context.contentResolver.openInputStream(uri) ?: throw IOException("No se pudo abrir el paquete.")).use {TouchControlPack.prepare(context,it)}}
    }
    fun exportPack(context:Context,uri:Uri,sprites:Boolean)=run("Paquete exportado correctamente.") {
        withContext(Dispatchers.IO) {(context.contentResolver.openOutputStream(uri,"wt") ?: throw IOException("No se pudo crear el archivo.")).use {TouchControlPack.export(context,Preferences(context),it,sprites)}}
    }
    fun applyPack(context:Context,settings:Boolean,sprites:Boolean) {
        val pack=pending ?: return
        run("Controles importados. Se aplicarán al iniciar el juego.") {
            withContext(Dispatchers.IO) {TouchControlPack.apply(context,Preferences(context),pack,settings,sprites)}
            pending=null
        }
    }
}
class TouchAssetsActivity:ComponentActivity() {
    override fun onCreate(savedInstanceState:Bundle?) {
        super.onCreate(savedInstanceState)
        val model=ViewModelProvider(this)[TouchAssetsModel::class.java]
        setContent {ReloadedTheme {TouchAssetsScreen(model) {finish()}}}
    }
}
