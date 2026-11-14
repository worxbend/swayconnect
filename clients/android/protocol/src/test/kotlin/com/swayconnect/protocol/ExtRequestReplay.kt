package com.swayconnect.protocol

import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.boolean
import kotlinx.serialization.json.booleanOrNull
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.int
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.put
import kotlinx.serialization.json.putJsonArray
import kotlinx.serialization.json.putJsonObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue

/** A typed call of the client. It returns the result members it reports, by their wire names. */
internal typealias TypedCall = suspend (Session) -> Map<String, JsonElement>

/**
 * Replays one `requests` case of an ext/ vector file (README rule 6 and its ext additions) through a typed call,
 * against a server whose `welcome` follows the case: its `ops` and `topics` are what a server lists for the case's
 * scopes and read-only mode. The client must refuse the call before sending (an op or topic that `welcome` lacks,
 * or an argument outside a bound the client knows), or send the payload and report the answer the file expects.
 *
 * @param knownOps the server's `known_ops` for a case.
 * @param topicState the state the server answers a subscription to a topic with, for a case.
 */
internal class ExtRequestReplay(
    private val knownOps: (JsonElement) -> List<String> = { EXAMPLE_KNOWN_OPS },
    private val topicState: (case: JsonElement, topic: String) -> JsonObject,
) {
    /** [breaksClientBound]: the typed call refuses the case's arguments before sending. */
    fun assertCase(case: JsonElement, call: TypedCall, breaksClientBound: Boolean) {
        val request = requestOf(case)
        when {
            breaksClientBound -> assertRefusedByTheClient(case, call)
            request.text("op") !in offeredOps(case) -> assertNotOffered(case, call)
            (request["args"] as? JsonObject)?.let(::topicsOf)?.all { it in offeredTopics(case) } == false -> assertNotOffered(case, call)
            else -> assertOutcome(case, call)
        }
    }

    /** A subscription leaves each subscribed topic with the state the server answered, as the session reads it. */
    fun subscribeCall(case: JsonElement, topics: List<String>): TypedCall = { session ->
        session.subscribe(topics)
        topics.forEach { topic ->
            val expected = decodeTopic(topic, topicState(case, topic))
            assertEquals("${case.text("name")}: $topic", expected, topicValue(session, topic))
        }
        emptyMap()
    }

    /**
     * The server checks the bucket (10.2 step 5) before the arguments (step 6), so a case whose bucket is empty
     * expects `rate_limited` for arguments that the client already refuses.
     */
    private fun assertRefusedByTheClient(case: JsonElement, call: TypedCall) {
        val name = case.text("name")
        val error = expectOf(case).text("error")
        assertTrue("$name: $error", error == "bad_request" || (error == "rate_limited" && bucketIsEmpty(case)))
        val outcome = run(case, exchange = emptyList(), call)
        assertTrue("$name: $outcome", outcome.exceptionOrNull() is IllegalArgumentException)
    }

    /** docs/PROTOCOL.md 13.2 rule 7 and 17.1 rule 2: the server does not list the op or topic, so the client never sends. */
    private fun assertNotOffered(case: JsonElement, call: TypedCall) {
        val name = case.text("name")
        assertTrue(name, expectOf(case).text("error") in setOf("forbidden", "read_only", "unknown_op"))
        val failure = run(case, exchange = emptyList(), call).exceptionOrNull() as? RequestFailedException
        assertEquals(name, RequestFailedException.NOT_OFFERED, failure?.code)
    }

    private fun assertOutcome(case: JsonElement, call: TypedCall) {
        val name = case.text("name")
        val expect = expectOf(case)
        val request = requestOf(case)
        val exchange = listOf(Step.Expect(request), Step.Send(answerTo(case, request, expect)))
        val outcome = run(case, exchange, call)
        val error = (expect as? JsonObject)?.get("error")
        if (error != null) {
            val failure = outcome.exceptionOrNull() as? RequestFailedException
            assertEquals(name, error.jsonPrimitive.content, failure?.code)
            expect.jsonObject["detail"]?.let { assertEquals(name, concrete(it.jsonObject), failure?.detail) }
            return
        }
        val reported = outcome.getOrElse { throw AssertionError("$name: the call failed", it) }
        val result = (expect as? JsonObject)?.get("result")?.let { concrete(it.jsonObject) } ?: JsonObject(emptyMap())
        reported.forEach { (member, value) -> assertEquals("$name: $member", result.getValue(member), value) }
    }

    /** Plays the case's handshake, then [exchange], and runs [call] in the session. */
    private fun run(case: JsonElement, exchange: List<Step>, call: TypedCall): Result<Map<String, JsonElement>> {
        val handshake = Fixtures.authHandshake.dropLast(1) + Step.Send(welcomeFor(case))
        val server = ScriptedServer(handshake + exchange + Step.AwaitRelease, Fixtures.fingerprint, ignoredMembers = setOf("id"))
        val client = clientFor(server, Fixtures.cliClient, Fixtures.laterClientNonce)
        var outcome: Result<Map<String, JsonElement>>? = null
        replay(server) {
            client.runSession(Fixtures.profile, Fixtures.key) { session ->
                outcome = runCatching { call(session) }
                server.release()
            }
        }
        return outcome!!
    }

    private fun welcomeFor(case: JsonElement): JsonObject {
        val fields = case.jsonObject
        return JsonObject(
            EXAMPLE_WELCOME + mapOf(
                "scopes" to fields.getValue("scopes"),
                "ops" to JsonArray(offeredOps(case).map(::JsonPrimitive)),
                "topics" to JsonArray(offeredTopics(case).map(::JsonPrimitive)),
                "known_ops" to JsonArray(knownOps(case).map(::JsonPrimitive)),
                "read_only" to fields.getValue("read_only"),
            ),
        )
    }

    /** ext/welcome.json `server_ops`: each known op whose scope the device holds, less the actions when read-only. */
    private fun offeredOps(case: JsonElement): List<String> {
        val scopes = scopesOf(case)
        val readOnly = case.jsonObject.getValue("read_only").jsonPrimitive.boolean
        return knownOps(case).filter { op ->
            val rule = OP_TABLE.getValue(op).jsonObject
            val scope = (rule["scope"] as? JsonPrimitive)?.takeIf { it.isString }?.content
            (scope == null || scope in scopes) && !(readOnly && rule.getValue("action").jsonPrimitive.boolean)
        }
    }

    /** docs/PROTOCOL.md 6.3 `topics`, "state topics this device may read": each needs `state.read` and its 17.2 scope. */
    private fun offeredTopics(case: JsonElement): List<String> {
        val scopes = scopesOf(case)
        if (STATE_READ !in scopes) return emptyList()
        return TOPIC_SCOPES.filterValues { it in scopes }.keys.toList()
    }

    private fun bucketIsEmpty(case: JsonElement): Boolean {
        val buckets = case.jsonObject["buckets"] as? JsonObject ?: return false
        val rateClass = OpTable.rules.getValue(requestOf(case).text("op")).rateClass ?: return false
        return buckets[rateClass.name.lowercase()]?.jsonPrimitive?.int == 0
    }

    private fun answerTo(case: JsonElement, request: JsonObject, expect: JsonElement): JsonObject =
        buildJsonObject {
            put("type", "res")
            put("id", 1)
            val error = (expect as? JsonObject)?.get("error")
            put("ok", error == null)
            if (error != null) {
                putJsonObject("error") {
                    put("code", error)
                    put("message", "from the vector file")
                    expect.jsonObject["detail"]?.let { put("detail", concrete(it.jsonObject)) }
                }
            } else {
                put("result", resultFor(case, request, expect))
            }
        }

    private fun resultFor(case: JsonElement, request: JsonObject, expect: JsonElement): JsonObject {
        (expect as? JsonObject)?.get("result")?.let { return concrete(it.jsonObject) }
        if (request.text("op") != Op.STATE_SUBSCRIBE) return JsonObject(emptyMap())
        return buildJsonObject {
            putJsonArray("states") {
                topicsOf(request.getValue("args").jsonObject)!!.forEach { topic ->
                    add(buildJsonObject {
                        put("topic", topic)
                        put("rev", 1)
                        put("data", topicState(case, topic))
                    })
                }
            }
        }
    }

    private fun decodeTopic(topic: String, data: JsonObject): Any =
        when (topic) {
            Topic.DESKTOP -> Wire.decode(DesktopSerializer, data).normalized()
            Topic.SESSION -> Wire.decode(SessionStateSerializer, data)
            Topic.AUDIO -> Wire.decode(AudioState.serializer(), data)
            Topic.MEDIA -> Wire.decode(MediaState.serializer(), data)
            Topic.APPS -> Wire.decode(AppsState.serializer(), data)
            else -> error("no decoder for topic $topic")
        }

    private fun topicValue(session: Session, topic: String): Any? =
        when (topic) {
            Topic.DESKTOP -> session.desktop.value
            Topic.SESSION -> session.sessionState.value
            Topic.AUDIO -> session.audio.value
            Topic.MEDIA -> session.media.value
            Topic.APPS -> session.apps.value
            else -> error("no state flow for topic $topic")
        }

    private fun scopesOf(case: JsonElement): Set<String> = case.jsonObject.getValue("scopes").jsonArray.map { it.jsonPrimitive.content }.toSet()

    companion object {
        private const val STATE_READ = "state.read"
        private val WELCOME_FILE = Vectors.load("ext/welcome.json").jsonObject
        private val OP_TABLE = WELCOME_FILE.getValue("op_table").jsonObject

        /** The `welcome` example of 17.18, whose `server_proof` matches T2. */
        private val EXAMPLE_WELCOME = parseObject(WELCOME_FILE.getValue("frames").jsonArray.first().text("payload"))
        val EXAMPLE_KNOWN_OPS: List<String> = EXAMPLE_WELCOME.getValue("known_ops").jsonArray.map { it.jsonPrimitive.content }

        /** The scope each topic needs besides `state.read` (17.2); `state.read` alone for desktop and session. */
        private val TOPIC_SCOPES = mapOf(
            Topic.DESKTOP to STATE_READ,
            Topic.SESSION to STATE_READ,
            Topic.AUDIO to "audio",
            Topic.MEDIA to "media",
            Topic.APPS to "launch",
        )

        /** The state a `state.changed` event payload carries. */
        fun stateOf(eventPayload: String): JsonObject {
            val event = Wire.decodeServerMessage(parseObject(eventPayload)) as Evt
            return Wire.decode(TopicState.serializer(), event.data).data
        }

        /** The state of ext/desktop.json case [prefix] (for example "03-"). */
        fun desktopCase(prefix: String): JsonObject =
            stateOf(Vectors.load("ext/desktop.json").jsonArray.single { it.text("name").startsWith(prefix) }.text("payload"))
    }
}

