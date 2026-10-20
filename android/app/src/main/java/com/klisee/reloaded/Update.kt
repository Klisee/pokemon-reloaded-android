package com.klisee.reloaded

import android.content.pm.PackageInfo
import android.content.pm.PackageManager
import android.os.Build
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.json.JSONObject
import java.io.File
import java.net.HttpURLConnection
import java.net.URL
import java.security.MessageDigest
import java.util.zip.ZipFile

/**
 * Looks for a newer build on GitHub. The repository's releases are the only place a version is
 * published, so there is no server to keep running and nothing to keep in step.
 */
object Update {
    private const val REPO = "Klisee/pokemon-reloaded-android"
    private const val LATEST = "https://api.github.com/repos/$REPO/releases/latest"
    const val RELEASES_PAGE = "https://github.com/$REPO/releases/latest"

    data class Release(val version: String, val apkUrl: String?, val pageUrl: String, val digest: String? = null)
    internal data class Asset(val name: String, val url: String, val digest: String?)

    /** The newer release, or `null` when the installed build is already the latest one. */
    suspend fun check(installed: String): Result<Release?> = withContext(Dispatchers.IO) {
        runCatching {
            val json = JSONObject(get(LATEST))
            val tag = json.optString("tag_name").trim().trimStart('v', 'V')
            if (tag.isEmpty() || !isNewer(tag, installed)) {
                return@runCatching null
            }
            val assets = json.optJSONArray("assets")?.let { array ->
                (0 until array.length()).mapNotNull { index ->
                    val asset = array.getJSONObject(index)
                    val name = asset.optString("name")
                    val url = asset.optString("browser_download_url")
                    if (name.isEmpty() || url.isEmpty()) null
                    else Asset(name, url, asset.optString("digest").takeIf { it.isNotEmpty() })
                }
            }.orEmpty()
            val apk = chooseApk(assets, android.os.Build.SUPPORTED_ABIS.toList())
            Release(tag, apk?.url, json.optString("html_url").ifEmpty { RELEASES_PAGE }, apk?.digest)
        }
    }

    /**
     * Picks the APK built for this device. Releases ship one per ABI, plus a universal fallback, so
     * taking the first `.apk` was wrong: a 32-bit phone could download the arm64 build and Android
     * would then refuse to install it. [abis] is ordered by preference, as `SUPPORTED_ABIS` is.
     */
    internal fun chooseApk(assets: List<Asset>, abis: List<String>): Asset? {
        val apks = assets.filter { it.name.endsWith(".apk", true) && it.url.isNotEmpty() }
        // 1. The build made for an ABI this device prefers (SUPPORTED_ABIS is ordered).
        for (abi in abis.map { it.lowercase() }) {
            apks.firstOrNull { it.name.lowercase().endsWith("-$abi.apk") }?.let { return it }
        }
        // 2. The universal build, which carries every ABI.
        apks.firstOrNull { it.name.lowercase().endsWith("-universal.apk") }?.let { return it }
        // 3. A single, untagged APK (the way older releases shipped). An APK tagged with a
        //    different ABI is never chosen: installing it would fail, or run the wrong binary.
        val knownAbis = setOf("arm64-v8a", "armeabi-v7a", "x86_64", "x86", "riscv64")
        return apks.firstOrNull { asset ->
            val stem = asset.name.lowercase().removeSuffix(".apk")
            knownAbis.none { stem.endsWith("-$it") } && !stem.endsWith("-universal")
        }
    }

    /**
     * Downloads the release's APK so the system installer can be handed a local file.
     *
     * Reports the bytes as they arrive, because sixty megabytes with no sign of life cannot be told
     * apart from a download that has died. Everything that happens is also written to the log, since
     * the failure used to be swallowed and the app simply opened the browser as if nothing was wrong.
     */
    suspend fun download(url: String, target: File, expectedDigest: String?, onProgress: (Long, Long) -> Unit): Result<File> = withContext(Dispatchers.IO) {
        runCatching {
            val connection = open(url)
            connection.connect()
            val code = connection.responseCode
            val total = connection.contentLengthLong
            trace("update: descarga $code, $total bytes desde $url")
            if (code !in 200..299) error("La descarga respondio $code.")
            val sha = MessageDigest.getInstance("SHA-256")
            connection.inputStream.use { input ->
                target.outputStream().use { output ->
                    val buffer = ByteArray(64 * 1024)
                    var written = 0L
                    while (true) {
                        val read = input.read(buffer)
                        if (read < 0) break
                        output.write(buffer, 0, read)
                        sha.update(buffer, 0, read)
                        written += read
                        onProgress(written, total)
                    }
                    trace("update: descargados $written bytes")
                }
            }
            val actual = "sha256:" + sha.digest().joinToString("") { "%02x".format(it) }
            trace("update: sha256 $actual")
            if (!expectedDigest.isNullOrBlank() && !expectedDigest.equals(actual, ignoreCase = true)) {
                target.delete()
                error("El archivo descargado no coincide con el de la release (posible corrupcion).")
            }
            target
        }.onFailure { trace("update: FALLO la descarga: ${it::class.java.simpleName}: ${it.message}") }
    }

    private fun trace(message: String) {
        android.util.Log.i("Reloaded", message)
    }

