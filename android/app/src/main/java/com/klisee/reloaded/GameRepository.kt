package com.klisee.reloaded

import android.content.Context
import android.net.Uri
import android.os.SystemClock
import android.provider.OpenableColumns
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.json.JSONObject
import java.io.File
import java.io.IOException
import java.io.RandomAccessFile
import java.net.HttpURLConnection
import java.net.URL
import java.nio.ByteBuffer
import java.nio.charset.CodingErrorAction
import java.util.Locale
import java.util.concurrent.Callable
import java.util.concurrent.ExecutionException
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicLong
import java.util.zip.ZipEntry
import java.util.zip.ZipFile
import java.util.zip.ZipInputStream
import java.util.zip.ZipOutputStream

/**
 * Owns the game in the app's private storage.
 *
 * The launcher inflates the game itself (so the emulator no longer depends on any archive name)
 * into `files/game/`, and records where it came from in `files/game.json`.
 */
class GameRepository(private val context: Context) {

    sealed interface Status {
        data object Missing : Status
        data class Ready(
            val exe: String,
            val version: String?,
            val source: String?,
            val installedAt: Long,
            val sizeBytes: Long,
        ) : Status
    }

    private val files: File get() = context.filesDir
    private val gameDir: File get() = File(files, "game")
    private val metaFile: File get() = File(files, "game.json")
    val logFile: File get() = File(files, "emulator.log")

    /**
     * Writing to app.log is kept off the caller's thread: on adopted storage a stalled card would
     * otherwise block the install itself (and the UI) just because it was writing a log line.
     */
    private val logWriter = Executors.newSingleThreadExecutor { runnable ->
        Thread(runnable, "app-log").apply { isDaemon = true }
    }

    /** The emulator's executable name is not pinned, so it is discovered by extension. */
    fun status(): Status {
        val exe = findExe(gameDir) ?: return Status.Missing
        // A gutted `data` folder is not an install either: the game would start and fail.
        val data = File(gameDir, "data")
        if (!data.isDirectory || (data.listFiles()?.size ?: 0) < 5) return Status.Missing
        val meta = readMeta()
        return Status.Ready(
            exe = exe.name,
            // Nothing ever recorded a version on install, so it is read off the executable's own
            // name, which is where the author puts it: "... The Last Beta 1.9.2.2 Full.exe".
            version = meta?.optString("version")?.ifEmpty { null } ?: versionFromName(exe.name),
            source = meta?.optString("source")?.ifEmpty { null },
            installedAt = meta?.optLong("installedAt", 0L) ?: 0L,
            sizeBytes = meta?.optLong("sizeBytes", 0L) ?: 0L,
        )
    }

    data class ReloadedOnlineConfig(
        val enabled: Boolean,
        val server: String,
        val name: String,
        val room: String,
        val bridgeUpstream: String = "",
    )

    /** Reads the Reloaded Online startup config, or sensible defaults when it is absent. */
    fun reloadedOnlineConfig(): ReloadedOnlineConfig {
        val file = File(gameDir, "reloaded-online.json")
        val json = runCatching { JSONObject(file.readText()) }.getOrNull()
        return ReloadedOnlineConfig(
            enabled = json?.optBoolean("enabled", false) ?: false,
            server = json?.optString("server")?.ifBlank { null } ?: "127.0.0.1:7777",
            name = json?.optString("name")?.ifBlank { null } ?: "Jugador",
            room = json?.optString("room")?.ifBlank { null } ?: "mundo",
            bridgeUpstream = json?.optString("bridge_upstream")?.ifBlank { null } ?: "",
        )
    }

    /** Writes the config the native side reads once, at game startup. */
    fun writeReloadedOnline(config: ReloadedOnlineConfig): Result<Unit> = runCatching {
        if (!gameDir.isDirectory) error("Primero instala el juego.")
        val json = JSONObject()
            .put("enabled", config.enabled)
            .put("server", config.server.trim())
            .put("name", config.name.trim().ifEmpty { "Jugador" })
            .put("room", config.room.trim().ifEmpty { "mundo" })
            .put("bridge_upstream", config.bridgeUpstream.trim())
        File(gameDir, "reloaded-online.json").writeText(json.toString())
    }

    /** The first version-looking number in a name, or null if it has none. */
    private fun versionFromName(name: String): String? =
        Regex("""\d+\.\d+(?:\.\d+){1,2}""").find(name)?.value

    /** Frees the extracted game and any half-finished install, leaving the launcher ready again. */
    suspend fun deleteGame() = withContext(Dispatchers.IO) {
        gameDir.deleteRecursively()
        metaFile.delete()
        File(files, "game.staging").deleteRecursively()
        File(files, "game.old").deleteRecursively()
        File(files, "game.saves").deleteRecursively()
        Unit
    }

    /**
     * Finishes or rolls back an install that was interrupted (the app was killed, the phone ran out
     * of battery). It runs once at startup, before the home screen asks for the game's status, so a
     * stray `game.staging` cannot make a good install look missing.
     */
    private fun recoverInterruptedInstall() {
        val staging = File(files, "game.staging")
        val previous = File(files, "game.old")
        if (looksLikeGame(gameDir)) {
            staging.deleteRecursively()
            previous.deleteRecursively()
            return
        }
        // A `game` folder without an exe or data is a leftover, not an install; it must not block
        // the complete extraction (or the previous game) waiting beside it.
        if (gameDir.exists()) gameDir.deleteRecursively()
        if (looksLikeGame(staging) && staging.renameTo(gameDir)) {
            trace("install: recuperada una extraccion completa de game.staging")
            previous.deleteRecursively()
        } else if (looksLikeGame(previous) && previous.renameTo(gameDir)) {
            trace("install: restaurado el juego anterior desde game.old")
        } else {
            staging.deleteRecursively()
            previous.deleteRecursively()
        }
    }

