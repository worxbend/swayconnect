package com.swayconnect.android.ui.more

import com.swayconnect.android.ui.state.Granted
import com.swayconnect.android.ui.state.PairForm
import com.swayconnect.android.ui.state.ServerTarget
import com.swayconnect.android.ui.state.UiState
import com.swayconnect.android.ui.state.durationChoicesS
import com.swayconnect.android.ui.state.oneHourFromNowS
import com.swayconnect.android.ui.state.withSession
import com.swayconnect.android.ui.state.withSessionStale
import com.swayconnect.protocol.Op
import com.swayconnect.protocol.OpAvailability
import com.swayconnect.protocol.SessionState
import com.swayconnect.protocol.SessionStateSerializer
import com.swayconnect.protocol.Wire
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.File

/** The keep-awake tile of docs/UX.md 10.2, one test per table row, from protocol/vectors/v1/ext/session.json. */
class MoreModelTest {
    private val received = 10_000L

    @Test
    fun `off - tap turns on, long press chooses a duration`() {
        val tile = tileOf("02-off")
        assertEquals(KeepAwakeFace.Off, tile.face)
        assertEquals(KeepAwakeAction.TurnOn, tile.tap())
        assertEquals(KeepAwakeAction.ChooseDuration, tile.longPress())
        assertFalse(tile.computerLocked)
        assertFalse(tile.windowKeepsAwake)
    }

    @Test
    fun `on from this phone counts down - tap turns off`() {
        val state = stateOf("01-spec-example")
        assertEquals(KeepAwakeFace.On(fromThisPhone = true, leftMs = 3_600_000), keepAwakeTile(state)!!.face)
        val later = keepAwakeTile(state.copy(clockMs = received + 8 * 60_000))!!
        assertEquals(KeepAwakeFace.On(fromThisPhone = true, leftMs = 52 * 60_000), later.face)
        assertEquals(KeepAwakeAction.TurnOff, later.tap())
        assertEquals(KeepAwakeAction.ChooseDuration, later.longPress())
    }

    @Test
    fun `on from another device - any device may turn it off`() {
        val tile = tileOf("03-on-other-device")
        assertEquals(KeepAwakeFace.On(fromThisPhone = false, leftMs = 2_700_000), tile.face)
        assertEquals(KeepAwakeAction.TurnOff, tile.tap())
        assertEquals(KeepAwakeAction.ChooseDuration, tile.longPress())
    }

    @Test
    fun `cooldown - tap refuses without sending, no long press`() {
        val tile = tileOf("04-cooldown")
        assertEquals(KeepAwakeFace.Cooldown(540_000), tile.face)
        assertEquals(KeepAwakeAction.Refuse(540_000), tile.tap())
        assertEquals(KeepAwakeAction.None, tile.longPress())
        assertEquals(9, minutesUp(540_000))
    }

    @Test
    fun `computer locked keeps the keep-awake state and adds its line`() {
        val tile = tileOf("05-locked")
        assertTrue(tile.computerLocked)
        assertEquals(KeepAwakeFace.On(fromThisPhone = true, leftMs = 1_800_000), tile.face)
        assertEquals(KeepAwakeAction.TurnOff, tile.tap())
        assertFalse(tileOf("06-lock-state-unknown").computerLocked)
    }

    @Test
    fun `a window that inhibits adds its line`() {
        assertTrue(tileOf("07-a-window-inhibits").windowKeepsAwake)
    }

    @Test
    fun `scope missing - tap and long press open the Locked sheet`() {
        val locked = stateOf("01-spec-example", OpAvailability.Locked("idle.inhibit"))
        val tile = keepAwakeTile(locked)!!
        assertEquals(KeepAwakeAction.ShowLocked("idle.inhibit"), tile.tap())
        assertEquals(KeepAwakeAction.ShowLocked("idle.inhibit"), tile.longPress())
    }

    @Test
    fun `read-only, stale, waiting and pending tiles take no tap`() {
        val readOnly = keepAwakeTile(stateOf("02-off", OpAvailability.ReadOnly))!!
        val stale = keepAwakeTile(stateOf("02-off").withSessionStale())!!
        val cached = keepAwakeTile(stateOf("02-off").copy(desktopLive = false))!!
        val pending = keepAwakeTile(stateOf("02-off").copy(pendingTargetId = ServerTarget.KEEP_AWAKE))!!
        val waiting = keepAwakeTile(base(OpAvailability.Available))!!
        assertEquals(KeepAwakeFace.Waiting, waiting.face)
        assertTrue(pending.pending)
        for (tile in listOf(readOnly, stale, cached, pending, waiting)) {
            assertEquals(KeepAwakeAction.None, tile.tap())
            assertEquals(KeepAwakeAction.None, tile.longPress())
        }
        assertFalse(stale.live)
    }

