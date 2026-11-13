package com.swayconnect.protocol

import java.io.ByteArrayInputStream
import java.io.ByteArrayOutputStream
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive

/** Reads protocol/vectors/v1, which the build puts on the test classpath as "/v1". */
internal object Vectors {
    fun load(path: String): JsonElement {
        val stream = Vectors::class.java.getResourceAsStream("/v1/$path")
            ?: error("vector file v1/$path is not on the test classpath")
        return stream.use { Json.parseToJsonElement(it.readBytes().toString(Charsets.UTF_8)) }
    }

    fun transcript(name: String): List<Step> =
        load("transcripts/$name.json").jsonObject.getValue("frames").jsonArray.map { frame ->
            val payload = parseObject(frame.text("payload"))
            if (frame.text("from") == "S") Step.Send(payload) else Step.Expect(payload)
        }
}

internal fun parseObject(json: String): JsonObject = Json.parseToJsonElement(json).jsonObject

internal fun JsonElement.text(name: String): String = jsonObject.getValue(name).jsonPrimitive.content

internal fun hexToBytes(hex: String): ByteArray = hex.chunked(2).map { it.toInt(16).toByte() }.toByteArray()

internal fun ByteArray.toHex(): String = joinToString("") { "%02x".format(it) }

/** What a peer reads after [frame] went through the frame codec with [limit]. */
internal fun throughCodec(frame: JsonObject, limit: Int): JsonObject? {
    val wire = ByteArrayOutputStream()
    FrameCodec.write(wire, frame, limit)
    return FrameCodec.read(ByteArrayInputStream(wire.toByteArray()), limit)
}
