package com.klisee.reloaded

import android.content.Context
import java.io.*
import java.util.zip.*
import org.json.JSONObject

/** Self-contained versioned ZIP; never touches game data and never extracts untrusted paths. */
object TouchControlPack {
    private const val FORMAT="pokemon-reloaded-touch-controls"
    private const val MAX_TOTAL=16*1024*1024
    data class Prepared(val layouts:Map<String,TouchPlacement>,val appearance:TouchAppearance,val opacity:Int,val sprites:Map<String,ByteArray>?)
    fun export(context:Context,prefs:Preferences,output:OutputStream,includeSprites:Boolean) {
        val refs=prefs.touchSprites
        val images=if(includeSprites) refs.filterValues {it!="original"}.mapValues { (id,_) -> TouchSprites.read(context,id,refs) ?: throw IOException("Falta un sprite. Restaurá ese botón e intentá de nuevo.") } else emptyMap()
        val manifest=JSONObject().put("format",FORMAT).put("version",2).put("layouts",TouchLayouts.encode(prefs.touchLayouts()))
            .put("appearance",prefs.touchAppearance.encode()).put("opacity",prefs.touchOpacityPercent).put("includesSprites",includeSprites)
            .put("sprites",JSONObject(images.keys.associateWith { "sprites/$it.png" }))
            .put("originalSprites",org.json.JSONArray(if(includeSprites)refs.filterValues {it=="original"}.keys.toList()else emptyList<String>()))
        ZipOutputStream(output).use { zip ->
            zip.putNextEntry(ZipEntry("manifest.json"));zip.write(manifest.toString(2).toByteArray());zip.closeEntry()
            for ((id,bytes) in images) { zip.putNextEntry(ZipEntry("sprites/$id.png"));zip.write(bytes);zip.closeEntry() }
        }
    }
    fun prepare(context:Context,input:InputStream):Prepared {
        val entries=linkedMapOf<String,ByteArray>();var total=0
        ZipInputStream(input).use { zip ->
            while(true) {
                val entry=zip.nextEntry ?: break;val name=entry.name
                if(entry.isDirectory || entries.containsKey(name) || entries.size>=TouchSprites.keys.size+1 ||
                    (name!="manifest.json" && !name.matches(Regex("sprites/(?:world/|battle/)?[a-z0-9_-]+\\.png")))) throw IOException("Archivos no válidos en el paquete.")
                val limit=if(name=="manifest.json")128*1024 else 1024*1024
                val bytes=TouchSprites.bounded(zip,minOf(limit,MAX_TOTAL-total));total+=bytes.size;entries[name]=bytes;zip.closeEntry()
            }
        }
        val raw=entries.remove("manifest.json") ?: throw IOException("No es un paquete de controles de Pokémon Reloaded.")
        val manifest=try { JSONObject(String(raw,Charsets.UTF_8)) } catch(e:Exception) { throw IOException("El paquete está dañado.",e) }
        if(manifest.optString("format")!=FORMAT || manifest.optInt("version",-1) !in 1..2) throw IOException("Formato incompatible. Usá un paquete exportado desde esta versión.")
        val layoutText=manifest.getString("layouts");val layouts=TouchLayouts.parse(layoutText)
        if(layoutText.trimEnd()!=TouchLayouts.encode(layouts).trimEnd())throw IOException("Distribución o perfiles no válidos.")
        val appearanceText=manifest.getString("appearance");val appearance=TouchAppearance.parse(appearanceText)
        if(appearanceText.trimEnd()!=appearance.encode().trimEnd())throw IOException("Apariencia incompatible.")
        val opacity=manifest.getInt("opacity");if(opacity !in 5..100)throw IOException("Opacidad no válida.")
        val included=manifest.getBoolean("includesSprites");val spriteMap=manifest.getJSONObject("sprites")
        val images=linkedMapOf<String,ByteArray>()
        for(id in spriteMap.keys()) {
            TouchSprites.target(id);val path=spriteMap.getString(id)
            if(path!="sprites/$id.png" || !included)throw IOException("Paquete de sprites no válido.")
            val bytes=entries.remove(path) ?: throw IOException("Falta la imagen de un botón.")
            images[id]=TouchSprites.normalize(context,id,bytes)
        }
        manifest.optJSONArray("originalSprites")?.let {defaults->
            for(i in 0 until defaults.length()){
                val id=defaults.getString(i)
                if(!included || !TouchSprites.validKey(id) || '/' !in id || id in images)throw IOException("Restauración de sprites no válida.")
                images[id]=byteArrayOf()
            }
        }
        if(entries.isNotEmpty())throw IOException("El paquete contiene archivos no reconocidos.")
        return Prepared(layouts,appearance,opacity,if(included)images else null)
    }
    fun apply(context:Context,prefs:Preferences,pack:Prepared,settings:Boolean,sprites:Boolean) {
        val nextSprites=if(sprites && pack.sprites!=null)TouchSprites.stage(context,pack.sprites)else prefs.touchSprites
        prefs.saveTouchConfiguration(if(settings)pack.layouts else prefs.touchLayouts(),if(settings)pack.appearance else prefs.touchAppearance,
            if(settings)pack.opacity else prefs.touchOpacityPercent,nextSprites)
    }
}
