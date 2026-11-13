package com.swayconnect.android.data

import com.swayconnect.protocol.Desktop
import com.swayconnect.protocol.DesktopSerializer
import com.swayconnect.protocol.Wire
import java.io.File
import java.io.IOException
import kotlinx.serialization.SerializationException
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.longOrNull
import kotlinx.serialization.json.put

/** The last desktop state of one computer, and when it arrived (wall clock). */
class CachedDesktop(val desktop: Desktop, val savedAtMs: Long)

/**
 * Keeps the last desktop state in a private file, so the app can show it, dimmed, before it connects
 * (docs/UX.md 5.9). The file names the computer by its pinned fingerprint; another computer's snapshot is
 * never shown. Every call blocks.
 */
class DesktopCache(private val file: File) {

    /** The snapshot of the computer with fingerprint [fp], or null when there is none or it cannot be read. */
    fun load(fp: String): CachedDesktop? {
        if (!file.exists()) return null
        return try {
            val fields = Wire.json.parseToJsonElement(file.readText(Charsets.UTF_8)).jsonObject
            if (fields[FP]?.jsonPrimitive?.content != fp) return null
            val savedAtMs = fields[SAVED_AT_MS]?.jsonPrimitive?.longOrNull ?: return null
            val desktop = Wire.json.decodeFromJsonElement(DesktopSerializer, fields[DESKTOP] ?: return null)
            CachedDesktop(desktop, savedAtMs)
        } catch (e: IOException) {
            null
        } catch (e: SerializationException) {
            // A damaged cache is only a missing cache: the next state replaces it.
            null
        } catch (e: IllegalArgumentException) {
            // A member of the wrong JSON type.
            null
        }
    }

    /**
     * Replaces the snapshot. Without [keepTitles] (the device lacks scope `state.titles`) every window title is
     * dropped before it reaches the disk.
     */
    @Throws(IOException::class)
    fun save(fp: String, desktop: Desktop, savedAtMs: Long, keepTitles: Boolean) {
        val kept = if (keepTitles) desktop else desktop.withoutTitles()
        val fields = buildJsonObject {
            put(FP, fp)
            put(SAVED_AT_MS, savedAtMs)
            put(DESKTOP, Wire.json.encodeToJsonElement(DesktopSerializer, kept))
        }
        // Written beside the target and renamed, so a crash never leaves half a file.
        val draft = File(file.parentFile, file.name + ".tmp")
        draft.writeText(fields.toString(), Charsets.UTF_8)
        if (!draft.renameTo(file)) throw IOException("the desktop cache could not be replaced")
    }

    /** Deletes the snapshot, for example when the pairing is forgotten: it may hold window titles. */
    @Throws(IOException::class)
    fun clear() {
        if (file.exists() && !file.delete()) throw IOException("the desktop cache could not be deleted")
    }

    private companion object {
        const val FP = "fp"
        const val SAVED_AT_MS = "saved_at_ms"
        const val DESKTOP = "desktop"
    }
}

private fun Desktop.withoutTitles(): Desktop =
    copy(
        windows = windows.map { it.copy(title = null) },
        scratchpad = scratchpad?.map { it.copy(title = null) },
    )