    private fun looksLikeGame(dir: File): Boolean {
        if (!dir.isDirectory || findExe(dir) == null) return false
        val data = File(dir, "data")
        return data.isDirectory && data.listFiles()?.isNotEmpty() == true
    }

    private fun findExe(dir: File): File? =
        dir.listFiles()?.firstOrNull { it.isFile && it.extension.equals("exe", ignoreCase = true) }

    /**
     * What a "save" actually is: the saved games plus the teams and custom characters the game's own
     * migration instructions tell you to carry over. Keeping the same folder names means a zip made
     * here also works by hand on the PC copy, and the other way round.
     */
    private val saveFolders = listOf("partidas", "equipos", "char_custom")

    /** One of the game's save slots, described the way its own "Cargar" screen describes it. */
    data class SaveSlot(
        val index: Int,
        val name: String?,
        val seen: String?,
        val caught: String?,
        val place: String?,
    )

    object Slots {
        /** How many slots the game's load screen offers. */
        const val COUNT = 3
    }

    /**
     * The slots the game has, read from the little summary file it writes beside each save.
     *
     * `reloaded<N>.nfo` is plain text with one field per line, in the order the game writes it: the
     * trainer's name, how many Pokemon have been seen and caught, where the save was made, the
     * character's sprite and the Pokemon beside them. Reading that file is what lets the app show
     * exactly what the game's own load screen shows, without guessing at anything.
     *
     * A slot counts as used when any `reloaded<N>.*` file exists for it: the save itself is the
     * `.savc`, while the `.data` and `.nfo` only describe it.
     */
    fun saveSlots(): List<SaveSlot> {
        val folder = File(gameDir, "data/partidas")
        val pattern = Regex("^reloaded(\\d+)\\.[a-z]+$", RegexOption.IGNORE_CASE)
        val used = folder.listFiles().orEmpty()
            .mapNotNull { pattern.find(it.name)?.groupValues?.get(1)?.toIntOrNull() }
            .toSet()

        return (1..maxOf(Slots.COUNT, used.maxOrNull() ?: 0)).map { index ->
            if (index !in used) {
                return@map SaveSlot(index, null, null, null, null)
            }
            val lines = runCatching { File(folder, "reloaded$index.nfo").readLines() }.getOrDefault(emptyList())
                // The game leaves a blank line after the trainer's name, and counting it as a field
                // shifted every value by one: the seen count came out empty and the place came out
                // as the number of captured.
                .filter { it.isNotBlank() }
            fun field(at: Int) = lines.getOrNull(at)?.trim()?.takeIf { it.isNotEmpty() }
            SaveSlot(index, field(0), field(1), field(2), field(3))
        }
    }

    /**
     * Restores one slot from a zip, into the slot that was asked for.
     *
     * The files inside an exported slot carry their own number - `reloaded1.savc` and the ones that
     * describe it - so importing them as they are would always put them back where they came from.
     * They are renamed on the way in instead, which is what turns "export from slot 1, import into
     * slot 3" into moving a save. The suffix is kept, so a backup stays a backup.
     */
    suspend fun importSlot(uri: Uri, index: Int): Result<Int> = withContext(Dispatchers.IO) {
        runCatching {
            val folder = File(gameDir, "data/partidas")
            if (!folder.isDirectory) error("Primero instala el juego.")
            val numbered = Regex("^reloaded\\d+(\\.|_)(.*)$", RegexOption.IGNORE_CASE)
            var written = 0
            context.contentResolver.openInputStream(uri)?.use { input ->
                ZipInputStream(input.buffered()).use { zip ->
                    var entry = zip.nextEntry
                    while (entry != null) {
                        val file = entry.name.substringAfterLast('/')
                        val match = numbered.find(file)
                        // Only a slot's own files, and only from the saves folder.
                        if (!entry.isDirectory && entry.name.startsWith("partidas/") && match != null) {
                            val separator = match.groupValues[1]
                            val rest = match.groupValues[2]
                            val target = File(folder, "reloaded$index$separator$rest")
                            target.outputStream().use { zip.copyTo(it) }
                            written++
                        }
                        zip.closeEntry()
                        entry = zip.nextEntry
                    }
                }
            }
            if (written == 0) error("Ese zip no trae ninguna partida.")
            written
        }
    }

    /**
     * Deletes a slot: the save itself, the files that describe it, and the backup the game keeps
     * beside it. The backup goes too, because leaving it behind would let the game offer to restore
     * the very save that was meant to be gone.
     */
    suspend fun deleteSlot(index: Int): Result<Unit> = withContext(Dispatchers.IO) {
        runCatching {
            val folder = File(gameDir, "data/partidas")
            val files = folder.listFiles().orEmpty()
                .filter { it.name.startsWith("reloaded$index.") || it.name.startsWith("reloaded${index}_") }
            if (files.isEmpty()) error("Esa partida ya no esta.")
            files.forEach { it.delete() }
        }
    }

    /** Zips a single slot, so one save can be carried on its own. */
    suspend fun exportSlot(index: Int): Result<File> = withContext(Dispatchers.IO) {
        runCatching {
            val folder = File(gameDir, "data/partidas")
            val files = folder.listFiles().orEmpty().filter { it.name.startsWith("reloaded$index.") }
            if (files.isEmpty()) error("Esa partida esta vacia.")
            val out = File(context.cacheDir, "partida$index.zip")
            ZipOutputStream(out.outputStream().buffered()).use { zip ->
                files.forEach { file ->
                    zip.putNextEntry(ZipEntry("partidas/" + file.name))
                    file.inputStream().use { it.copyTo(zip) }
                    zip.closeEntry()
                }
            }
            out
        }
    }

