package com.swayconnect.android.ui.state

import com.swayconnect.protocol.Base64Url
import com.swayconnect.protocol.Desktop
import com.swayconnect.protocol.FINGERPRINT_BYTES
import com.swayconnect.protocol.Op
import com.swayconnect.protocol.OpAvailability
import com.swayconnect.protocol.PairingUri
import com.swayconnect.protocol.PairingUriException
import com.swayconnect.protocol.Profile
import com.swayconnect.protocol.Workspace
import com.swayconnect.protocol.parsePairingUri
import com.swayconnect.protocol.replaceUnsafe

enum class Screen { Loading, Pair, Desktop }

/** The connection states of docs/DESIGN.md 8.4. */
enum class ConnectionStatus { Disconnected, Connecting, Authenticating, Ready, Backoff }

/** [Confirmed]: the computer said yes; the pair screen shows "What this phone can do" until Done (UX 11 step 4). */
enum class PairPhase { Editing, Connecting, AwaitingConfirmation, Confirmed }

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

/**
 * The paired computer, from the stored profile. [devicePrefix] is the first 4 characters of this phone's device id,
 * as `swayconnectd devices grant` takes it (docs/UX.md 10.4); null when the stored id is not plain base64url.
 */
data class ServerSummary(val name: String, val shortFingerprint: String, val devicePrefix: String? = null) {
    companion object {
        private const val DEVICE_PREFIX_CHARS = 4
        private val DEVICE_ID = Regex("[A-Za-z0-9_-]{4,}")

        fun of(profile: Profile): ServerSummary {
            val fingerprint = Base64Url.decode(profile.fp, FINGERPRINT_BYTES)
            val prefix = profile.deviceId.takeIf { DEVICE_ID.matches(it) }?.take(DEVICE_PREFIX_CHARS)
            return ServerSummary(replaceUnsafe(profile.serverName), fingerprint?.let(::shortFingerprint) ?: "unknown", prefix)
        }
    }
}

data class UiState(
    val pair: PairForm,
    val screen: Screen = Screen.Loading,
    val server: ServerSummary? = null,
    val connection: ConnectionStatus = ConnectionStatus.Disconnected,
    /** Why the last connection ended. Shown under the banner until a connection is ready again. */
    val connectionProblem: String? = null,
    val granted: Granted? = null,
    /** The newest state, live or not: see [desktopLive]. */
    val desktop: Desktop? = null,
    /** False while [desktop] is the cached snapshot or the last state of a closed connection (UX 5.9: dimmed, no taps). */
    val desktopLive: Boolean = false,
    /** Wall-clock time of the newest state in [desktop], for "last seen 12 s ago". */
    val lastSeenAtMs: Long? = null,
    /** The workspace shown on the phone (the pager page). The source of truth for the page index (UX 5.5). */
    val selectedWorkspaceId: Long? = null,
    /** The workspace or window whose action waits for its answer. Both kinds share one id space. */
    val pendingTargetId: Long? = null,
    /** One line about the last action. */
    val notice: String? = null,
    /** The computer revoked this device, or no longer knows it. */
    val unpaired: Boolean = false,
    /** The name of the computer that revoked this device, for the full-screen "no longer paired with" page. */
    val lostServerName: String? = null,
    /** The computer presented another certificate than the paired one: no auto-retry (docs/UX.md 16). */
    val wrongKey: Boolean = false,
    val area: Area = Area.Desktop,
    val sheet: Sheet? = null,
    val viewMode: ViewMode = ViewMode.Map,
    val drive: DriveMode = DriveMode.Drive,
    /** Drive was refused with `rate_limited` or `read_only`: the status line shows "Drive paused" until the UI clears it. */
    val drivePaused: Boolean = false,
    /** "Computer moved to 7 →": the computer's focused workspace when follow focus did not scroll there. */
    val followFocusChip: Long? = null,
    /** "Larger text · Show list" was used or closed; in memory until a settings store exists (UX 15). */
    val listHintDismissed: Boolean = false,
)

/** The pages of the pager, one per workspace, in sway's order. */
val UiState.pages: List<Workspace>
    get() = desktop?.workspaces.orEmpty()

/** The page index of [UiState.selectedWorkspaceId], found again by id after every state; -1 without pages. */
val UiState.shownPageIndex: Int
    get() = pages.indexOfFirst { it.id == selectedWorkspaceId }

/** True when a user-initiated settle sends `workspace.focus` (UX 5.5 Drive mode). "Drive paused" browses only. */
val UiState.isDriving: Boolean
    get() = drive == DriveMode.Drive && !drivePaused && desktopLive &&
        granted?.availabilityOf(Op.WORKSPACE_FOCUS) == OpAvailability.Available

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
