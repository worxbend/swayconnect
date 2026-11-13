package com.swayconnect.protocol

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject

/**
 * State of topic `desktop` (docs/PROTOCOL.md section 9).
 * Names, titles and application ids come from applications: plain text only, never markup, a URL or a command.
 */
@Serializable
data class Desktop(
    val sway: SwayStatus,
    val workspaces: List<Workspace>,
    val windows: List<Window>,
    val truncated: Boolean,
    /** Absent from an older server, like the other members of 17.4. */
    val outputs: List<Output>? = null,
    val scratchpad: List<ScratchpadWindow>? = null,
    /** True when the server dropped every [Workspace.layout] to fit the size limit. */
    @SerialName("layouts_cut") val layoutsCut: Boolean? = null,
)

@Serializable
data class SwayStatus(val online: Boolean, val version: String?)

@Serializable
data class Workspace(
    val id: Long,
    val number: Long?,
    val name: String,
    val output: String?,
    val focused: Boolean,
    val visible: Boolean,
    val urgent: Boolean,
    /** `null` when unknown or absent. */
    val rect: Rect? = null,
    /** `null` when the server dropped it or is older. */
    val layout: Layout? = null,
)

@Serializable
data class Window(
    val id: Long,
    @SerialName("workspace_id") val workspaceId: Long,
    @SerialName("app_id") val appId: String?,
    val title: String?,
    val focused: Boolean,
    val urgent: Boolean,
    val floating: Boolean,
    val fullscreen: Boolean,
    /** Icon hash for [Session.iconGet]; `null` when the server has none. */
    val icon: String? = null,
)

/** Deepest layout a server sends (docs/PROTOCOL.md 17.17). Deeper input is refused before the recursive decoder sees it. */
private const val MAX_LAYOUT_DEPTH = 16

/**
 * Decodes a `desktop` state. Refuses `null` in the optional members that have no `or null`, and a layout
 * deeper than [MAX_LAYOUT_DEPTH] container levels, which no conformant server sends.
 */
object DesktopSerializer : AbsentOrNotNullSerializer<Desktop>(Desktop.serializer(), setOf("outputs", "scratchpad", "layouts_cut")) {
    override fun transformDeserialize(element: JsonElement): JsonElement {
        super.transformDeserialize(element)
        val workspaces = (element as JsonObject)["workspaces"] as? JsonArray ?: return element
        for (workspace in workspaces) {
            val layout = (workspace as? JsonObject)?.get("layout") as? JsonObject ?: continue
            layout["tiled"]?.let { checkTileDepth(it, levelsAbove = 0) }
            (layout["floating"] as? JsonArray)?.forEach { checkTileDepth(it, levelsAbove = 0) }
        }
        return element
    }

    /** Recursion stops at [MAX_LAYOUT_DEPTH] + 1 levels. Wrong shapes are left to the decoder. */
    private fun checkTileDepth(tile: JsonElement, levelsAbove: Int) {
        val fields = tile as? JsonObject ?: return
        if ("w" in fields) return
        val children = fields["c"] as? JsonArray ?: return
        wireRequire(levelsAbove < MAX_LAYOUT_DEPTH) { "layout deeper than $MAX_LAYOUT_DEPTH container levels" }
        children.forEach { checkTileDepth(it, levelsAbove + 1) }
    }
}
