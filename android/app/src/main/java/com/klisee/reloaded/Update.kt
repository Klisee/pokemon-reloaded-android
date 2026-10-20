package com.klisee.reloaded

import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.json.JSONObject
import java.io.File
import java.net.HttpURLConnection
import java.net.URL

/**
 * Looks for a newer build on GitHub. The repository's releases are the only place a version is
 * published, so there is no server to keep running and nothing to keep in step.
 */
object Update {
    private const val REPO = "Klisee/pokemon-reloaded-android"
    private const val LATEST = "https://api.github.com/repos/$REPO/releases/latest"
    const val RELEASES_PAGE = "https://github.com/$REPO/releases/latest"

    data class Release(val version: String, val apkUrl: String?, val pageUrl: String)

    /** The newer release, or `null` when the installed build is already the latest one. */
    suspend fun check(installed: String): Result<Release?> = withContext(Dispatchers.IO) {
        runCatching {
            val json = JSONObject(get(LATEST))
            val tag = json.optString("tag_name").trim().trimStart('v', 'V')
            if (tag.isEmpty() || !isNewer(tag, installed)) {
                return@runCatching null
            }
            val apk = chooseApk(json.optJSONArray("assets"), android.os.Build.SUPPORTED_ABIS.toList())
            Release(tag, apk, json.optString("html_url").ifEmpty { RELEASES_PAGE })
        }
    }

    /**
     * Picks the APK built for this device. Releases ship one per ABI, plus a universal fallback, so
     * taking the first `.apk` was wrong: a 32-bit phone could download the arm64 build and Android
     * would then refuse to install it. [abis] is ordered by preference, as `SUPPORTED_ABIS` is.
     */
    internal fun chooseApk(assets: org.json.JSONArray?, abis: List<String>): String? {
        val apks = (0 until (assets?.length() ?: 0)).mapNotNull { index ->
            val asset = assets!!.getJSONObject(index)
            val name = asset.optString("name")
            val url = asset.optString("browser_download_url")
            if (name.endsWith(".apk", true) && url.isNotEmpty()) name.lowercase() to url else null
        }
        val forDevice = abis.map { it.lowercase() }.firstNotNullOfOrNull { abi ->
            apks.firstOrNull { it.first.endsWith("-$abi.apk") }?.second
        }
        return forDevice
            ?: apks.firstOrNull { it.first.endsWith("-universal.apk") }?.second
            ?: apks.firstOrNull()?.second
    }

    /**
     * Downloads the release's APK so the system installer can be handed a local file.
     *
     * Reports the bytes as they arrive, because sixty megabytes with no sign of life cannot be told
     * apart from a download that has died. Everything that happens is also written to the log, since
     * the failure used to be swallowed and the app simply opened the browser as if nothing was wrong.
     */
    suspend fun download(url: String, target: File, onProgress: (Long, Long) -> Unit): Result<File> = withContext(Dispatchers.IO) {
        runCatching {
            val connection = open(url)
            connection.connect()
            val code = connection.responseCode
            val total = connection.contentLengthLong
            trace("update: descarga $code, $total bytes desde $url")
            if (code !in 200..299) error("La descarga respondio $code.")
            connection.inputStream.use { input ->
                target.outputStream().use { output ->
                    val buffer = ByteArray(64 * 1024)
                    var written = 0L
                    while (true) {
                        val read = input.read(buffer)
                        if (read < 0) break
                        output.write(buffer, 0, read)
                        written += read
                        onProgress(written, total)
                    }
                    trace("update: descargados $written bytes")
                }
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
        val intent = android.content.Intent(android.content.Intent.ACTION_VIEW)
            .setDataAndType(uri, "application/vnd.android.package-archive")
            .addFlags(android.content.Intent.FLAG_GRANT_READ_URI_PERMISSION or android.content.Intent.FLAG_ACTIVITY_NEW_TASK)
        context.startActivity(intent)
        null
    }.getOrElse { "No pude abrir el instalador: ${it::class.java.simpleName}: ${it.message}" }
}