internal fun requestOf(case: JsonElement): JsonObject = parseObject(case.text("payload"))

internal fun expectOf(case: JsonElement): JsonElement = case.jsonObject.getValue("expect")

/** README rule 6 and the ext/requests.json addition: `{"min", "max"}` stands for any value in that range. The lowest is used. */
internal fun concrete(members: JsonObject): JsonObject =
    JsonObject(members.mapValues { (_, value) -> (value as? JsonObject)?.get("min") ?: value })

/** The topics of `state.subscribe` args when they are exactly a list of strings, else null. */
internal fun topicsOf(args: JsonObject): List<String>? {
    val topics = args["topics"] as? JsonArray ?: return null
    if (args.keys != setOf("topics")) return null
    return topics.map { (it as? JsonPrimitive)?.takeIf { topic -> topic.isString }?.content ?: return null }
}

private val INTEGER = Regex("-?(0|[1-9][0-9]*)")

/** A member that is a JSON integer literal (no fraction, no exponent) that fits a Long. */
internal fun JsonObject.int(name: String): Long? =
    (this[name] as? JsonPrimitive)?.takeIf { !it.isString && INTEGER.matches(it.content) }?.content?.toLongOrNull()

internal fun JsonObject.bool(name: String): Boolean? = (this[name] as? JsonPrimitive)?.takeIf { !it.isString }?.booleanOrNull

internal fun JsonObject.string(name: String): String? = (this[name] as? JsonPrimitive)?.takeIf { it.isString }?.content
