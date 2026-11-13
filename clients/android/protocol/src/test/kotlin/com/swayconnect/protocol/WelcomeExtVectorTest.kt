package com.swayconnect.protocol

import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.boolean
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import org.junit.Assert.assertEquals
import org.junit.Assert.assertThrows
import org.junit.Test

/** Replays protocol/vectors/v1/ext/welcome.json (docs/PROTOCOL.md 17.3 and 17.15). */
class WelcomeExtVectorTest {
    private val file = Vectors.load("ext/welcome.json").jsonObject

    @Test
    fun `the op table matches the vector file`() {
        val expected = file.getValue("op_table").jsonObject.mapValues { (_, rule) ->
            (rule.jsonObject["scope"] as? JsonPrimitive)?.takeIf { it.isString }?.content to
                rule.jsonObject.getValue("action").jsonPrimitive.boolean
        }
        assertEquals(expected, OpTable.rules.mapValues { (_, rule) -> rule.scope to rule.isAction })
    }

    @Test
    fun `welcome frames decode with their known lists or are refused`() {
        for (case in file.getValue("frames").jsonArray) {
            val frame = parseObject(case.text("payload"))
            val expect = case.jsonObject.getValue("expect")
            if (expect == JsonPrimitive("bad")) {
                assertThrows(case.text("name"), MalformedMessageException::class.java) { Wire.decodeServerMessage(frame) }
                continue
            }
            val welcome = Wire.decodeServerMessage(frame) as Welcome
            val fields = expect.jsonObject.getValue("fields").jsonObject
            val name = case.text("name")
            assertEquals(name, strings(fields.getValue("scopes")), welcome.scopes)
            assertEquals(name, strings(fields.getValue("ops")), welcome.ops)
            assertEquals(name, strings(fields.getValue("known_scopes")), welcome.knownScopes)
            assertEquals(name, strings(fields.getValue("known_ops")), welcome.knownOps)
            assertEquals(name, fields.getValue("read_only").jsonPrimitive.boolean, welcome.readOnly)
        }
    }

    @Test
    fun `availability follows the table of 17_3`() {
        for (case in file.getValue("availability").jsonArray) {
            val fields = case.jsonObject
            val welcome = welcomeWith(
                scopes = strings(fields.getValue("scopes"))!!,
                ops = strings(fields.getValue("ops"))!!,
                knownOps = strings(fields.getValue("known_ops")),
                readOnly = fields.getValue("read_only").jsonPrimitive.boolean,
            )
            val expected = when (val state = case.text("expect")) {
                "available" -> OpAvailability.Available
                "read_only" -> OpAvailability.ReadOnly
                "locked" -> OpAvailability.Locked(case.text("scope"))
                "unsupported" -> OpAvailability.Unsupported
                else -> error("unknown availability $state")
            }
            assertEquals(case.text("name"), expected, welcome.availabilityOf(case.text("op")))
        }
    }

    @Test
    fun `server ops follow from the op table`() {
        for (case in file.getValue("server_ops").jsonArray) {
            val fields = case.jsonObject
            val scopes = strings(fields.getValue("scopes"))!!.toSet()
            val readOnly = fields.getValue("read_only").jsonPrimitive.boolean
            val offered = strings(fields.getValue("known_ops"))!!.filter { op ->
                val rule = OpTable.rules.getValue(op)
                (rule.scope == null || rule.scope in scopes) && !(readOnly && rule.isAction)
            }
            assertEquals(case.text("name"), strings(fields.getValue("expect_ops"))!!.toSet(), offered.toSet())
        }
    }

    @Test
    fun `bye codes give the client reaction`() {
        for (case in file.getValue("bye").jsonArray) {
            val bye = Wire.decodeServerMessage(parseObject(case.text("payload"))) as Bye
            assertEquals(case.text("name"), ByeReaction.valueOf(case.text("expect").uppercase()), ByeReaction.of(bye.code))
        }
    }

    private fun strings(element: JsonElement): List<String>? =
        if (element == JsonNull) null else element.jsonArray.map { it.jsonPrimitive.content }

    private fun welcomeWith(scopes: List<String>, ops: List<String>, knownOps: List<String>?, readOnly: Boolean): Welcome =
        Welcome(
            protocol = 1,
            deviceId = Fixtures.deviceId,
            serverProof = "",
            scopes = scopes,
            ops = ops,
            topics = emptyList(),
            readOnly = readOnly,
            server = ServerInfo("desk", "0.2.0"),
            limits = FrameLimits(16384, 1_048_576, 90_000),
            knownOps = knownOps,
        )
}