    @Test
    fun `no tile without the op or before welcome`() {
        assertNull(keepAwakeTile(stateOf("02-off", OpAvailability.Unsupported)))
        assertNull(keepAwakeTile(UiState(pair = PairForm(deviceName = "Pixel"))))
    }

    @Test
    fun `duration choices stop at max_duration_s, and 1 h from now is capped by it`() {
        val keepAwake = sessionOf("01-spec-example").keepAwake
        assertEquals(listOf(900L, 1_800L, 3_600L, 7_200L), keepAwake.durationChoicesS())
        assertEquals(3_600L, keepAwake.oneHourFromNowS())
        val short = keepAwake.copy(maxDurationS = 1_800)
        assertEquals(listOf(900L, 1_800L), short.durationChoicesS())
        assertEquals(1_800L, short.oneHourFromNowS())
    }

    @Test
    fun `durations read as whole hours, else minutes rounded up`() {
        assertEquals(DurationWords.Hours(2), DurationWords.of(7_200))
        assertEquals(DurationWords.Hours(4), DurationWords.of(14_400))
        assertEquals(DurationWords.Hours(1), DurationWords.of(3_600))
        assertEquals(DurationWords.Minutes(90), DurationWords.of(5_400))
        assertEquals(DurationWords.Minutes(15), DurationWords.of(900))
        assertEquals(DurationWords.Minutes(2), DurationWords.of(61))
        assertEquals(DurationWords.Minutes(0), DurationWords.of(0))
        assertEquals(DurationWords.Minutes(0), DurationWords.of(-5))
    }

    @Test
    fun `minutes round up and never go below zero`() {
        assertEquals(1, minutesUp(1))
        assertEquals(1, minutesUp(60_000))
        assertEquals(2, minutesUp(60_001))
        assertEquals(0, minutesUp(0))
        assertEquals(0, minutesUp(-1))
    }

    private fun tileOf(name: String): KeepAwakeTile = keepAwakeTile(stateOf(name))!!

    private fun stateOf(name: String, access: OpAvailability = OpAvailability.Available): UiState =
        base(access).withSession(sessionOf(name), received)

    private fun base(access: OpAvailability): UiState =
        UiState(
            pair = PairForm(deviceName = "Pixel"),
            desktopLive = true,
            granted = Granted(
                ops = if (access == OpAvailability.Available) setOf(Op.KEEP_AWAKE_SET) else emptySet(),
                scopes = emptyList(),
                readOnly = access == OpAvailability.ReadOnly,
                availability = mapOf(Op.KEEP_AWAKE_SET to access),
            ),
        )

    private companion object {
        private val states by lazy {
            val file = Wire.json.parseToJsonElement(vectorFile("ext/session.json").readText()).jsonObject
            file.getValue("states").jsonArray.associate { case ->
                case.jsonObject.getValue("name").jsonPrimitive.content to case.jsonObject.getValue("payload").jsonPrimitive.content
            }
        }

        /** The `data` of the `state.changed` frame named [name]. */
        fun sessionOf(name: String): SessionState {
            val frame = Wire.json.parseToJsonElement(states.getValue(name)).jsonObject
            val data = frame.getValue("data").jsonObject.getValue("data")
            return Wire.decode(SessionStateSerializer, data)
        }

        /** The shared vectors, found from the test's working directory up to the repository root. */
        fun vectorFile(path: String): File {
            var dir: File? = File(System.getProperty("user.dir")).absoluteFile
            while (dir != null) {
                val candidate = File(dir, "protocol/vectors/v1/$path")
                if (candidate.isFile) return candidate
                dir = dir.parentFile
            }
            error("protocol/vectors/v1/$path not found above ${System.getProperty("user.dir")}")
        }
    }

    @Test
    fun `the tile grid has two columns on a phone and one when narrow or at a large font scale`() {
        // A 360dp phone leaves 328dp between the 16dp gutters.
        assertEquals(2, tileColumns(widthDp = 328f, fontScale = 1f))
        assertEquals(2, tileColumns(widthDp = 328f, fontScale = 1.15f))
        assertEquals(1, tileColumns(widthDp = 328f, fontScale = 1.3f))
        // Two tiles of exactly 140dp still fit; one dp less does not.
        assertEquals(2, tileColumns(widthDp = 292f, fontScale = 1f))
        assertEquals(1, tileColumns(widthDp = 291f, fontScale = 1f))
        // Display size "largest" on a small phone: about 280dp of content.
        assertEquals(1, tileColumns(widthDp = 280f, fontScale = 1f))
    }
}
