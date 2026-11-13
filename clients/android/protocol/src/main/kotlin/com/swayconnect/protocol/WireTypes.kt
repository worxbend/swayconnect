package com.swayconnect.protocol

import kotlinx.serialization.KSerializer
import kotlinx.serialization.SerializationException
import kotlinx.serialization.descriptors.PrimitiveKind
import kotlinx.serialization.descriptors.PrimitiveSerialDescriptor
import kotlinx.serialization.descriptors.SerialDescriptor
import kotlinx.serialization.encoding.Decoder
import kotlinx.serialization.encoding.Encoder
import kotlinx.serialization.json.JsonDecoder
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.JsonTransformingSerializer

private const val MAX_SAFE_INTEGER = 9_007_199_254_740_991L
private val INTEGER_LITERAL = Regex("-?(0|[1-9][0-9]*)")

/** Throws the exception [Wire.decode] turns into [MalformedMessageException]. */
internal fun wireRequire(condition: Boolean, message: () -> String) {
    if (!condition) throw SerializationException(message())
}

private fun Decoder.decodePrimitive(): JsonPrimitive {
    val json = this as? JsonDecoder ?: throw SerializationException("this type is read from JSON only")
    return json.decodeJsonElement() as? JsonPrimitive ?: throw SerializationException("expected a JSON primitive")
}

/**
 * A JSON number with no fraction and no exponent in [range] (docs/PROTOCOL.md 2.2 and 17.1 rule 6).
 * The generic decoder would accept `1.0` or a quoted number in some versions, so the literal is checked here.
 */
abstract class StrictIntegerSerializer(name: String, private val range: LongRange) : KSerializer<Long> {
    override val descriptor: SerialDescriptor = PrimitiveSerialDescriptor(name, PrimitiveKind.LONG)

    override fun deserialize(decoder: Decoder): Long {
        val literal = decoder.decodePrimitive()
        wireRequire(!literal.isString && INTEGER_LITERAL.matches(literal.content)) { "expected an integer" }
        return literal.content.toLongOrNull()?.takeIf { it in range }
            ?: throw SerializationException("integer out of range")
    }

    override fun serialize(encoder: Encoder, value: Long) = encoder.encodeLong(value)
}

/** Type `int`: 0 to 2^53 - 1. */
object WireInt : StrictIntegerSerializer("com.swayconnect.protocol.WireInt", 0..MAX_SAFE_INTEGER)

/** Type `sint`: -(2^53 - 1) to 2^53 - 1. */
object WireSint : StrictIntegerSerializer("com.swayconnect.protocol.WireSint", -MAX_SAFE_INTEGER..MAX_SAFE_INTEGER)

/** A closed string enum whose unknown values map to [fallback] (docs/PROTOCOL.md 17.1 rule 3). */
abstract class FallbackEnumSerializer<T : Enum<T>>(
    name: String,
    private val values: List<T>,
    private val fallback: T,
    private val wireName: (T) -> String,
) : KSerializer<T> {
    override val descriptor: SerialDescriptor = PrimitiveSerialDescriptor(name, PrimitiveKind.STRING)

    override fun deserialize(decoder: Decoder): T {
        val literal = decoder.decodePrimitive()
        wireRequire(literal.isString) { "expected a string" }
        return values.firstOrNull { wireName(it) == literal.content } ?: fallback
    }

    override fun serialize(encoder: Encoder, value: T) = encoder.encodeString(wireName(value))
}

/**
 * Refuses JSON `null` in [members], which are optional but not nullable: "absent" is a Kotlin null,
 * and an explicit `null` is a wrong type (docs/PROTOCOL.md 2.2).
 */
open class AbsentOrNotNullSerializer<T : Any>(inner: KSerializer<T>, private val members: Set<String>) :
    JsonTransformingSerializer<T>(inner) {
    override fun transformDeserialize(element: JsonElement): JsonElement {
        val fields = element as? JsonObject ?: throw SerializationException("expected an object")
        members.forEach { name -> wireRequire(fields[name] !is JsonNull) { "member \"$name\" must not be null" } }
        return element
    }
}
