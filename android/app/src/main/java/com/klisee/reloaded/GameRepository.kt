package com.klisee.reloaded

import android.content.Context
import android.net.Uri
import android.provider.OpenableColumns
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.json.JSONObject
import java.io.File
import java.io.RandomAccessFile
import java.net.HttpURLConnection
import java.net.URL
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

    /** The emulator's executable name is not pinned, so it is discovered by extension. */
    fun status(): Status {
        val exe = gameDir.listFiles()?.firstOrNull { it.isFile && it.extension.equals("exe", ignoreCase = true) }
            ?: return Status.Missing
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

    /** The first version-looking number in a name, or null if it has none. */
    private fun versionFromName(name: String): String? =
        Regex("""\d+\.\d+(?:\.\d+){1,2}""").find(name)?.value

    /** Frees the extracted game, leaving the launcher ready for another import or download. */
    suspend fun deleteGame() = withContext(Dispatchers.IO) {
        gameDir.deleteRecursively()
        metaFile.delete()
        Unit
    }

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
    suspend fun prepare(): Unit = withContext(Dispatchers.IO) {
        val exe = gameDir.listFiles()?.firstOrNull { it.isFile && it.extension.equals("exe", ignoreCase = true) }
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
    suspend fun import(uri: Uri, onProgress: (Int) -> Unit): Result<Unit> = withContext(Dispatchers.IO) {
        runCatching {
            val temp = File(files, "import.zip")
            try {
                trace("import: uri=$uri")
                copyFromUri(uri, temp, onProgress)
                trace("import: copiado a $temp (${temp.length()} bytes, libre ${files.usableSpace})")
                installZip(temp, version = null, source = "manual", onProgress = onProgress).getOrThrow()
                prepare()
            } catch (e: Throwable) {
                trace("import: FALLO ${e::class.java.simpleName}: ${e.message}")
                throw e
            } finally {
                temp.delete()
            }
        }
    }

    /**
     * Copies the chosen zip into `dest`, tolerating the flaky streams some pickers hand out.
     *
     * Reading a big document through `contentResolver.openInputStream()` is not reliable on every
     * device: the stream can end early (`unexpected EOF`) or come back empty, especially on devices
     * with little memory and the game on slow/adopted storage. That is what made importing the zip
     * fail. So the size the picker reports is used as a reference, the file is verified after the
     * copy, and the whole thing is retried a few times before giving up. Leaving a half-written
     * `import.zip` behind (as before) made the next step read a broken archive.
     */
    private fun copyFromUri(uri: Uri, dest: File, onProgress: (Int) -> Unit) {
        val expected = contentResolverSize(uri)
        var last: Throwable? = null
        for (attempt in 1..3) {
            try {
                dest.delete()
                context.contentResolver.openInputStream(uri)?.use { input ->
                    dest.outputStream().buffered().use { output ->
                        val buffer = ByteArray(256 * 1024)
                        var copied = 0L
                        while (true) {
                            val n = input.read(buffer)
                            if (n < 0) break
                            output.write(buffer, 0, n)
                            copied += n
                            if (expected > 0) onProgress(((copied * 100) / expected).toInt().coerceIn(0, 99))
                        }
                    }
                } ?: error("No pude abrir el fichero elegido.")
                val size = dest.length()
                if (size == 0L) error("La copia vino vacia.")
                if (expected > 0 && size != expected) error("Copia incompleta: $size de $expected bytes.")
                return
            } catch (e: Throwable) {
                last = e
                trace("import: intento $attempt de copiar fallo (${e.message}); reintentando")
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
    suspend fun download(url: String, cookies: String?, onProgress: (Int) -> Unit): Result<Unit> = withContext(Dispatchers.IO) {
        runCatching {
            val temp = File(files, "download.zip")
            try {
                val connection = (URL(url).openConnection() as HttpURLConnection).apply {
                    instanceFollowRedirects = true
                    connectTimeout = 30_000
                    readTimeout = 30_000
                    setRequestProperty("User-Agent", "Mozilla/5.0 (Android) ReloadedLauncher/1.0")
                    if (!cookies.isNullOrEmpty()) setRequestProperty("Cookie", cookies)
                }
                connection.connect()
                if (connection.responseCode !in 200..299) error("El servidor respondio ${connection.responseCode}.")
                val total = connection.contentLengthLong.takeIf { it > 0 } ?: -1L
                connection.inputStream.use { input ->
                    temp.outputStream().use { output ->
                        val buffer = ByteArray(64 * 1024)
                        var written = 0L
                        while (true) {
                            val n = input.read(buffer)
                            if (n < 0) break
                            output.write(buffer, 0, n)
                            written += n
                            if (total > 0) onProgress((written * 100 / total).toInt())
                        }
                    }
                }
                installZip(temp, version = null, source = "descarga", onProgress = onProgress).getOrThrow()
                prepare()
            } finally {
                temp.delete()
            }
        }
    }

    /** Validates, inflates and swaps in a game zip. Mirrors the emulator's own requirements. */
    private fun installZip(zip: File, version: String?, source: String, onProgress: (Int) -> Unit): Result<Unit> = runCatching {
        foldZipNamesToAscii(zip)
        requireGameZip(zip)
        val staging = File(files, "game.staging")
        staging.deleteRecursively()
        staging.mkdirs()

        val total = zip.length().coerceAtLeast(1)
        trace("import: extrayendo ${zip.length()} bytes de ${zip.name}...")
        ZipInputStream(zip.inputStream().buffered()).use { input ->
            val buffer = ByteArray(64 * 1024)
            var written = 0L
            var entries = 0
            var entry = input.nextEntry
            while (entry != null) {
                val target = File(staging, entry.name)
                // Reject zip-slip: entries must stay inside the staging directory.
                if (!target.canonicalPath.startsWith(staging.canonicalPath + File.separator)) {
                    error("El zip tiene una ruta no valida: ${entry.name}")
                }
                if (entry.isDirectory) {
                    target.mkdirs()
                } else {
                    target.parentFile?.mkdirs()
                    target.outputStream().buffered().use { output ->
                        var n = input.read(buffer)
                        while (n > 0) {
                            output.write(buffer, 0, n)
                            written += n
                            onProgress(((written * 100) / total).toInt().coerceIn(0, 99))
                            n = input.read(buffer)
                        }
                    }
                }
                input.closeEntry()
                entries++
                // A game zip holds tens of thousands of files; on slow/adopted storage the extraction
                // can take a while, so leave a trail instead of going silent.
                if (entries % 5000 == 0) {
                    trace("import: extraidas $entries entradas (${written / (1024 * 1024)} MB)")
                }
                entry = input.nextEntry
            }
            trace("import: extraccion terminada ($entries entradas, ${written / (1024 * 1024)} MB)")
        }

        // A zip of the game folder keeps it one level down, and the emulator wants it at the root.
        unwrapSingleFolder(staging)
        trace("import: primer nivel de ${staging.name} = ${staging.listFiles()?.joinToString { it.name } ?: "vacio"}")

        if (staging.listFiles()?.none { it.isFile && it.extension.equals("exe", ignoreCase = true) } != false ||
            !File(staging, "data").isDirectory
        ) {
            staging.deleteRecursively()
            error("El zip no trae el .exe y la carpeta data/.")
        }

        // Carry the player's data across an update: the folders the game's own migration tells you
        // to keep. They are copied aside before the old install is swapped out and merged back into
        // the new one, so importing a new version no longer costs anyone their saves.
        val saved = File(files, "game.saves")
        saved.deleteRecursively()
        var kept = 0
        if (gameDir.isDirectory) {
            for (folder in saveFolders) {
                val source = File(gameDir, "data/$folder")
                if (!source.isDirectory) continue
                val dest = File(saved, folder)
                runCatching { source.copyRecursively(dest, overwrite = true) }
                if (dest.isDirectory) kept += dest.walkTopDown().count { it.isFile }
            }
        }
        trace("import: conservados $kept ficheros de partidas y datos del jugador")

        // Swap the folders instead of erasing the old install first: if the last move fails, the
        // previous game (and its saves) is still on disk to fall back to.
        val previous = File(files, "game.old")
        previous.deleteRecursively()
        val movedAside = gameDir.isDirectory && gameDir.renameTo(previous)
        if (gameDir.isDirectory) gameDir.deleteRecursively()
        if (!staging.renameTo(gameDir)) {
            if (movedAside) {
                gameDir.deleteRecursively()
                previous.renameTo(gameDir)
            }
            staging.deleteRecursively()
            saved.deleteRecursively()
            error("No pude mover el juego a su carpeta definitiva.")
        }
        previous.deleteRecursively()

        // Put the player's data back. Anything the new version ships in those folders wins, so an
        // update never reverts the game's own files; a save is only added when it is missing.
        for (folder in saveFolders) {
            val source = File(saved, folder)
            if (!source.isDirectory) continue
            val dest = File(gameDir, "data/$folder")
            dest.mkdirs()
            runCatching { source.copyRecursively(dest, overwrite = false) }
        }
        saved.deleteRecursively()
        metaFile.writeText(
            JSONObject()
                .put("version", version ?: "")
                .put("source", source)
                .put("installedAt", System.currentTimeMillis())
                .put("sizeBytes", gameDir.walkTopDown().filter { it.isFile }.sumOf { it.length() })
                .toString(),
        )
        onProgress(100)
    }

    /** Cheap structural check before doing the expensive extraction. */
    private fun requireGameZip(file: File) {
        ZipFile(file).use { zip ->
            var exe = false
            var data = false
            var seen = 0
            val first = StringBuilder()
            val entries = zip.entries()
            while (entries.hasMoreElements()) {
                val name = entries.nextElement().name
                if (seen < 12) {
                    first.append(name).append(" | ")
                    seen++
                }
                if (name.endsWith(".exe", ignoreCase = true)) exe = true
                if (name.startsWith("data/") || name.contains("/data/")) data = true
            }
            trace("import: exe=$exe data=$data; empieza por: $first")
            // Only a sanity check. The layout is settled once the zip is unpacked, because a zip
            // made by right-clicking the game folder keeps everything one level down.
            if (!exe || !data) error("Ese zip no parece Pokemon Reloaded: no encuentro el .exe ni la carpeta data/.")
        }
    }

    /**
     * Writes one line to logcat, which is what the log screen reads, and also to a small file.
     *
     * logcat only holds what the running process printed, so anything logged before the phone was
     * restarted is gone - which is exactly when somebody sits down to report a bug. The file keeps
     * the last stretch, and is rolled over so it cannot grow without end.
     */
    private fun trace(message: String) {
        android.util.Log.i("Reloaded", message)
        runCatching {
            val file = File(files, "app.log")
            if (file.length() > 256 * 1024) {
                File(files, "app.log.1").delete()
                file.renameTo(File(files, "app.log.1"))
            }
            file.appendText("$message\n")
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
                    for (i in 0 until nameLen) {
                        val at = p + 46 + i
                        val byte = cd[at].toInt() and 0xff
                        if (byte > 0x7f) {
                            cd[at] = foldToAscii(byte)
                            changed = true
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

    /**
     * Moves the contents of a lone top-level folder up to `staging`.
     *
     * A zip made by compressing the game folder keeps the game one level down, and the emulator
     * wants it at the root: refusing those is what made importing fail for most people. It only
     * acts when that folder really looks like the game, and does nothing when the top level holds
     * several things, because then there is no telling which one is the game.
     */
    private fun unwrapSingleFolder(staging: File) {
        val top = staging.listFiles()?.filter { it.name != "__MACOSX" && !it.name.startsWith(".") } ?: return
        val only = top.singleOrNull()?.takeIf { it.isDirectory } ?: return
        val inner = only.listFiles()?.toList().orEmpty()
        val hasExe = inner.any { it.isFile && it.extension.equals("exe", ignoreCase = true) }
        val hasData = inner.any { it.isDirectory && it.name == "data" }
        if (!hasExe || !hasData) return
        for (child in inner) {
            if (!child.renameTo(File(staging, child.name))) return
        }
        only.delete()
    }
}
