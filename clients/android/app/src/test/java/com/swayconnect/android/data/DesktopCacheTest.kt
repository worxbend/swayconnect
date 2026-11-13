package com.swayconnect.android.data

import com.swayconnect.protocol.Desktop
import com.swayconnect.protocol.ScratchpadWindow
import com.swayconnect.protocol.SwayStatus
import com.swayconnect.protocol.Window
import com.swayconnect.protocol.Workspace
import java.io.File
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder

class DesktopCacheTest {
    @get:Rule val directory = TemporaryFolder()

    private val file by lazy { File(directory.root, "last_desktop.json") }
    private val cache by lazy { DesktopCache(file) }

    private val desktop =
        Desktop(
            sway = SwayStatus(online = true, version = "1.11"),
            workspaces = listOf(Workspace(id = 4, number = 4, name = "4", output = "DP-1", focused = true, visible = true, urgent = false)),
            windows = listOf(
                Window(id = 7, workspaceId = 4, appId = "foot", title = "secret.txt", focused = true, urgent = false, floating = false, fullscreen = false),
            ),
            truncated = false,
            scratchpad = listOf(ScratchpadWindow(id = 9, appId = "foot", title = "notes", icon = null)),
        )

    @Test
    fun `a snapshot comes back as it was written`() {
        cache.save("fp-a", desktop, savedAtMs = 1234, keepTitles = true)

        val loaded = cache.load("fp-a")

        assertEquals(desktop, loaded?.desktop)
        assertEquals(1234L, loaded?.savedAtMs)
    }

    @Test
    fun `without state titles no title reaches the disk`() {
        cache.save("fp-a", desktop, savedAtMs = 1, keepTitles = false)

        val loaded = cache.load("fp-a")?.desktop
        assertEquals(listOf<String?>(null), loaded?.windows?.map { it.title })
        assertEquals(listOf<String?>(null), loaded?.scratchpad?.map { it.title })
        val text = file.readText()
        assertFalse(text.contains("secret.txt"))
        assertFalse(text.contains("notes"))
    }

    @Test
    fun `another computer's snapshot is not shown`() {
        cache.save("fp-a", desktop, savedAtMs = 1, keepTitles = true)

        assertNull(cache.load("fp-b"))
    }

    @Test
    fun `a damaged or missing file is no snapshot`() {
        assertNull(cache.load("fp-a"))
        file.writeText("{\"fp\":\"fp-a\",\"saved_at_ms\":1,\"desktop\":{\"sway\":3}}")
        assertNull(cache.load("fp-a"))
        file.writeText("{\"fp\":\"fp-a\",\"desktop\":{}}")
        assertNull(cache.load("fp-a"))
        file.writeText("not json")
        assertNull(cache.load("fp-a"))
        file.writeText("[1]")
        assertNull(cache.load("fp-a"))
    }

    @Test
    fun `clear deletes the snapshot`() {
        cache.save("fp-a", desktop, savedAtMs = 1, keepTitles = true)

        cache.clear()

        assertFalse(file.exists())
        assertNull(cache.load("fp-a"))
    }
}