    /** Zips the saves into the cache, ready to be handed to a file picker. */
    suspend fun exportSaves(): Result<File> = withContext(Dispatchers.IO) {
        runCatching {
            val data = File(gameDir, "data")
            val out = File(context.cacheDir, "partidas.zip")
            var written = 0
            ZipOutputStream(out.outputStream().buffered()).use { zip ->
                saveFolders.forEach { folder ->
                    val root = File(data, folder)
                    if (!root.isDirectory) return@forEach
                    root.walkTopDown().filter { it.isFile }.forEach { file ->
                        zip.putNextEntry(ZipEntry("$folder/" + file.relativeTo(root).invariantSeparatorsPath))
                        file.inputStream().use { it.copyTo(zip) }
                        zip.closeEntry()
                        written++
                    }
                }
            }
            if (written == 0) {
                out.delete()
                error("No hay partidas que exportar todavia.")
            }
            out
        }
    }

    /** Restores an exported zip into the game's data folder, replacing what is there. */
    suspend fun importSaves(uri: Uri): Result<Int> = withContext(Dispatchers.IO) {
        runCatching {
            val data = File(gameDir, "data")
            if (!data.isDirectory) error("Primero instala el juego.")
            var files = 0
            // A save is a .savc file; the .data and .nfo beside it only describe it. Counting every
            // file in the zip said "51 partidas" when there was really one.
            var saves = 0
            context.contentResolver.openInputStream(uri)?.use { input ->
                ZipInputStream(input.buffered()).use { zip ->
                    var entry = zip.nextEntry
                    while (entry != null) {
                        val name = entry.name
                        val target = File(data, name)
                        // Only the save folders, and never a path that escapes them.
                        val wanted = saveFolders.any { name.startsWith("$it/") }
                        val safe = target.canonicalPath.startsWith(data.canonicalPath + File.separator)
                        if (!entry.isDirectory && wanted && safe) {
                            target.parentFile?.mkdirs()
                            target.outputStream().use { zip.copyTo(it) }
                            files++
                            // Backups (`reloaded1_bk.savc`) sit beside the real slot, so they are not
                            // counted as separate saves.
                            val isSlot = name.startsWith("partidas/") &&
                                name.endsWith(".savc", ignoreCase = true) &&
                                !name.contains("_bk", ignoreCase = true)
                            if (isSlot) saves++
                        }
                        zip.closeEntry()
                        entry = zip.nextEntry
                    }
                }
            } ?: error("No pude abrir el archivo.")
            if (files == 0) error("Ese zip no trae partidas del juego.")
            saves
        }
    }

    private fun readMeta(): JSONObject? = runCatching { JSONObject(metaFile.readText()) }.getOrNull()

    /**
     * Copies the bundled MIDI soundfont next to the game, which is where the emulator looks for it.
     * It ships inside the APK so music works on any device without depending on external paths.
     */

    /** Copies the mod manifests that ship with the app once, without touching user edits. */
    private fun installBundledMods() {
        val modsDir = File(gameDir, "mods")
        for (id in listOf("gb", "crt")) {
            val target = File(modsDir, "$id/mod.ini")
            if (target.isFile) continue
            runCatching {
                target.parentFile?.mkdirs()
                context.assets.open("mods/$id/mod.ini").use { input ->
                    target.outputStream().use { output -> input.copyTo(output) }
                }
            }
        }
        // Mods are opt-in from 0.4.0. Record that default once so an old install which never
        // touched the master switch does not keep loading them after the update. If the user
        // already chose, leave their enabled.flag/disabled.flag alone.
        val enabledFlag = File(modsDir, "enabled.flag")
        val disabledFlag = File(modsDir, "disabled.flag")
        if (!enabledFlag.isFile && !disabledFlag.isFile) {
            runCatching {
                modsDir.mkdirs()
                disabledFlag.writeText("1")
            }
        }
    }

    suspend fun prepare(): Unit = withContext(Dispatchers.IO) {
        recoverInterruptedInstall()
        installBundledMods()
        val exe = findExe(gameDir)
        if (exe != null && File(gameDir, "data").isDirectory) {
            // Installs made before the launcher recorded these still need their size and date.
            val meta = readMeta() ?: JSONObject()
            if (meta.optLong("sizeBytes", 0L) == 0L || meta.optLong("installedAt", 0L) == 0L) {
                meta.put("sizeBytes", gameDir.walkTopDown().filter { it.isFile }.sumOf { it.length() })
                if (meta.optLong("installedAt", 0L) == 0L) meta.put("installedAt", System.currentTimeMillis())
                metaFile.writeText(meta.toString())
            }
        }
        // The touch controls' pixel-art artwork ships inside the APK, but the engine can only read
        // files from its own folder, so every sprite is copied there (a few hundred bytes each,
        // refreshed on every start so an app update takes effect). Any `<control>.png` dropped in
        // the assets is picked up.
        val sprites = runCatching { context.assets.list("")?.filter { it.endsWith(".png", ignoreCase = true) } }
            .getOrNull()
            .orEmpty()
        for (name in sprites) {
            runCatching {
                context.assets.open(name).use { input ->
                    File(files, name).outputStream().use { output -> input.copyTo(output) }
                }
            }
        }
        if (!gameDir.isDirectory) return@withContext
        val target = File(gameDir, "soundfont.sf2")
        if (target.isFile && target.length() > 0) return@withContext
        runCatching {
            context.assets.open("soundfont.sf2").use { input ->
                target.outputStream().use { output -> input.copyTo(output) }
            }
        }
        Unit
    }

    /** Imports a zip picked with the system file chooser. */
    suspend fun import(
        uri: Uri,
        onProgress: (Int) -> Unit,
        onLog: (String) -> Unit = {},
    ): Result<Unit> = withContext(Dispatchers.IO) {
        runCatching {
            val log = logger(onLog)
            val started = SystemClock.elapsedRealtime()
            val temp = File(files, "import.zip")
            var stalled = false
            try {
                log("Comprobando el almacenamiento antes de copiar…")
                preflightStorage(log)
                copyFromUri(uri, temp, onProgress, log)
                log("Zip listo: ${formatBytes(temp.length())}")
                installZip(temp, version = null, source = "manual", onProgress = onProgress, log = log)
                prepare()
                log("Instalación terminada en ${seconds(started)}.")
                onProgress(100)
            } catch (e: Throwable) {
                stalled = e is StorageStalled
                log("Error: ${messageOf(e)}")
                throw e
            } finally {
                // Touching the card while it is stalled would hang this coroutine again.
                if (!stalled) temp.delete()
            }
        }
    }

