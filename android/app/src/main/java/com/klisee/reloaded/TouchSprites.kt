package com.klisee.reloaded

import android.content.Context
import android.graphics.*
import android.util.AtomicFile
import java.io.*
import java.security.MessageDigest
import org.json.JSONObject

/** Global legacy art with independent world/battle overrides for shared menu buttons. */
object TouchSprites {
    data class Target(val id: String, val title: String, val asset: String?)
    val targets = TouchLayouts.controls.map { Target(it.id, if (it.joystick) "Joystick · base" else it.title, it.asset) } + Target("joystick-knob", "Joystick · palanca", "joystick-knob.png")
    val independent=setOf("m","e","p","l","t","space","a")
    fun storageKey(id:String,profile:TouchProfile)=if(id in independent && profile in setOf(TouchProfile.WORLD,TouchProfile.BATTLE)) "${profile.id}/$id" else id
    fun baseId(id:String)=id.substringAfter('/')
    val keys:List<String> get()=targets.map {it.id}+listOf("world","battle").flatMap {p->independent.map {"$p/$it"}}
    fun validKey(id:String)=id in keys
    fun effectiveKey(id:String,profile:TouchProfile,values:Map<String,String>):String=storageKey(id,profile).takeIf {it in values}?:id
    fun bundledAsset(id: String, profile: TouchProfile): String? =
        if (profile == TouchProfile.BATTLE && id in setOf("a", "m", "e")) "battle__${id}.png" else target(id).asset
    private val hashPattern = Regex("[0-9a-f]{64}")
    const val MAX_IMAGE_BYTES = 8 * 1024 * 1024
    fun target(id: String) = targets.firstOrNull { validKey(id) && it.id == baseId(id) } ?: throw IOException("Control desconocido en el paquete.")
    fun encode(values: Map<String, String>) = JSONObject(values).toString()
    fun parse(text: String): Map<String, String> = runCatching {
        val json = JSONObject(text)
        keys.mapNotNull { id -> json.optString(id).takeIf { it.matches(hashPattern) || it=="original" && '/' in id }?.let { id to it } }.toMap()
    }.getOrDefault(emptyMap())
    fun file(context: Context, hash: String): File {
        require(hash.matches(hashPattern))
        return File(context.filesDir, "touch-sprites/$hash.png")
    }
    fun read(context: Context, id: String, values: Map<String, String>): ByteArray? = values[id]?.let { hash ->
        if(hash=="original") null else file(context, hash).takeIf { it.isFile && it.length() <= MAX_IMAGE_BYTES }?.readBytes()
    }
    fun bounded(input: InputStream, limit: Int): ByteArray {
        val out = ByteArrayOutputStream(); val buffer = ByteArray(8192)
        while (true) {
            val n = input.read(buffer); if (n < 0) break
            if (out.size().toLong() + n > limit) throw IOException("El archivo supera el tamaño permitido.")
            out.write(buffer, 0, n)
        }
        return out.toByteArray()
    }
    /** Fit artwork to the existing aspect ratio, without changing the engine's hit targets. */
    fun normalize(context: Context, id: String, data: ByteArray): ByteArray {
        val t = target(id)
        if (data.size > MAX_IMAGE_BYTES || data.size < 8 || !data.copyOfRange(0,8).contentEquals(byteArrayOf(-119,80,78,71,13,10,26,10)))
            throw IOException("Elegí una imagen PNG válida, de hasta 8 MB.")
        val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
        BitmapFactory.decodeByteArray(data,0,data.size,bounds)
        if (bounds.outWidth !in 1..4096 || bounds.outHeight !in 1..4096) throw IOException("El PNG debe medir entre 1 y 4096 píxeles por lado.")
        var sample=1
        while (maxOf(bounds.outWidth,bounds.outHeight)/sample > 512) sample*=2
        val bitmap=BitmapFactory.decodeByteArray(data,0,data.size,BitmapFactory.Options().apply { inSampleSize=sample }) ?: throw IOException("PNG dañado.")
        val original=BitmapFactory.Options().apply { inJustDecodeBounds=true }
        t.asset?.let { context.assets.open(it).use { input -> BitmapFactory.decodeStream(input,null,original) } }
        val baseW=original.outWidth.coerceAtLeast(1);val baseH=original.outHeight.coerceAtLeast(1)
        val factor=(256/maxOf(baseW,baseH)).coerceAtLeast(1)
        val w=baseW*factor;val h=baseH*factor
        val output=Bitmap.createBitmap(w,h,Bitmap.Config.ARGB_8888)
        try {
            val fit=minOf(w.toFloat()/bitmap.width,h.toFloat()/bitmap.height)
            val dw=bitmap.width*fit;val dh=bitmap.height*fit
            Canvas(output).drawBitmap(bitmap,null,RectF((w-dw)/2,(h-dh)/2,(w+dw)/2,(h+dh)/2),Paint())
            return ByteArrayOutputStream().use { bytes ->
                if (!output.compress(Bitmap.CompressFormat.PNG,100,bytes)) throw IOException("No se pudo preparar el sprite.")
                bytes.toByteArray()
            }
        } finally { bitmap.recycle();output.recycle() }
    }
    private fun write(target: File, bytes: ByteArray) {
        val atomic=AtomicFile(target);val stream=atomic.startWrite()
        try { stream.write(bytes);atomic.finishWrite(stream) }
        catch (e:Exception) { atomic.failWrite(stream);throw e }
    }
    fun stage(context: Context, images: Map<String,ByteArray>): Map<String,String> = images.mapValues { (id,bytes) ->
        target(id)
        if(bytes.isEmpty() && '/' in id)return@mapValues "original"
        val hash=MessageDigest.getInstance("SHA-256").digest(bytes).joinToString("") { "%02x".format(it) }
        val path=file(context,hash);path.parentFile?.mkdirs()
        if (!path.isFile) write(path,bytes)
        hash
    }
    fun importImage(context: Context, prefs: Preferences, id: String, input: InputStream) {
        val data=normalize(context,id,bounded(input,MAX_IMAGE_BYTES))
        prefs.saveTouchSprites(prefs.touchSprites + stage(context,mapOf(id to data)))
    }
    fun reset(context:Context,prefs:Preferences,id:String){
        val old=prefs.touchSprites
        prefs.saveTouchSprites(if('/' in id && baseId(id) in old)old+(id to "original")else old-id)
    }
    fun writeRuntime(context: Context, values: Map<String,String>) {
        val dir=File(context.filesDir,"touch-sprites-active").apply { mkdirs() }
        for (id in keys) {
            val name=id.replace("/","__")
            val marker=File(dir,"$name.original")
            if(values[id]=="original")write(marker,byteArrayOf(1))else marker.delete()
            val out=File(dir,"$name.png");val bytes=read(context,id,values)
            if(bytes==null) { out.delete();File(dir,"$name.png.bak").delete() } else write(out,bytes)
        }
    }
}
