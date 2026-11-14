package com.swayconnect.android.ui.pair

import androidx.annotation.StringRes
import com.swayconnect.android.R
import com.swayconnect.android.ui.state.Granted

/**
 * "What this phone can do" (docs/UX.md 11 step 4): [granted] from `welcome.scopes`, [locked] from
 * `welcome.known_scopes` minus the granted ones. A server older than PROTOCOL 17.3 sends no `known_scopes`,
 * so [locked] stays empty. `known_scopes` never counts as granted.
 */
data class PairCapabilities(val granted: List<String>, val locked: List<String>, val readOnly: Boolean) {
    /**
     * The phone can type or start apps, which can run code as the owner (DESIGN 11.8, D30): step 4 then says how
     * to remove it if the phone is lost. Read-only mode does not change what the pairing holds.
     */
    val canRunCode: Boolean get() = granted.any { it in CODE_SCOPES }

    companion object {
        /** Scopes with no operation in this protocol revision (PROTOCOL 17.2): a pill would promise a feature. */
        private val RESERVED = setOf("commands", "input.pointer")
        private val CODE_SCOPES = setOf("launch", "launch.risky", "input.keys", "input.text")

        fun of(granted: Granted): PairCapabilities {
            val mine = granted.scopes.filterNot { it in RESERVED }.distinct()
            val locked = granted.knownScopes.orEmpty().filterNot { it in RESERVED || it in mine }.distinct()
            return PairCapabilities(mine, locked, granted.readOnly)
        }
    }
}

/** The pill label of [scope], or null for a scope this app does not know (then the pill shows the name). */
@StringRes
fun scopeLabel(scope: String): Int? =
    when (scope) {
        "state.read" -> R.string.pair_scope_state_read
        "state.titles" -> R.string.pair_scope_state_titles
        "control" -> R.string.pair_scope_control
        "destructive" -> R.string.pair_scope_destructive
        "idle.inhibit" -> R.string.pair_scope_idle_inhibit
        "audio" -> R.string.pair_scope_audio
        "audio.capture" -> R.string.pair_scope_audio_capture
        "media" -> R.string.pair_scope_media
        "launch" -> R.string.pair_scope_launch
        "launch.risky" -> R.string.pair_scope_launch_risky
        "input.text" -> R.string.pair_scope_input_text
        "input.keys" -> R.string.pair_scope_input_keys
        else -> null
    }