    /**
     * Copies the chosen zip into `dest`, tolerating the flaky streams some pickers hand out.
     *
     * Reading a big document through `contentResolver.openInputStream()` is not reliable on every
     * device: the stream can end early (`unexpected EOF`) or come back empty, especially on devices
     * with little memory and the game on slow/adopted storage. So the size the picker reports is
     * used as a reference, the file is verified after the copy, and the whole thing is retried
     * before giving up.
     */
    private fun copyFromUri(uri: Uri, dest: File, onProgress: (Int) -> Unit, log: (String) -> Unit) {
        val expected = contentResolverSize(uri)
        if (expected > 0) {
            val free = files.usableSpace
            log("Espacio libre: ${formatBytes(free)}; el zip ocupa ${formatBytes(expected)}.")
            if (free < expected + 32L * 1024 * 1024) {
                error("No hay espacio para copiar el zip: quedan ${formatBytes(free)} y necesita ${formatBytes(expected)}.")
            }
        }
        val reporter = InstallReporter(onProgress, log) { percent -> percent * COPY_SHARE / 100 }
        var last: Throwable? = null
        for (attempt in 1..3) {
            try {
                dest.delete()
                log("Leyendo el archivo elegido…")
                context.contentResolver.openInputStream(uri)?.use { input ->
                    dest.outputStream().buffered(BUFFER).use { output ->
                        val buffer = ByteArray(BUFFER)
                        var copied = 0L
                        while (true) {
                            val read = input.read(buffer)
                            if (read < 0) break
                            output.write(buffer, 0, read)
                            copied += read
                            if (expected > 0) {
                                reporter.progress(
                                    ((copied * 100) / expected).toInt().coerceIn(0, 99),
                                    "Copiando…",
                                )
                            }
                        }
                    }
                } ?: error("No pude abrir el fichero elegido.")
                val size = dest.length()
                if (size == 0L) error("La copia vino vacía.")
                if (expected > 0 && size != expected) {
                    error("Copia incompleta: $size de $expected bytes.")
                }
                reporter.progress(100, "Copiando…")
                return
            } catch (e: Throwable) {
                last = e
                log("Intento $attempt de copiar falló: ${messageOf(e)}")
            }
        }
        throw last ?: error("No pude copiar el zip.")
    }

    /** The size the content provider advertises for a document, or -1 if it does not say. */
    private fun contentResolverSize(uri: Uri): Long = runCatching {
        context.contentResolver.query(uri, arrayOf(OpenableColumns.SIZE), null, null, null)?.use { cursor ->
            if (cursor.moveToFirst() && !cursor.isNull(0)) cursor.getLong(0) else -1L
        } ?: -1L
    }.getOrDefault(-1L)

    /** Downloads a zip from a direct URL (following redirects) and installs it. */
    suspend fun download(
        url: String,
        cookies: String?,
        onProgress: (Int) -> Unit,
        onLog: (String) -> Unit = {},
    ): Result<Unit> = withContext(Dispatchers.IO) {
        runCatching {
            val log = logger(onLog)
            val started = SystemClock.elapsedRealtime()
            val temp = File(files, "download.zip")
            var stalled = false
            try {
                log("Comprobando el almacenamiento antes de descargar…")
                preflightStorage(log)
                log("Conectando con el servidor…")
                val connection = (URL(url).openConnection() as HttpURLConnection).apply {
                    instanceFollowRedirects = true
                    connectTimeout = 30_000
                    readTimeout = 30_000
                    setRequestProperty("User-Agent", "Mozilla/5.0 (Android) ReloadedLauncher/1.0")
                    if (!cookies.isNullOrEmpty()) setRequestProperty("Cookie", cookies)
                }
                connection.connect()
                if (connection.responseCode !in 200..299) error("El servidor respondió ${connection.responseCode}.")
                val total = connection.contentLengthLong.takeIf { it > 0 } ?: -1L
                log(if (total > 0) "Descargando ${formatBytes(total)}…" else "Descargando…")
                val reporter = InstallReporter(onProgress, log) { percent -> percent * COPY_SHARE / 100 }
                connection.inputStream.use { input ->
                    temp.outputStream().buffered(BUFFER).use { output ->
                        val buffer = ByteArray(BUFFER)
                        var written = 0L
                        while (true) {
                            val read = input.read(buffer)
                            if (read < 0) break
                            output.write(buffer, 0, read)
                            written += read
                            if (total > 0) {
                                reporter.progress((written * 100 / total).toInt().coerceIn(0, 99), "Descargando…")
                            }
                        }
                    }
                }
                reporter.progress(100, "Descargando…")
                installZip(temp, version = null, source = "descarga", onProgress = onProgress, log = log)
                prepare()
                log("Instalación terminada en ${seconds(started)}.")
                onProgress(100)
            } catch (e: Throwable) {
                stalled = e is StorageStalled
                log("Error: ${messageOf(e)}")
                throw e
            } finally {
                if (!stalled) temp.delete()
            }
        }
    }

