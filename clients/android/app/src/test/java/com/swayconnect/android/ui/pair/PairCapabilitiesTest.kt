package com.swayconnect.android.ui.pair

import com.swayconnect.android.ui.state.Granted
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class PairCapabilitiesTest {
    private fun granted(scopes: List<String>, known: List<String>?, readOnly: Boolean = false) =
        Granted(ops = emptySet(), scopes = scopes, readOnly = readOnly, knownScopes = known)

    @Test
    fun `granted pills come from scopes and locked pills from the rest of known_scopes`() {
        val capabilities =
            PairCapabilities.of(
                granted(
                    scopes = listOf("state.read", "state.titles", "control"),
                    known = listOf("state.read", "state.titles", "control", "idle.inhibit", "audio"),
                )
            )

        assertEquals(listOf("state.read", "state.titles", "control"), capabilities.granted)
        assertEquals(listOf("idle.inhibit", "audio"), capabilities.locked)
    }

    @Test
    fun `typing or launching says how to remove the phone and seeing or steering does not`() {
        val default =
            listOf("state.read", "state.titles", "control", "audio", "media", "idle.inhibit", "launch", "input.keys", "input.text")

        assertTrue(PairCapabilities.of(granted(scopes = default, known = null)).canRunCode)
        assertTrue(PairCapabilities.of(granted(scopes = listOf("input.keys"), known = null)).canRunCode)
        assertFalse(
            PairCapabilities.of(granted(scopes = listOf("state.read", "control", "audio"), known = listOf("launch"))).canRunCode
        )
    }

    @Test
    fun `known_scopes never counts as granted`() {
        val capabilities = PairCapabilities.of(granted(scopes = emptyList(), known = listOf("control", "audio")))

        assertTrue(capabilities.granted.isEmpty())
        assertEquals(listOf("control", "audio"), capabilities.locked)
    }

    @Test
    fun `a server without known_scopes shows no locked pills`() {
        val capabilities = PairCapabilities.of(granted(scopes = listOf("state.read"), known = null))

        assertEquals(listOf("state.read"), capabilities.granted)
        assertTrue(capabilities.locked.isEmpty())
    }

    @Test
    fun `reserved scopes without an operation get no pill`() {
        val capabilities =
            PairCapabilities.of(granted(scopes = listOf("commands"), known = listOf("commands", "input.pointer", "media")))

        assertTrue(capabilities.granted.isEmpty())
        assertEquals(listOf("media"), capabilities.locked)
    }

    @Test
    fun `duplicates are shown once and read-only is kept`() {
        val capabilities =
            PairCapabilities.of(granted(scopes = listOf("control", "control"), known = listOf("control"), readOnly = true))

        assertEquals(listOf("control"), capabilities.granted)
        assertTrue(capabilities.locked.isEmpty())
        assertTrue(capabilities.readOnly)
    }

    @Test
    fun `every scope of PROTOCOL 17_2 with an operation has a label and an unknown one has none`() {
        val named =
            listOf(
                "state.read", "state.titles", "control", "destructive", "idle.inhibit", "audio", "audio.capture",
                "media", "launch", "launch.risky", "input.text", "input.keys",
            )
        named.forEach { assertNotNull(it, scopeLabel(it)) }
        assertNull(scopeLabel("future.scope"))
    }
}
