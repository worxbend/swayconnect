package com.swayconnect.protocol

/** The scope each operation needs (7.2, 17.2), and whether read-only mode refuses it. */
class OpRule(val scope: String?, val isAction: Boolean)

/** Every operation of protocol version 1 with its rule, including those of later milestones. */
object OpTable {
    private const val CONTROL = "control"

    val rules: Map<String, OpRule> = buildMap {
        put(Op.PING, OpRule(null, isAction = false))
        put(Op.STATE_SUBSCRIBE, OpRule("state.read", isAction = false))
        put(Op.ICON_GET, OpRule("state.read", isAction = false))
        listOf(
            Op.WORKSPACE_FOCUS, Op.WINDOW_FOCUS, Op.WINDOW_FULLSCREEN, "window.move", "window.floating", "window.layout",
            "window.scratchpad", "scratchpad.show", "session.lock",
        ).forEach { put(it, OpRule(CONTROL, isAction = true)) }
        put("window.close", OpRule("destructive", isAction = true))
        put("keep_awake.set", OpRule("idle.inhibit", isAction = true))
        listOf("audio.set_volume", "audio.set_mute", "audio.set_default").forEach { put(it, OpRule("audio", isAction = true)) }
        listOf("media.play_pause", "media.next", "media.previous").forEach { put(it, OpRule("media", isAction = true)) }
        listOf("app.launch", "launcher.open").forEach { put(it, OpRule("launch", isAction = true)) }
        put("input.text", OpRule("input.text", isAction = true))
        put("input.key", OpRule("input.keys", isAction = true))
    }
}

/** How a client shows one operation (docs/PROTOCOL.md 17.3). */
sealed interface OpAvailability {
    data object Available : OpAvailability

    /** Show it disabled: the server is in read-only mode. */
    data object ReadOnly : OpAvailability

    /** Show it locked with [scope], which the owner can grant at the desktop. */
    data class Locked(val scope: String) : OpAvailability

    /** Hide it. */
    data object Unsupported : OpAvailability
}

/**
 * The client rule of 17.3. An op this client has no [OpTable] rule for is unsupported unless [Welcome.ops]
 * lists it. So is an op that the server knows and this device may use but that is missing from `ops` without
 * read-only mode: the table has no row for that case.
 */
fun Welcome.availabilityOf(op: String): OpAvailability {
    if (op in ops) return OpAvailability.Available
    val rule = OpTable.rules[op]
    if (knownOps == null || op !in knownOps || rule == null) return OpAvailability.Unsupported
    if (rule.scope != null && rule.scope !in scopes) return OpAvailability.Locked(rule.scope)
    return if (readOnly) OpAvailability.ReadOnly else OpAvailability.Unsupported
}