    /**
     * Validates, inflates and swaps in a game zip.
     *
     * The order is what keeps a failed import from costing the installed game: everything that can
     * be checked is checked first (a second or two), the archive is unpacked beside the current
     * install rather than over it, and only a complete `game.staging` is renamed into place. If that
     * last move fails, `game.old` is renamed back.
     */
    private fun installZip(
        zip: File,
        version: String?,
        source: String,
        onProgress: (Int) -> Unit,
        log: (String) -> Unit,
    ) {
        log("Comprobando el zip…")
        foldZipNamesToAscii(zip)
        val plan = planZip(zip, log)
        log("Zip válido: ${plan.items.count { !it.directory }} archivos, ${formatBytes(plan.totalBytes)} sin comprimir.")

        // A previous failed attempt can leave a lot in staging, so clear it before measuring the
        // free space it would otherwise hide.
        val staging = File(files, "game.staging")
        if (staging.exists()) {
            log("Limpiando una extracción anterior…")
            if (!staging.deleteRecursively()) error("No pude limpiar ${staging.absolutePath}.")
        }

        // The saves are copied aside during the swap, so their size also has to fit.
        val savesBytes = saveFolders.sumOf { folder ->
            val dir = File(gameDir, "data/$folder")
            if (!dir.isDirectory) 0L else dir.walkTopDown().filter { it.isFile }.sumOf { it.length() }
        }
        val free = files.usableSpace
        val needed = plan.totalBytes + savesBytes + 32L * 1024 * 1024
        log("Espacio libre: ${formatBytes(free)}; hacen falta ${formatBytes(needed)} (incluye partidas).")
        if (plan.totalBytes > 0 && free < needed) {
            error("No hay espacio suficiente: hacen falta ${formatBytes(needed)} y quedan ${formatBytes(free)} libres.")
        }

        if (!staging.mkdirs() && !staging.isDirectory) {
            error("No pude crear ${staging.absolutePath}.")
        }

        val saved = File(files, "game.saves")
        try {
            log("Extrayendo en ${staging.absolutePath}…")
            extractZip(zip, plan, staging, onProgress, log)
            onProgress(EXTRACT_END)

            log("Verificando la extracción…")
            if (!looksLikeGame(staging)) {
                error("El zip no trae el .exe y la carpeta data/ en la raíz.")
            }
            log("Verificación correcta.")

            log("Conservando partidas y datos del jugador…")
            if (!saved.deleteRecursively() && saved.exists()) {
                error("No pude preparar ${saved.absolutePath}.")
            }
            var kept = 0
            if (gameDir.isDirectory) {
                for (folder in saveFolders) {
                    val from = File(gameDir, "data/$folder")
                    if (!from.isDirectory) continue
                    val to = File(saved, folder)
                    runCatching { from.copyRecursively(to, overwrite = true) }
                    if (to.isDirectory) kept += to.walkTopDown().count { it.isFile }
                }
            }
            log("Partidas conservadas: $kept archivos.")
            onProgress(95)

            log("Instalando (reemplazo atómico)…")
            val previous = File(files, "game.old")
            if (!previous.deleteRecursively() && previous.exists()) {
                error("No pude limpiar ${previous.absolutePath}; no he tocado el juego actual.")
            }
            val hadGame = gameDir.isDirectory
            if (hadGame && !gameDir.renameTo(previous)) {
                error("No pude apartar el juego anterior (${gameDir.absolutePath}); no se ha modificado nada.")
            }
            if (!staging.renameTo(gameDir)) {
                val restored = if (!hadGame) {
                    true
                } else {
                    gameDir.deleteRecursively()
                    previous.renameTo(gameDir)
                }
                staging.deleteRecursively()
                saved.deleteRecursively()
                error(
                    if (restored) "No pude instalar el juego nuevo; se ha restaurado el anterior."
                    else "No pude instalar el juego nuevo ni restaurar el anterior. Reinicia la app.",
                )
            }
            if (hadGame && !previous.deleteRecursively() && previous.exists()) {
                log("Aviso: no pude borrar ${previous.absolutePath}; se limpiará en el próximo arranque.")
            }
            onProgress(98)

            log("Restaurando partidas…")
            for (folder in saveFolders) {
                val from = File(saved, folder)
                if (!from.isDirectory) continue
                val to = File(gameDir, "data/$folder")
                to.mkdirs()
                runCatching { from.copyRecursively(to, overwrite = false) }
            }
            saved.deleteRecursively()

            runCatching {
                metaFile.writeText(
                    JSONObject()
                        .put("version", version ?: "")
                        .put("source", source)
                        .put("installedAt", System.currentTimeMillis())
                        .put("sizeBytes", plan.totalBytes)
                        .toString(),
                )
            }.onFailure { log("Aviso: no pude escribir ${metaFile.name}: ${messageOf(it)}") }

            onProgress(100)
            log("Juego instalado en ${gameDir.absolutePath} (${formatBytes(plan.totalBytes)}).")
        } catch (e: Throwable) {
            if (e is StorageStalled) {
                log("El almacenamiento no responde; dejo ${staging.name} para limpiarlo al reiniciar.")
            } else {
                staging.deleteRecursively()
                saved.deleteRecursively()
            }
            throw e
        }
    }

    /**
     * What the zip contains, worked out from its central directory before anything is written.
     *
     * A zip made by right-clicking the game folder keeps it one level down (`Pokemon Reloaded/…`),
     * so the common root of the `.exe` and `data/` is found and its prefix stripped during
     * extraction. That also skips `__MACOSX` and other top-level junk, and gives the exact
     * uncompressed size used for the free-space check and the progress bar.
     */
    private fun planZip(file: File, log: (String) -> Unit): ZipPlan {
        if (!file.isFile || file.length() == 0L) error("El archivo del juego está vacío.")
        val items = ArrayList<ZipItem>()
        ZipFile(file).use { zip ->
            val entries = zip.entries()
            while (entries.hasMoreElements()) {
                val entry = entries.nextElement()
                val path = normalizeZipName(entry.name)
                if (path.isEmpty() || isJunk(path)) continue
                items += ZipItem(
                    raw = entry.name,
                    path = path,
                    directory = entry.isDirectory || entry.name.endsWith("/") || entry.name.endsWith("\\"),
                    size = entry.size,
                )
            }
        }
        if (items.isEmpty()) error("El zip está vacío.")

        val names = items.map { it.path }
        val roots = LinkedHashSet<String>()
        roots += ""
        for (name in names) {
            val slash = name.indexOf('/')
            if (slash > 0) roots += name.substring(0, slash + 1)
        }
        val root = roots.firstOrNull { prefix ->
            val hasExe = names.any { name ->
                name.startsWith(prefix, ignoreCase = true) && !name.endsWith("/") &&
                    name.substring(prefix.length).let { rest ->
                        rest.isNotEmpty() && '/' !in rest && rest.endsWith(".exe", ignoreCase = true)
                    }
            }
            val hasData = names.any { name ->
                name.startsWith(prefix + "data/") && name.length > prefix.length + "data/".length
            }
            hasExe && hasData
        } ?: error(
            "Ese zip no parece Pokémon Reloaded: no encuentro el .exe y la carpeta data/ " +
                "en la misma carpeta. Empieza por: ${names.take(5).joinToString(" | ")}",
        )

        val relative = LinkedHashMap<String, ZipItem>()
        for (item in items) {
            if (!item.path.startsWith(root)) continue
            val path = item.path.substring(root.length)
            if (path.isEmpty()) continue
            if (relative.put(path, item.copy(path = path)) != null) {
                error("El zip trae el mismo archivo dos veces: $path. Vuelve a descargarlo.")
            }
        }
        if (relative.isEmpty()) error("El zip no tiene archivos que instalar.")
        val totalBytes = relative.values.sumOf { if (!it.directory && it.size > 0) it.size else 0L }
        log("Estructura: raíz \"$root\", ${relative.size} entradas.")
        return ZipPlan(relative.values.toList(), totalBytes)
    }

