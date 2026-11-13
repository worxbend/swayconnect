package com.swayconnect.protocol

import java.io.ByteArrayInputStream
import java.nio.ByteBuffer
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test

/** Replays protocol/vectors/v1/ext/desktop.json (docs/PROTOCOL.md 17.4). */
class DesktopExtVectorTest {
    private val cases = Vectors.load("ext/desktop.json").jsonArray

    @Test
    fun `ok states decode and re-encode to the same value`() {
        val okCases = cases.filter { it.jsonObject["expect"] == JsonPrimitive("ok") }
        assertTrue(okCases.isNotEmpty())
        for (case in okCases) {
            val state = stateOf(case.text("payload"))
            val encoded = Wire.json.encodeToJsonElement(DesktopSerializer, Wire.decode(DesktopSerializer, state))
            assertEquals(case.text("name"), withoutNullOptionals(state), encoded)
        }
    }

    @Test
    fun `bad states are refused`() {
        val badCases = cases.filter { it.jsonObject["expect"] == JsonPrimitive("bad") }
        assertTrue(badCases.isNotEmpty())
        for (case in badCases) {
            val state = stateOf(case.text("payload"))
            assertThrows(case.text("name"), MalformedMessageException::class.java) { Wire.decode(DesktopSerializer, state) }
        }
    }

    @Test
    fun `normalizing gives the normalized state`() {
        val normalizedCases = cases.filter { it.jsonObject["expect"] is JsonObject }
        assertTrue(normalizedCases.isNotEmpty())
        for (case in normalizedCases) {
            val expected = stateOf(case.jsonObject.getValue("expect").text("normalized"))
            val actual = Wire.decode(DesktopSerializer, stateOf(case.text("payload"))).normalized()
            assertEquals(case.text("name"), Wire.decode(DesktopSerializer, expected), actual)
        }
    }

    @Test
    fun `a layout of 16 container levels is read and one of 17 is refused`() {
        assertEquals(16, depthOf(Wire.decode(DesktopSerializer, stateWithNestedContainers(16))))
        assertThrows(MalformedMessageException::class.java) { Wire.decode(DesktopSerializer, stateWithNestedContainers(17)) }
    }

    @Test
    fun `the deepest layout fits the frame nesting limit`() {
        val frame = throughCodec(JsonObject(mapOf("data" to stateWithNestedContainers(16))), Limits.MAX_FRAME_S2C)
        assertEquals(16, depthOf(Wire.decode(DesktopSerializer, frame!!.getValue("data"))))
    }

    @Test
    fun `a deeply nested frame is refused before parsing`() {
        val payload = ("[".repeat(20_000) + "]".repeat(20_000)).toByteArray()
        val frame = ByteBuffer.allocate(4 + payload.size).putInt(payload.size).put(payload).array()

        val refused = assertThrows(FrameException::class.java) { FrameCodec.read(ByteArrayInputStream(frame), Limits.MAX_FRAME_S2C) }

        assertEquals(FrameException.Kind.BAD, refused.kind)
    }

    private fun stateOf(eventPayload: String): JsonObject {
        val event = Wire.decodeServerMessage(parseObject(eventPayload)) as Evt
        return Wire.decode(TopicState.serializer(), event.data).data
    }

    private fun depthOf(desktop: Desktop): Int {
        var tile: Tile? = desktop.workspaces.single().layout?.tiled
        var depth = 0
        while (tile is ContainerTile) {
            depth++
            tile = tile.children.single()
        }
        return depth
    }

    private fun stateWithNestedContainers(levels: Int): JsonObject {
        val tile = StringBuilder("""{"w":1,"r":[0,0,10000,10000]}""")
        repeat(levels) {
            tile.insert(0, """{"k":"v","r":[0,0,10000,10000],"c":[""")
            tile.append("]}")
        }
        return parseObject(
            """{"sway":{"online":true,"version":"1.12"},"workspaces":[{"id":2,"number":1,"name":"1","output":"DP-1",""" +
                """"focused":true,"visible":true,"urgent":false,"rect":{"x":0,"y":0,"w":100,"h":100},""" +
                """"layout":{"size":[100,100],"fullscreen":null,"tiled":$tile,"floating":[]}}],""" +
                """"windows":[{"id":1,"workspace_id":2,"app_id":null,"title":null,"focused":true,"urgent":false,""" +
                """"floating":false,"fullscreen":false}],"truncated":false}""",
        )
    }

    /**
     * `Workspace.rect`, `Workspace.layout` and `Window.icon` are optional and "or null". The decoded state
     * has one value for "absent" and "null", and re-encoding writes absent, so the payload's nulls are dropped.
     */
    private fun withoutNullOptionals(state: JsonObject): JsonObject {
        fun JsonElement.withoutNull(members: Set<String>) =
            JsonObject(jsonObject.filterNot { (name, value) -> name in members && value == JsonNull })
        return JsonObject(
            state + mapOf(
                "workspaces" to JsonArray(state.getValue("workspaces").jsonArray.map { it.withoutNull(setOf("rect", "layout")) }),
                "windows" to JsonArray(state.getValue("windows").jsonArray.map { it.withoutNull(setOf("icon")) }),
            ),
        )
    }
}
