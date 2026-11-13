package com.swayconnect.protocol

import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.put
import kotlinx.serialization.json.putJsonArray
import kotlinx.serialization.json.putJsonObject
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test

/** Replays protocol/vectors/v1/ext/icons.json (docs/PROTOCOL.md 17.5) from the client side. */
class IconsVectorTest {
    private val file = Vectors.load("ext/icons.json").jsonObject
    private val store: Map<String, ByteArray> = file.getValue("store").jsonArray.associate {
        it.text("hash") to Base64Url.decodeAny(it.text("png_b64url"))!!
    }

    /** The `welcome` example of 17.18: T2's handshake, with `icon.get` offered. */
    private val handshake: List<Step> by lazy {
        val welcome = Vectors.load("ext/welcome.json").jsonObject.getValue("frames").jsonArray.first()
        Fixtures.authHandshake.dropLast(1) + Step.Send(parseObject(welcome.text("payload")))
    }

    @Test
    fun `store hashes are the hash of their png`() {
        store.forEach { (hash, png) -> assertEquals(hash, IconHash.of(png)) }
    }

    @Test
    fun `results are checked as the file says`() {
        for (case in file.getValue("results").jsonArray) {
            val name = case.text("name")
            val requested = case.jsonObject.getValue("requested").jsonArray.map { it.jsonPrimitive.content }
            val result = (Wire.decodeServerMessage(parseObject(case.text("payload"))) as Res).result!!
            val expect = case.jsonObject.getValue("expect")
            if (expect == JsonPrimitive("bad")) {
                assertThrows(name, MalformedMessageException::class.java) { readIconResult(requested, result) }
                continue
            }
            val batch = readIconResult(requested, result)
            assertEquals(name, hashes(expect.jsonObject.getValue("accepted")), batch.icons.map { it.hash })
            val dropped = expect.jsonObject.getValue("dropped").jsonArray.map {
                DroppedIcon(it.text("hash"), DropReason.valueOf(it.text("reason").uppercase()))
            }
            assertEquals(name, dropped, batch.dropped)
            assertEquals(name, hashes(expect.jsonObject.getValue("missing")), batch.missing)
        }
    }

    @Test
    fun `iconGet sends each valid request and reports the outcome`() {
        // Case 26 has 9 hashes: the server checks its rate limit first, but a client never sends 9.
        val sendable = requestCases().filter { !isRefusedByTheClient(it) && requestedHashes(it)!!.size <= IconHash.MAX_PER_REQUEST }
        assertTrue(sendable.size >= 10)
        for (case in sendable) {
            val requested = requestedHashes(case)!!
            val expect = case.jsonObject.getValue("expect").jsonObject
            val outcome = runIconGet(requested, answerTo(expect))
            val name = case.text("name")
            val error = expect["error"]
            if (error != null) {
                val failure = outcome.exceptionOrNull() as? RequestFailedException
                assertEquals(name, error.jsonPrimitive.content, failure?.code)
                expect["detail"]?.let { assertEquals(name, concrete(it.jsonObject), failure?.detail) }
            } else {
                val icons = outcome.getOrThrow().icons
                val expected = expect.getValue("result").jsonObject.getValue("icons")
                assertEquals(name, hashes(expected), icons.map { it.hash })
                icons.forEach { assertArrayEquals(name, store.getValue(it.hash), it.png) }
            }
        }
    }

    @Test
    fun `iconGet refuses bad hash lists before sending`() {
        val refused = requestCases().filter(::isRefusedByTheClient)
        assertTrue(refused.size >= 8)
        for (case in refused) {
            assertThrows(case.text("name"), IllegalArgumentException::class.java) {
                runIconGet(requestedHashes(case)!!, answer = null).getOrThrow()
            }
        }
    }

    @Test
    fun `eight icons of 65536 bytes arrive in one frame`() {
        val case = file.getValue("results").jsonArray.single { it.text("name").startsWith("06-") }
        val requested = case.jsonObject.getValue("requested").jsonArray.map { it.jsonPrimitive.content }
        val answer = JsonObject(parseObject(case.text("payload")) + ("id" to JsonPrimitive(1)))

        val batch = runIconGet(requested, answer).getOrThrow()

        assertEquals(requested, batch.icons.map { it.hash })
        batch.icons.forEach { assertEquals(IconHash.MAX_PNG_BYTES, it.png.size) }
    }

    /** Request cases whose `hashes` a client API can express: a list of strings. */
    private fun requestCases(): List<JsonElement> = file.getValue("requests").jsonArray.filter { requestedHashes(it) != null }

    private fun requestedHashes(case: JsonElement): List<String>? {
        val hashes = (parseObject(case.text("payload"))["args"] as? JsonObject)?.get("hashes") as? JsonArray ?: return null
        return hashes.map { (it as? JsonPrimitive)?.takeIf { value -> value.isString }?.content ?: return null }
    }

    private fun isRefusedByTheClient(case: JsonElement): Boolean =
        (case.jsonObject.getValue("expect") as? JsonObject)?.get("error") == JsonPrimitive("bad_request")

    /** Runs one [Session.iconGet]. [answer] is null when the client must not send anything. */
    private fun runIconGet(requested: List<String>, answer: JsonObject?): Result<IconBatch> {
        val request = buildJsonObject {
            put("type", "req")
            put("op", Op.ICON_GET)
            putJsonObject("args") { putJsonArray("hashes") { requested.forEach { add(JsonPrimitive(it)) } } }
        }
        val exchange = if (answer == null) emptyList() else listOf(Step.Expect(request), Step.Send(answer))
        val server = ScriptedServer(handshake + exchange + Step.AwaitRelease, Fixtures.fingerprint, ignoredMembers = setOf("id"))
        val client = clientFor(server, Fixtures.cliClient, Fixtures.laterClientNonce)
        var outcome: Result<IconBatch>? = null
        replay(server) {
            client.runSession(Fixtures.profile, Fixtures.key) { session ->
                outcome = runCatching { session.iconGet(requested) }
                server.release()
            }
        }
        return outcome!!
    }

    private fun answerTo(expect: JsonObject): JsonObject =
        buildJsonObject {
            put("type", "res")
            put("id", 1)
            val error = expect["error"]
            put("ok", error == null)
            if (error != null) {
                putJsonObject("error") {
                    put("code", error)
                    put("message", "from the vector file")
                    expect["detail"]?.let { put("detail", concrete(it.jsonObject)) }
                }
            } else {
                val result = expect.getValue("result").jsonObject
                putJsonObject("result") {
                    putJsonArray("icons") {
                        hashes(result.getValue("icons")).forEach { hash ->
                            add(buildJsonObject {
                                put("hash", hash)
                                put("png", Base64Url.encode(store.getValue(hash)))
                            })
                        }
                    }
                    put("missing", result.getValue("missing"))
                }
            }
        }

    /** README rule 6 of requests.json: `{"min", "max"}` stands for any value in that range. The lowest is used. */
    private fun concrete(detail: JsonObject): JsonObject =
        JsonObject(detail.mapValues { (_, value) -> (value as? JsonObject)?.get("min") ?: value })

    private fun hashes(list: JsonElement): List<String> = list.jsonArray.map { it.jsonPrimitive.content }
}