    /**
     * Unpacks [plan] into [staging] using one [ZipFile] handle per worker.
     *
     * `ZipInputStream` walks the archive from beginning to end and skips unknown entries, while
     * `ZipFile` reads straight from the central directory. The work is split between a few threads
     * (deflate is CPU-bound, so this is what makes a 20,000-file game take seconds instead of a
     * minute on a mult-core phone). Progress is reported at most every 250 ms / 1 %, and failures
     * from any worker abort the whole extraction.
     */
    private fun extractZip(
        zip: File,
        plan: ZipPlan,
        staging: File,
        onProgress: (Int) -> Unit,
        log: (String) -> Unit,
    ) {
        val items = plan.items
        val total = plan.totalBytes.coerceAtLeast(1)
        val written = AtomicLong(0)
        val reporter = InstallReporter(onProgress, log) { percent ->
            EXTRACT_START + percent * (EXTRACT_END - EXTRACT_START) / 100
        }
        // Adopted storage (a microSD formatted as internal) goes through a FUSE layer that can stall
        // or deadlock under parallel writes: four workers left every extraction thread in the kernel
        // at 69 % on a HiBreak. One writer is slower but actually finishes. Internal flash still gets
        // the parallel path.
        val adopted = files.absolutePath.startsWith("/mnt/expand/")
        val workers = when {
            items.size < 200 || adopted -> 1
            else -> Runtime.getRuntime().availableProcessors().coerceIn(1, 4)
        }
        log(
            "Extrayendo ${items.count { !it.directory }} archivos (${formatBytes(plan.totalBytes)}) " +
                "con $workers ${if (workers == 1) "hilo" else "hilos"}…" +
                if (adopted) " (almacenamiento adoptado, sin escrituras en paralelo)" else "",
        )
        val pool = Executors.newFixedThreadPool(workers) { runnable ->
            Thread(runnable, "install-zip").apply { isDaemon = true }
        }
        try {
            val chunks = List(workers) { worker -> items.filterIndexed { index, _ -> index % workers == worker } }
            val futures = chunks.filter { it.isNotEmpty() }.map { chunk ->
                pool.submit(Callable {
                    ZipFile(zip).use { archive ->
                        val buffer = ByteArray(BUFFER)
                        for (item in chunk) {
                            val target = File(staging, item.path)
                            if (item.directory) {
                                target.mkdirs()
                                continue
                            }
                            val entry = archive.getEntry(item.raw)
                                ?: throw IOException("No encuentro ${item.raw} dentro del zip.")
                            target.parentFile?.mkdirs()
                            target.outputStream().buffered(buffer.size).use { output ->
                                archive.getInputStream(entry).use { input ->
                                    while (true) {
                                        val read = input.read(buffer)
                                        if (read < 0) break
                                        output.write(buffer, 0, read)
                                        // Count as the bytes land, not when the file is finished: a
                                        // single big entry must keep the watchdog and the bar alive.
                                        written.addAndGet(read.toLong())
                                        reporter.progress(
                                            ((written.get() * 100) / total).toInt().coerceIn(0, 100),
                                            "Extrayendo…",
                                        )
                                    }
                                }
                            }
                            if (item.size >= 0 && target.length() != item.size) {
                                throw IOException(
                                    "${item.path} salió incompleto (${target.length()} de ${item.size} bytes).",
                                )
                            }
                        }
                    }
                })
            }
            // Wait without blocking blindly: if the card stops answering, say so and eventually
            // abort instead of leaving the install (and the user) waiting forever.
            var lastBytes = 0L
            var lastChangeAt = SystemClock.elapsedRealtime()
            var lastWarningAt = 0L
            while (futures.any { !it.isDone }) {
                val now = SystemClock.elapsedRealtime()
                val bytes = written.get()
                if (bytes != lastBytes) {
                    lastBytes = bytes
                    lastChangeAt = now
                }
                val idle = now - lastChangeAt
                if (idle >= 15_000 && now - lastWarningAt >= 15_000) {
                    lastWarningAt = now
                    log("El almacenamiento no responde; sigo esperando… (${idle / 1000} s sin escribir)")
                }
                if (idle >= 90_000) {
                    futures.forEach { it.cancel(true) }
                    throw StorageStalled(
                        "El almacenamiento adoptado (tarjeta SD) dejó de responder durante la extracción. " +
                            "Reinicia el móvil y vuelve a intentarlo.",
                    )
                }
                Thread.sleep(200)
            }
            try {
                futures.forEach { it.get() }
            } catch (e: ExecutionException) {
                futures.forEach { it.cancel(true) }
                throw (e.cause ?: e)
            } catch (e: InterruptedException) {
                futures.forEach { it.cancel(true) }
                Thread.currentThread().interrupt()
                throw e
            }
        } finally {
            pool.shutdownNow()
            runCatching { pool.awaitTermination(2, TimeUnit.SECONDS) }
        }
        reporter.progress(100, "Extrayendo…")
    }

