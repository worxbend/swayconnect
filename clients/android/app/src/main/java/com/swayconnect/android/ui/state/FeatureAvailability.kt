package com.swayconnect.android.ui.state

import com.swayconnect.protocol.OpAvailability
import com.swayconnect.protocol.OpTable
import com.swayconnect.protocol.Welcome
import com.swayconnect.protocol.availabilityOf
import com.swayconnect.protocol.replaceUnsafe

/**
 * What `welcome` granted on the current connection, and how every operation is shown (`FeatureAvailability`,
 * docs/PROTOCOL.md 17.3). Granted scopes are [scopes] only and callable operations are [ops] only;
 * [knownScopes] is for the "locked" pills and never counts as granted.
 */
data class Granted(
    val ops: Set<String>,
    /** For display: unsafe characters replaced. */
    val scopes: List<String>,
    val readOnly: Boolean,
    /** Every scope the server knows, for display. Null from a server older than 17.3. */
    val knownScopes: List<String>? = null,
    /** The 17.3 state of each op the server or this client knows. Computed once, by [Welcome.availabilityOf]. */
    private val availability: Map<String, OpAvailability> = emptyMap(),
) {
    /** Available, read-only (show disabled), locked (show the scope) or unsupported (hide). */
    fun availabilityOf(op: String): OpAvailability =
        availability[op] ?: if (op in ops) OpAvailability.Available else OpAvailability.Unsupported

    companion object {
        fun of(welcome: Welcome): Granted {
            val named = OpTable.rules.keys + welcome.ops + welcome.knownOps.orEmpty()
            return Granted(
                ops = welcome.ops.toSet(),
                scopes = welcome.scopes.map(::replaceUnsafe),
                readOnly = welcome.readOnly,
                knownScopes = welcome.knownScopes?.map(::replaceUnsafe),
                availability = named.associateWith(welcome::availabilityOf),
            )
        }
    }
}
