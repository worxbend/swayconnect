package com.swayconnect.protocol

import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

/** Topic `session` in a live [Session] (docs/PROTOCOL.md 8.2, 17.1 rule 2, 17.8) and the rate classes of 17.14. */
class SessionTopicTest {
    private val states = Vectors.load("ext/session.json").jsonObject.getValue("states").jsonArray
    private val welcomeWithSession =
        parseObject(Vectors.load("ext/welcome.json").jsonObject.getValue("frames").jsonArray.first().text("payload"))

    @Test
    fun `session states follow the server and a revision that is not higher is dropped`() {
        val subscribe = Step.Expect(parseObject("""{"type":"req","id":1,"op":"state.subscribe","args":{"topics":["session"]}}"""))
        val subscribed = Step.Send(
            parseObject("""{"type":"res","id":1,"ok":true,"result":{"states":[{"topic":"session","rev":3,"data":${dataOf("01-")}}]}}"""),
        )
        // Both events come before the ping answer, so the client has handled them when the ping returns.
        val script = handshake() + subscribe + subscribed + pingRequest + event("05-", rev = 2) + event("03-", rev = 4) +
            pingResponse + Step.AwaitRelease
        val server = ScriptedServer(script, Fixtures.fingerprint)
        val client = clientFor(server, Fixtures.cliClient, Fixtures.laterClientNonce)
        var seen: SessionState? = null
        var desktop: Desktop? = null

        replay(server) {
            client.runSession(Fixtures.profile, Fixtures.key) { session ->
                session.subscribe(listOf(Topic.SESSION))
                session.request(Op.PING)
                seen = session.sessionState.value
                desktop = session.desktop.value
                server.release()
            }
        }

        assertEquals(Wire.decode(SessionStateSerializer, dataOf("03-")), seen)
        assertEquals(Holder.OTHER_DEVICE, seen?.keepAwake?.holder)
        assertNull("desktop was not subscribed", desktop)
    }

    /** T2's `welcome` lists topic `desktop` only. */
    @Test
    fun `a topic that welcome does not list is never subscribed`() {
        val server = ScriptedServer(Fixtures.authHandshake + Step.AwaitRelease, Fixtures.fingerprint)
        val client = clientFor(server, Fixtures.cliClient, Fixtures.laterClientNonce)
        var failure: RequestFailedException? = null

        replay(server) {
            client.runSession(Fixtures.profile, Fixtures.key) { session ->
                failure = runCatching { session.subscribe(listOf(Topic.DESKTOP, Topic.SESSION)) }.exceptionOrNull() as? RequestFailedException
                server.release()
            }
        }

        assertEquals(RequestFailedException.NOT_OFFERED, failure?.code)
    }

    @Test
    fun `each op has the rate class of 17_14`() {
        val expected = mapOf(
            RateClass.ACTIONS to setOf(
                "workspace.focus", "window.focus", "window.fullscreen", "window.move", "window.floating", "window.layout",
                "window.scratchpad", "window.close", "scratchpad.show", "session.lock", "keep_awake.set", "audio.set_mute",
                "audio.set_default", "media.play_pause", "media.next", "media.previous",
            ),
            RateClass.CONTINUOUS to setOf("audio.set_volume"),
            RateClass.LAUNCH to setOf("app.launch", "launcher.open"),
            RateClass.INPUT to setOf("input.key"),
            RateClass.INPUT_TEXT to setOf("input.text"),
            RateClass.ICONS to setOf("icon.get"),
        )
        val actual = OpTable.rules.entries.filter { it.value.rateClass != null }
            .groupBy({ it.value.rateClass!! }, { it.key }).mapValues { it.value.toSet() }

        assertEquals(expected, actual)
        assertEquals(setOf(Op.PING, Op.STATE_SUBSCRIBE), OpTable.rules.filterValues { it.rateClass == null }.keys)
    }

    /** T2 with the `welcome` example of 17.18, which lists topic `session`. */
    private fun handshake(): List<Step> = Fixtures.authHandshake.dropLast(1) + Step.Send(welcomeWithSession)

    private fun stateCase(prefix: String) = states.single { it.text("name").startsWith(prefix) }

    private fun dataOf(prefix: String): JsonObject {
        val event = parseObject(stateCase(prefix).text("payload"))
        return event.getValue("data").jsonObject.getValue("data").jsonObject
    }

    private fun event(prefix: String, rev: Int): Step {
        val event = parseObject(stateCase(prefix).text("payload"))
        val data = event.getValue("data").jsonObject
        return Step.Send(JsonObject(event + ("data" to JsonObject(data + ("rev" to JsonPrimitive(rev))))))
    }

    private val pingRequest = Step.Expect(parseObject("""{"type":"req","id":2,"op":"ping"}"""))
    private val pingResponse = Step.Send(parseObject("""{"type":"res","id":2,"ok":true,"result":{}}"""))
}
