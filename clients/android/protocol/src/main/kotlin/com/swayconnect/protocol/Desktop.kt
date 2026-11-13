package com.swayconnect.protocol

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

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
)
