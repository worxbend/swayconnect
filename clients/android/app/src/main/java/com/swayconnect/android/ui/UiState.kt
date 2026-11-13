package com.swayconnect.android.ui

import com.swayconnect.protocol.Base64Url
import com.swayconnect.protocol.Desktop
import com.swayconnect.protocol.FINGERPRINT_BYTES
import com.swayconnect.protocol.Op
import com.swayconnect.protocol.PairingUri
import com.swayconnect.protocol.PairingUriException
import com.swayconnect.protocol.Profile
import com.swayconnect.protocol.Workspace
import com.swayconnect.protocol.parsePairingUri
import com.swayconnect.protocol.replaceUnsafe

enum class Screen { Loading, Pair, Desktop }

/** The connection states of docs/DESIGN.md 8.4. */
enum class ConnectionStatus { Disconnected, Connecting, Authenticating, Ready, Backoff }

enum class PairPhase { Editing, Connecting, AwaitingConfirmation }

/** What a pairing link says about the computer. It holds nothing secret. */
data class PairTarget(val host: String, val port: Int, val shortFingerprint: String, val name: String?) {
    companion object {
        fun of(uri: PairingUri) = PairTarget(uri.host, uri.port, shortFingerprint(uri.fingerprint), uri.name)
    }
}

data class PairForm(
    val deviceName: String,
    /** The pasted link. It holds the pairing secret: it lives in memory only and is dropped when an attempt ends. */
    val link: String = "",
    val target: PairTarget? = null,
    val error: String? = null,
    val phase: PairPhase = PairPhase.Editing,
    val confirmCode: String? = null,
    val confirmingReplace: Boolean = false,
) {
    // The generated toString would print the link, and with it the pairing secret.
    override fun toString(): String = "PairForm(phase=$phase)"
}

/** For every text that is not a whole link. The parser's own message names URI parameters and stays out of the UI. */
const val NOT_A_PAIRING_LINK = "This is not a complete pairing link. Copy the whole link from the computer again."

/** Parses [text] as a pairing link and fills the form from it. Nothing connects here. */
fun PairForm.withLink(text: String): PairForm {
    val link = text.trim()
    if (link.isEmpty()) return copy(link = text, target = null, error = null)
    return try {
        copy(link = text, target = PairTarget.of(parsePairingUri(link)), error = null)
    } catch (e: PairingUriException) {
        copy(link = text, target = null, error = NOT_A_PAIRING_LINK)
    }
}

/** The paired computer, from the stored profile. */
data class ServerSummary(val name: String, val shortFingerprint: String) {
    companion object {
        fun of(profile: Profile): ServerSummary {
            val fingerprint = Base64Url.decode(profile.fp, FINGERPRINT_BYTES)
            return ServerSummary(replaceUnsafe(profile.serverName), fingerprint?.let(::shortFingerprint) ?: "unknown")
        }
    }
}

/** What `welcome` granted on the current connection. */
data class Granted(val ops: Set<String>, val scopes: List<String>, val readOnly: Boolean)

data class UiState(
    val pair: PairForm,
    val screen: Screen = Screen.Loading,
    val server: ServerSummary? = null,
    val connection: ConnectionStatus = ConnectionStatus.Disconnected,
    /** Why the last connection ended. Shown under the banner until a connection is ready again. */
    val connectionProblem: String? = null,
    val granted: Granted? = null,
    val desktop: Desktop? = null,
    val selectedWorkspaceId: Long? = null,
    /** The workspace or window whose action waits for its answer. Both kinds share one id space. */
    val pendingTargetId: Long? = null,
    /** One line about the last action. */
    val notice: String? = null,
    /** The computer revoked this device, or no longer knows it. */
    val unpaired: Boolean = false,
)

/** Applies a state from the server. The selection follows the focus when the focus moves. */
fun UiState.withDesktop(next: Desktop?): UiState {
    val focusedBefore = desktop?.workspaces?.firstOrNull { it.focused }?.id
    val focusedNow = next?.workspaces?.firstOrNull { it.focused }?.id
    val stillThere = next?.workspaces?.any { it.id == selectedWorkspaceId } == true
    val selected =
        when {
            focusedNow != focusedBefore && focusedNow != null -> focusedNow
            stillThere -> selectedWorkspaceId
            else -> focusedNow ?: next?.workspaces?.firstOrNull()?.id
        }
    // The window list uses the id as its key, and Compose throws on a key that comes twice. Sway ids
    // are unique; a broken server must not be able to crash the app.
    val shown = next?.copy(windows = next.windows.distinctBy { it.id })
    return copy(desktop = shown, selectedWorkspaceId = selected, pendingTargetId = null)
}

private val ACTION_OPS = setOf(Op.WORKSPACE_FOCUS, Op.WINDOW_FOCUS, Op.WINDOW_FULLSCREEN)

/** Why a tap does nothing: the connection offers no action. Null when it offers one, or before `welcome`. */
fun UiState.controlHint(): String? {
    val granted = granted ?: return null
    return when {
        granted.ops.any { it in ACTION_OPS } -> null
        granted.readOnly -> "Read-only: this phone can look, not control."
        else -> "This device may not control the computer."
    }
}

fun UiState.bannerText(): String =
    when (connection) {
        ConnectionStatus.Disconnected -> "Not connected"
        ConnectionStatus.Connecting -> "Connecting"
        ConnectionStatus.Authenticating -> "Authenticating"
        ConnectionStatus.Ready -> if (desktop?.sway?.online == false) "Sway offline" else "Ready"
        ConnectionStatus.Backoff -> "Reconnecting"
    }

/** Chip text. The name comes from sway: it is shown as plain text with unsafe characters replaced. */
fun workspaceLabel(workspace: Workspace): String =
    listOfNotNull(
        replaceUnsafe(workspace.name),
        "focused".takeIf { workspace.focused },
        "visible".takeIf { workspace.visible && !workspace.focused },
        "urgent".takeIf { workspace.urgent },
    ).joinToString(" · ")

private const val SHORT_FINGERPRINT_BYTES = 8

/** The first 8 bytes as hex in groups of four digits, for example `aa51 08ad 019a de39`. */
fun shortFingerprint(fingerprint: ByteArray): String =
    fingerprint.take(SHORT_FINGERPRINT_BYTES)
        .joinToString("") { byte -> (byte.toInt() and 0xFF).toString(16).padStart(2, '0') }
        .chunked(4)
        .joinToString(" ")
