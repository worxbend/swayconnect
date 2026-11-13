package com.swayconnect.protocol

import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test

class TranscriptsVectorTest {
    @Test
    fun `every payload of every transcript is accepted`() {
        val names = listOf(
            "t1-pair-ok", "t2-auth-ok", "t3-session", "t4-auth-failed", "t5-version", "t6-sway-offline", "t7-pair-refused",
        )
        for (name in names) {
            for (step in Vectors.transcript(name)) {
                when (step) {
                    is Step.Send -> assertNotNull(name, Wire.decodeServerMessage(step.payload))
                    is Step.Expect -> assertEquals(name, step.payload, throughCodec(step.payload, CLIENT_FRAME_LIMIT))
                    Step.ExpectAnyFrame, Step.AwaitRelease -> error("a transcript file has only frames")
                }
            }
        }
    }

    @Test
    fun `t1 pairing is stored only after the owner confirmed`() {
        val server = ScriptedServer(Vectors.transcript("t1-pair-ok"), Fixtures.fingerprint)
        val client = clientFor(server, Fixtures.androidClient, Fixtures.pairingClientNonce)
        var shownCode: String? = null

        val paired = replay(server) { client.pair(Fixtures.pairingUri, "Pixel 9") { shownCode = it } }

        assertEquals(Fixtures.confirmCode, shownCode)
        assertEquals(Fixtures.profile, paired.profile)
        assertArrayEquals(Fixtures.key, paired.key)
    }

    @Test
    fun `t7 refused pairing gives no credential`() {
        val server = ScriptedServer(Vectors.transcript("t7-pair-refused"), Fixtures.fingerprint)
        val client = clientFor(server, Fixtures.androidClient, Fixtures.pairingClientNonce)

        val refused = assertThrows(PairingNotConfirmedException::class.java) {
            replay(server) { client.pair(Fixtures.pairingUri, "Pixel 9") {} }
        }

        assertEquals(SessionEnd.ByServer(ByeCode.REVOKED, "pairing was not confirmed"), refused.end)
    }

    @Test
    fun `t2 authentication reaches the session`() {
        val server = ScriptedServer(Fixtures.authHandshake + Step.AwaitRelease, Fixtures.fingerprint)
        val client = clientFor(server, Fixtures.cliClient, Fixtures.laterClientNonce)
        var offered: List<String> = emptyList()

        val end = replay(server) {
            client.runSession(Fixtures.profile, Fixtures.key) {
                offered = it.welcome.ops
                server.release()
            }
        }

        assertTrue(offered.contains(Op.WINDOW_FULLSCREEN))
        assertTrue("the script ends without bye", end is SessionEnd.Lost)
    }

    @Test
    fun `t3 session matches responses, applies states and ends on bye`() {
        // The bye is held back until the client code has read the state that the frames before it gave.
        val transcript = Vectors.transcript("t3-session")
        val script = Fixtures.authHandshake + transcript.dropLast(1) + Step.AwaitRelease + transcript.last()
        val server = ScriptedServer(script, Fixtures.fingerprint)
        val client = clientFor(server, Fixtures.cliClient, Fixtures.laterClientNonce)
        var focusedAfterSubscribe: Long? = null
        var notFound: RequestFailedException? = null
        var desktopAtEnd: Desktop? = null

        val end = replay(server) {
            client.runSession(Fixtures.profile, Fixtures.key) { session ->
                session.subscribeDesktop()
                focusedAfterSubscribe = session.desktop.value?.workspaces?.single { it.focused }?.id
                session.focusWorkspaceNumber(2)
                try {
                    session.focusWindow(999)
                } catch (e: RequestFailedException) {
                    notFound = e
                }
                session.request(Op.PING)
                desktopAtEnd = session.desktop.value
                server.release()
            }
        }

        assertEquals(4L, focusedAfterSubscribe)
        assertEquals("not_found", notFound?.code)
        assertEquals(9L, desktopAtEnd?.workspaces?.single { it.focused }?.id)
        assertEquals("Example Domain — Mozilla Firefox", desktopAtEnd?.windows?.single { it.focused }?.title)
        assertEquals(SessionEnd.ByServer("shutdown", "daemon is shutting down"), end)
    }

    @Test
    fun `t4 auth_failed is reported with its code`() {
        // The transcript prints a zero proof. A client cannot produce it, so the proof is not compared.
        val script = Fixtures.authHello + Vectors.transcript("t4-auth-failed")
        val server = ScriptedServer(script, Fixtures.fingerprint, ignoredMembers = setOf("proof"))
        val client = clientFor(server, Fixtures.anonymousClient, Fixtures.laterClientNonce)

        val rejected = assertThrows(RejectedException::class.java) {
            replay(server) { client.runSession(Fixtures.profile, Fixtures.key) {} }
        }

        assertEquals(RejectCode.AUTH_FAILED, rejected.code)
    }

    @Test
    fun `t5 unsupported_version is reported with its code`() {
        // The transcript prints version 2, which this client never sends.
        val script = Fixtures.authHello + Vectors.transcript("t5-version")
        val server = ScriptedServer(script, Fixtures.fingerprint, ignoredMembers = setOf("protocol"))
        val client = clientFor(server, Fixtures.anonymousClient, Fixtures.laterClientNonce)

        val rejected = assertThrows(RejectedException::class.java) {
            replay(server) { client.runSession(Fixtures.profile, Fixtures.key) {} }
        }

        assertEquals(RejectCode.UNSUPPORTED_VERSION, rejected.code)
    }

    @Test
    fun `t6 offline state is applied and the action fails with sway_unavailable`() {
        // T6 is "during a session with a desktop subscription" and prints request id 6:
        // the subscription of T3 and four pings bring a fresh connection to that point.
        val subscription = Vectors.transcript("t3-session").take(2)
        val pings = (2..5).flatMap(::pingExchange)
        val script = Fixtures.authHandshake + subscription + pings + Vectors.transcript("t6-sway-offline") + Step.AwaitRelease
        val server = ScriptedServer(script, Fixtures.fingerprint)
        val client = clientFor(server, Fixtures.cliClient, Fixtures.laterClientNonce)
        var unavailable: RequestFailedException? = null
        var desktopAtEnd: Desktop? = null

        replay(server) {
            client.runSession(Fixtures.profile, Fixtures.key) { session ->
                session.subscribeDesktop()
                repeat(4) { session.request(Op.PING) }
                try {
                    session.focusWindow(5)
                } catch (e: RequestFailedException) {
                    unavailable = e
                }
                desktopAtEnd = session.desktop.value
                server.release()
            }
        }

        assertEquals("sway_unavailable", unavailable?.code)
        assertEquals(Desktop(SwayStatus(online = false, version = null), emptyList(), emptyList(), truncated = false), desktopAtEnd)
    }

    private fun pingExchange(id: Int): List<Step> =
        listOf(
            Step.Expect(parseObject("""{"type":"req","id":$id,"op":"ping"}""")),
            Step.Send(parseObject("""{"type":"res","id":$id,"ok":true,"result":{}}""")),
        )

    private companion object {
        const val CLIENT_FRAME_LIMIT = 16384
    }
}
