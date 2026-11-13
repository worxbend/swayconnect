package com.swayconnect.protocol

import kotlinx.serialization.KSerializer
import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import kotlinx.serialization.SerializationException
import kotlinx.serialization.descriptors.SerialDescriptor
import kotlinx.serialization.descriptors.buildClassSerialDescriptor
import kotlinx.serialization.encoding.Decoder
import kotlinx.serialization.encoding.Encoder
import kotlinx.serialization.json.JsonDecoder
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject

// The desktop additions of docs/PROTOCOL.md 17.4. Strings here come from the desktop: plain text only.

/** Global logical pixels. */
@Serializable
data class Rect(
    @Serializable(with = WireSint::class) val x: Long,
    @Serializable(with = WireSint::class) val y: Long,
    @Serializable(with = WireInt::class) val w: Long,
    @Serializable(with = WireInt::class) val h: Long,
)

@Serializable(with = TransformSerializer::class)
enum class Transform(val wire: String) {
    NORMAL("normal"),
    ROTATE_90("90"),
    ROTATE_180("180"),
    ROTATE_270("270"),
    FLIPPED("flipped"),
    FLIPPED_90("flipped-90"),
    FLIPPED_180("flipped-180"),
    FLIPPED_270("flipped-270"),
}

internal object TransformSerializer :
    FallbackEnumSerializer<Transform>("com.swayconnect.protocol.Transform", Transform.entries, Transform.NORMAL, Transform::wire)

@Serializable
data class Output(
    val name: String,
    val model: String?,
    val rect: Rect,
    @SerialName("scale_milli") @Serializable(with = WireInt::class) val scaleMilli: Long,
    val transform: Transform,
    val active: Boolean,
    val power: Boolean,
    @SerialName("current_workspace_id") @Serializable(with = WireInt::class) val currentWorkspaceId: Long?,
)

/** A window hidden in the scratchpad. A shown scratchpad window is an entry of [Desktop.windows] instead. */
@Serializable
data class ScratchpadWindow(
    @Serializable(with = WireInt::class) val id: Long,
    @SerialName("app_id") val appId: String?,
    val title: String?,
    val icon: String?,
)

/** The tiling layout of one workspace. Display only: never a reason to call an operation (17.4.3). */
@Serializable
data class Layout(
    /** Width and height in logical pixels. */
    val size: List<@Serializable(with = WireInt::class) Long>,
    @Serializable(with = WireInt::class) val fullscreen: Long?,
    val tiled: Tile?,
    /** Bottom first, top last. */
    val floating: List<Tile>,
) {
    init {
        wireRequire(size.size == 2) { "layout size needs 2 values" }
    }
}

/** [bounds] is `[x, y, w, h]` in basis points (0 to 10000) of the workspace rect. */
@Serializable(with = TileSerializer::class)
sealed interface Tile {
    val bounds: List<Long>
}

@Serializable
data class WindowTile(
    @SerialName("w") @Serializable(with = WireInt::class) val windowId: Long,
    @SerialName("r") override val bounds: List<@Serializable(with = WireInt::class) Long>,
) : Tile {
    init {
        requireBasisPointRect(bounds)
    }
}

@Serializable
data class ContainerTile(
    @SerialName("k") val kind: ContainerKind,
    @SerialName("r") override val bounds: List<@Serializable(with = WireInt::class) Long>,
    @SerialName("c") val children: List<Tile>,
    /** For tabbed and stacked containers: the index of the child shown. */
    @SerialName("a") @Serializable(with = WireInt::class) val active: Long? = null,
) : Tile {
    init {
        requireBasisPointRect(bounds)
        wireRequire(children.isNotEmpty()) { "a container has at least one child" }
    }
}

@Serializable(with = ContainerKindSerializer::class)
enum class ContainerKind(val wire: String, val showsOneChild: Boolean) {
    SPLIT_H("h", false),
    SPLIT_V("v", false),
    TABBED("t", true),
    STACKED("s", true),
}

internal object ContainerKindSerializer :
    FallbackEnumSerializer<ContainerKind>(
        "com.swayconnect.protocol.ContainerKind",
        ContainerKind.entries,
        ContainerKind.SPLIT_H,
        ContainerKind::wire,
    )

private fun requireBasisPointRect(bounds: List<Long>) = wireRequire(bounds.size == 4) { "a tile rect needs 4 values" }

/** A tile is a window tile when it has member `w` (17.4.3); its `k` and `c` are then ignored. */
internal object TileSerializer : KSerializer<Tile> {
    override val descriptor: SerialDescriptor = buildClassSerialDescriptor("com.swayconnect.protocol.Tile")

    override fun deserialize(decoder: Decoder): Tile {
        val json = decoder as? JsonDecoder ?: throw SerializationException("a tile is read from JSON only")
        val fields = json.decodeJsonElement() as? JsonObject ?: throw SerializationException("a tile is an object")
        if ("w" in fields) return json.json.decodeFromJsonElement(WindowTile.serializer(), fields)
        wireRequire(fields["a"] !is JsonNull) { "member \"a\" must not be null" }
        return json.json.decodeFromJsonElement(ContainerTile.serializer(), fields)
    }

    override fun serialize(encoder: Encoder, value: Tile) =
        when (value) {
            is WindowTile -> encoder.encodeSerializableValue(WindowTile.serializer(), value)
            is ContainerTile -> encoder.encodeSerializableValue(ContainerTile.serializer(), value)
        }
}

private const val MAX_BASIS_POINTS = 10_000L

/**
 * The client rules of 17.4.2 and 17.4.3 that need more than one value: a tile whose window is not in
 * [Desktop.windows] is skipped, each rect value is clamped to 10000, and a tabbed or stacked container
 * shows child 0 when `a` is absent or out of range. Enum fallbacks already happen while decoding.
 */
fun Desktop.normalized(): Desktop {
    val known = windows.mapTo(HashSet()) { it.id }
    return copy(workspaces = workspaces.map { it.copy(layout = it.layout?.normalized(known)) })
}

private fun Layout.normalized(known: Set<Long>): Layout =
    copy(tiled = tiled?.normalized(known), floating = floating.mapNotNull { it.normalized(known) })

private fun Tile.normalized(known: Set<Long>): Tile? =
    when (this) {
        is WindowTile -> if (windowId in known) copy(bounds = bounds.clamped()) else null
        is ContainerTile -> normalized(known)
    }

/** A container that keeps no child is skipped too: an empty `c` is not a valid tile. */
private fun ContainerTile.normalized(known: Set<Long>): ContainerTile? {
    val kept = children.map { it.normalized(known) }
    val remaining = kept.filterNotNull()
    if (remaining.isEmpty()) return null
    val shown = if (kind.showsOneChild) shownIndexAfterSkip(kept) else active
    return copy(bounds = bounds.clamped(), children = remaining, active = shown)
}

/** The index of the shown child among the children kept, or 0 when the shown one was skipped. */
private fun ContainerTile.shownIndexAfterSkip(kept: List<Tile?>): Long {
    val shown = active?.takeIf { it < children.size }?.toInt() ?: 0
    if (kept[shown] == null) return 0
    return kept.subList(0, shown).count { it != null }.toLong()
}

private fun List<Long>.clamped(): List<Long> = map { it.coerceAtMost(MAX_BASIS_POINTS) }