    /**
     * Fails in a second, with the exact path, when the app's storage cannot be written.
     *
     * This is the check that would have caught the engine's old `/data/data/.../files/game`
     * permission error before copying a 300 MB zip, and the message names the directory so a bug
     * report says what is wrong instead of only "falló".
     */
    private fun preflightStorage(log: (String) -> Unit) {
        val root = files
        log("Comprobando ${root.absolutePath}…")
        if (!root.isDirectory && !root.mkdirs()) {
            error("No existe ${root.absolutePath} y no pude crearla.")
        }
        checkWritable(root, "carpeta de datos de la app")
        val game = gameDir
        if (game.exists() && !game.isDirectory) {
            error("${game.absolutePath} existe pero no es una carpeta. Bórrala o reinstala la app.")
        }
        if (!game.isDirectory && !game.mkdirs() && !game.isDirectory) {
            error("No pude crear ${game.absolutePath}. Comprueba el espacio libre.")
        }
        checkWritable(game, "carpeta del juego")
        log("Escritura correcta en ${game.absolutePath}.")
        log("Espacio libre: ${formatBytes(root.usableSpace)}.")
    }

    private fun checkWritable(dir: File, what: String) {
        val probe = File(dir, ".reloaded-write-test")
        try {
            probe.outputStream().use { output ->
                output.write(ByteArray(16 * 1024))
                output.flush()
            }
        } catch (e: Throwable) {
            throw IllegalStateException(
                "No puedo escribir en ${dir.absolutePath} ($what): ${messageOf(e)}. " +
                    "Revisa el almacenamiento o reinstala la app.",
                e,
            )
        } finally {
            probe.delete()
        }
        if (probe.exists()) error("Puedo escribir en ${dir.absolutePath} pero no borrar la prueba.")
    }

    private fun normalizeZipName(raw: String): String {
        val forward = raw.replace('\\', '/')
        if (forward.indexOf('\u0000') >= 0) error("El zip trae un nombre no válido.")
        if (forward.startsWith("/") || Regex("^[A-Za-z]:").containsMatchIn(forward)) {
            error("El zip trae una ruta absoluta no válida: $raw")
        }
        val parts = ArrayList<String>()
        for (part in forward.split('/')) {
            when (part) {
                "", "." -> {}
                ".." -> error("El zip trae una ruta que se sale de la carpeta: $raw")
                else -> parts += part
            }
        }
        return parts.joinToString("/")
    }

    private fun isJunk(path: String): Boolean =
        path == "__MACOSX" || path.startsWith("__MACOSX/") ||
            path == ".DS_Store" || path.endsWith("/.DS_Store") ||
            path == "Thumbs.db" || path.endsWith("/Thumbs.db")

    private fun logger(onLog: (String) -> Unit): (String) -> Unit = { message ->
        // The UI comes first: writing app.log must never delay (or block) what the console shows.
        runCatching { onLog(message) }
        trace(message)
    }

    private fun messageOf(e: Throwable): String =
        e.message?.takeIf { it.isNotBlank() } ?: e::class.java.simpleName

    private fun formatBytes(bytes: Long): String = when {
        bytes >= 1_000_000_000L -> String.format(Locale.US, "%.1f GB", bytes / 1_000_000_000.0)
        bytes >= 1_000_000L -> String.format(Locale.US, "%.1f MB", bytes / 1_000_000.0)
        bytes >= 1_000L -> String.format(Locale.US, "%.0f KB", bytes / 1_000.0)
        else -> "$bytes B"
    }

    private fun seconds(startedAt: Long): String =
        String.format(Locale.US, "%.1f s", (SystemClock.elapsedRealtime() - startedAt) / 1000.0)

    private data class ZipItem(
        val raw: String,
        val path: String,
        val directory: Boolean,
        val size: Long,
    )

    private data class ZipPlan(
        val items: List<ZipItem>,
        val totalBytes: Long,
    )

    /**
     * Writes one line to logcat, which is what the log screen reads, and also to a small file.
     *
     * logcat only holds what the running process printed, so anything logged before the phone was
     * restarted is gone - which is exactly when somebody sits down to report a bug. The file keeps
     * the last stretch, and is rolled over so it cannot grow without end.
     */
    private fun trace(message: String) {
        android.util.Log.i("Reloaded", message)
        logWriter.execute {
            runCatching {
                val file = File(files, "app.log")
                if (file.length() > 256 * 1024) {
                    File(files, "app.log.1").delete()
                    file.renameTo(File(files, "app.log.1"))
                }
                file.appendText("$message\n")
            }
        }
    }