    /**
     * True when `candidate` is ahead of `installed`, comparing the numbers one by one so that 0.1.10
     * counts as newer than 0.1.9 (a plain string comparison would get that wrong).
     */
    fun isNewer(candidate: String, installed: String): Boolean {
        val a = numbers(candidate)
        val b = numbers(installed)
        for (index in 0 until maxOf(a.size, b.size)) {
            val left = a.getOrElse(index) { 0 }
            val right = b.getOrElse(index) { 0 }
            if (left != right) return left > right
        }
        return false
    }

    private fun numbers(value: String) = value.trim().split('.', '-', '+').mapNotNull { it.toIntOrNull() }

    private fun open(url: String) = (URL(url).openConnection() as HttpURLConnection).apply {
        connectTimeout = 20_000
        readTimeout = 20_000
        setRequestProperty("User-Agent", "ReloadedLauncher/1.0")
        setRequestProperty("Accept", "application/vnd.github+json")
    }

    private fun get(url: String): String {
        val connection = open(url)
        connection.connect()
        if (connection.responseCode !in 200..299) error("GitHub respondio ${connection.responseCode}.")
        return connection.inputStream.bufferedReader().use { it.readText() }
    }
}

/**
 * Hands a downloaded APK to the system installer, which is the only thing allowed to replace the
 * app. Android asks the user to allow installs from this app the first time, and if that has not
 * been granted there is nothing to open - so that case is reported instead of failing quietly.
 */
fun installUpdate(context: android.content.Context, apk: File): String? {
    val pm = context.packageManager
    // Never hand an arbitrary file to the installer: check it is this app, newer, built for this
    // device and signed with the same key. Android would reject most of these anyway, but the user
    // deserves a clear reason instead of a bare failure, and a wrong-ABI APK must never be offered.
    // GET_SIGNING_CERTIFICATES and longVersionCode arrived in API 28; the app runs from API 26.
    val packageFlags = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
        PackageManager.GET_SIGNING_CERTIFICATES
    } else {
        @Suppress("DEPRECATION") PackageManager.GET_SIGNATURES
    }
    val archive = runCatching { pm.getPackageArchiveInfo(apk.path, packageFlags) }.getOrNull()
        ?: return "El APK descargado no se puede leer."
    if (archive.packageName != context.packageName) return "Ese APK no es Pokemon Reloaded."
    val installedInfo = runCatching { pm.getPackageInfo(context.packageName, packageFlags) }.getOrNull()
    if (versionCode(archive) < (installedInfo?.let(::versionCode) ?: 0L)) {
        return "Ese APK es mas viejo que el que tienes instalado."
    }
    val deviceAbis = android.os.Build.SUPPORTED_ABIS.toSet()
    val forThisDevice = runCatching {
        ZipFile(apk).use { zip ->
            zip.entries().asSequence().any { entry ->
                entry.name.startsWith("lib/") && entry.name.endsWith(".so") &&
                    entry.name.removePrefix("lib/").substringBefore('/') in deviceAbis
            }
        }
    }.getOrDefault(false)
    if (!forThisDevice) return "Ese APK no trae una version para la arquitectura de este movil."
    val installedSigners = signers(installedInfo)
    val archiveSigners = signers(archive)
    if (installedSigners != null && archiveSigners != null && installedSigners != archiveSigners) {
        return "Ese APK no esta firmado con la misma clave que la app instalada."
    }
    // Without this Android will not open an installer for a file from this app, and it fails with
    // nothing to see. Sending the user straight to the switch is the only way to get anywhere.
    if (!context.packageManager.canRequestPackageInstalls()) {
        return runCatching {
            context.startActivity(
                android.content.Intent(android.provider.Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES)
                    .setData(android.net.Uri.parse("package:${context.packageName}"))
                    .addFlags(android.content.Intent.FLAG_ACTIVITY_NEW_TASK),
            )
            "Activa \"Permitir desde esta fuente\" y vuelve a darle a Descargar."
        }.getOrElse { "Android no deja instalar desde esta app: ${it.message}" }
    }
    return runCatching {
        val uri = androidx.core.content.FileProvider.getUriForFile(context, "${context.packageName}.updates", apk)
        // (signature/ABI/version already verified above)
        val intent = android.content.Intent(android.content.Intent.ACTION_VIEW)
            .setDataAndType(uri, "application/vnd.android.package-archive")
            .addFlags(android.content.Intent.FLAG_GRANT_READ_URI_PERMISSION or android.content.Intent.FLAG_ACTIVITY_NEW_TASK)
        context.startActivity(intent)
        null
    }.getOrElse { "No pude abrir el instalador: ${it::class.java.simpleName}: ${it.message}" }
}

/** The certificate(s) an APK is signed with, or null when the system does not report them. */
private fun signers(info: PackageInfo?): Set<List<Byte>>? {
    if (info == null) return null
    val certificates = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
        info.signingInfo?.apkContentsSigners
    } else {
        @Suppress("DEPRECATION") info.signatures
    }
    return certificates?.map { it.toByteArray().toList() }?.toSet()
}

private fun versionCode(info: PackageInfo): Long =
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
        info.longVersionCode
    } else {
        @Suppress("DEPRECATION") info.versionCode.toLong()
    }
