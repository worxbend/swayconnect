package com.swayconnect.protocol

/** The token buckets of docs/PROTOCOL.md 17.14. Each device has one bucket per class, shared by its connections. */
enum class RateClass { ACTIONS, CONTINUOUS, LAUNCH, INPUT, INPUT_TEXT, ICONS }

/**
 * The scope each operation needs (7.2, 17.2), whether read-only mode refuses it, and its rate class (17.14).
 * [rateClass] is null for the operations that are not rate limited.
 */
class OpRule(val scope: String?, val isAction: Boolean, val rateClass: RateClass?)

/** Every operation of protocol version 1 with its rule, including those of later milestones. */
object OpTable {
    private const val CONTROL = "control"

    val rules: Map<String, OpRule> = buildMap {
        put(Op.PING, OpRule(null, isAction = false, rateClass = null))
        put(Op.STATE_SUBSCRIBE, OpRule("state.read", isAction = false, rateClass = null))
        put(Op.ICON_GET, OpRule("state.read", isAction = false, rateClass = RateClass.ICONS))
        listOf(
            Op.WORKSPACE_FOCUS, Op.WINDOW_FOCUS, Op.WINDOW_FULLSCREEN, Op.WINDOW_MOVE, Op.WINDOW_FLOATING, Op.WINDOW_LAYOUT,
            Op.WINDOW_SCRATCHPAD, Op.SCRATCHPAD_SHOW, Op.SESSION_LOCK,
        ).forEach { put(it, action(CONTROL)) }
        put(Op.WINDOW_CLOSE, action("destructive"))
        put(Op.KEEP_AWAKE_SET, action("idle.inhibit"))
        put(Op.AUDIO_SET_VOLUME, action("audio", RateClass.CONTINUOUS))
        listOf(Op.AUDIO_SET_MUTE, Op.AUDIO_SET_DEFAULT).forEach { put(it, action("audio")) }
        MediaCommand.entries.forEach { put(it.op, action("media")) }
        listOf(Op.APP_LAUNCH, Op.LAUNCHER_OPEN).forEach { put(it, action("launch", RateClass.LAUNCH)) }
        put(Op.INPUT_TEXT, action("input.text", RateClass.INPUT_TEXT))
        put(Op.INPUT_KEY, action("input.keys", RateClass.INPUT))
    }

    private fun action(scope: String, rateClass: RateClass = RateClass.ACTIONS) = OpRule(scope, isAction = true, rateClass)
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