    /**
     * Folds the entry names that are not valid UTF-8 down to plain ASCII, in place and without
     * changing a single length.
     *
     * The official game zip was made with a DOS code page, so two of its names carry an accent as a
     * single byte - `0xA1` for the "i" of "Especificas", `0xA2` for the "o" of "Informacion". The
     * zip does not set the UTF-8 flag, and Android's zip reader refuses the *whole* archive when it
     * meets a name that is not valid UTF-8, which is why importing the game failed for everybody.
     * Rewriting one byte for one byte leaves every offset and length in the file untouched, so the
     * archive stays perfectly valid.
     */
    private fun foldZipNamesToAscii(file: File) {
        runCatching {
            RandomAccessFile(file, "rw").use { raf ->
                val eocd = findEndOfCentralDirectory(raf) ?: return
                raf.seek(eocd + 10)
                val count = readLe16(raf)
                val cdSize = readLe32(raf)
                val cdOffset = readLe32(raf)
                if (cdSize <= 0L || cdSize > 64L * 1024 * 1024) return

                val cd = ByteArray(cdSize.toInt())
                raf.seek(cdOffset)
                raf.readFully(cd)

                var p = 0
                repeat(count) {
                    if (p + 46 > cd.size) return
                    if (cd[p] != 'P'.code.toByte() || cd[p + 1] != 'K'.code.toByte() ||
                        cd[p + 2] != 1.toByte() || cd[p + 3] != 2.toByte()
                    ) {
                        return
                    }
                    val nameLen = le16(cd, p + 28)
                    val extraLen = le16(cd, p + 30)
                    val commentLen = le16(cd, p + 32)
                    val localOffset = le32(cd, p + 42)

                    var changed = false
                    // A name that is already valid UTF-8 is read correctly by Android's zip reader
                    // and is left alone: folding its two-byte accents would mangle it, while the
                    // fold exists only for archives whose names are raw DOS code page bytes.
                    if (!isValidUtf8(cd, p + 46, nameLen)) {
                        for (i in 0 until nameLen) {
                            val at = p + 46 + i
                            val byte = cd[at].toInt() and 0xff
                            if (byte > 0x7f) {
                                cd[at] = foldToAscii(byte)
                                changed = true
                            }
                        }
                    }

                    if (changed) {
                        trace("import: nombre arreglado: ${asciiName(cd, p + 46, nameLen)}")
                        raf.seek(cdOffset + p + 46)
                        raf.write(cd, p + 46, nameLen)

                        // The local header repeats the name, and readers may use either copy.
                        raf.seek(localOffset + 26)
                        val localNameLen = readLe16(raf)
                        raf.seek(localOffset + 30)
                        raf.write(cd, p + 46, minOf(nameLen, localNameLen))
                    }
                    p += 46 + nameLen + extraLen + commentLen
                }
            }
        }
    }

    private fun asciiName(bytes: ByteArray, at: Int, length: Int) =
        String(bytes, at, length, Charsets.ISO_8859_1)

    private fun isValidUtf8(bytes: ByteArray, at: Int, length: Int): Boolean = runCatching {
        Charsets.UTF_8.newDecoder()
            .onMalformedInput(CodingErrorAction.REPORT)
            .onUnmappableCharacter(CodingErrorAction.REPORT)
            .decode(ByteBuffer.wrap(bytes, at, length))
    }.isSuccess

    private fun findEndOfCentralDirectory(raf: RandomAccessFile): Long? {
        val window = minOf(raf.length(), 64L * 1024).toInt()
        if (window < 4) return null
        val tail = ByteArray(window)
        raf.seek(raf.length() - window)
        raf.readFully(tail)
        for (i in window - 4 downTo 0) {
            if (tail[i] == 'P'.code.toByte() && tail[i + 1] == 'K'.code.toByte() &&
                tail[i + 2] == 5.toByte() && tail[i + 3] == 6.toByte()
            ) {
                return raf.length() - window + i
            }
        }
        return null
    }

    private fun le16(bytes: ByteArray, at: Int) =
        (bytes[at].toInt() and 0xff) or ((bytes[at + 1].toInt() and 0xff) shl 8)

    private fun le32(bytes: ByteArray, at: Int) = le16(bytes, at).toLong() or ((le16(bytes, at + 2)).toLong() shl 16)

    private fun readLe16(raf: RandomAccessFile) = le16(ByteArray(2).also { raf.readFully(it) }, 0)

    private fun readLe32(raf: RandomAccessFile) = le32(ByteArray(4).also { raf.readFully(it) }, 0)

    /** The DOS code page byte for a Spanish accent, folded to the plain letter it stands for. */
    private fun foldToAscii(byte: Int): Byte = when (byte) {
        0x82, 0x90 -> 'e'.code.toByte()
        0xA0, 0xB5 -> 'a'.code.toByte()
        0xA1, 0xD6 -> 'i'.code.toByte()
        0xA2, 0xE0 -> 'o'.code.toByte()
        0xA3, 0xE9 -> 'u'.code.toByte()
        0xA4, 0xA5 -> 'n'.code.toByte()
        0x81, 0x9A -> 'u'.code.toByte()
        else -> '_'.code.toByte()
    }

    private companion object {
        /** Copy/download takes the bar from 0 to 20; extraction continues from there to 90. */
        const val COPY_SHARE = 20
        const val EXTRACT_START = 20
        const val EXTRACT_END = 90

        /** Big enough that the read/write loop is not the bottleneck on a 20,000-file zip. */
        const val BUFFER = 256 * 1024
    }
}

/** The adopted storage stopped answering mid-install; touching it again would hang as well. */
private class StorageStalled(message: String) : IOException(message)

/**
 * Rate-limits install progress for the UI and the on-screen console.
 *
 * Progress is reported at most every 150 ms and once per percent, so a fast extraction cannot flood
 * Compose with thousands of recompositions, while the console gets the current percent roughly
 * every 800 ms. Safe to call from the extraction workers.
 */
private class InstallReporter(
    private val onProgress: (Int) -> Unit,
    private val log: (String) -> Unit,
    private val overall: (Int) -> Int = { it },
) {
    private var lastPercent = -1
    private var lastAt = 0L
    private var lastLogged = -10
    private var lastLogAt = 0L

    @Synchronized
    fun progress(percent: Int, verb: String) {
        val value = percent.coerceIn(0, 100)
        val now = SystemClock.elapsedRealtime()
        if (value != lastPercent && (value == 100 || now - lastAt >= 150)) {
            lastPercent = value
            lastAt = now
            runCatching { onProgress(overall(value)) }
        }
        if (value == 100) {
            if (lastLogged != 100) {
                lastLogged = 100
                runCatching { log("$verb ${overall(value)}%") }
            }
        } else if (value != lastLogged && now - lastLogAt >= 800) {
            lastLogged = value
            lastLogAt = now
            runCatching { log("$verb ${overall(value)}%") }
        }
    }
}
