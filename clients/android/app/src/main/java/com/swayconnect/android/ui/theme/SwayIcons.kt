package com.swayconnect.android.ui.theme

import androidx.compose.runtime.Immutable
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.PathFillType
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.StrokeJoin
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.graphics.vector.addPathNodes
import androidx.compose.ui.unit.dp
import kotlin.math.hypot

/**
 * One glyph in two forms (docs/UX.md 3.4). Draw with `Icon(imageVector = icon.outline, ...)`; the nav uses
 * [filled] for the selected area. A glyph made only of lines has no filled form, so both are the same vector.
 */
@Immutable
data class SwayIcon(val outline: ImageVector, val filled: ImageVector)

/**
 * The app's glyphs as Kotlin `ImageVector` builders: no XML, no resource ids, no icon library.
 * Drawn on a 24 x 24 grid with 2-unit round strokes, the look of Material Symbols Rounded at weight 400.
 *
 * Each glyph has a closed `body`, open `stroke` paths, and `line`, `dot` and `ring` details. The outline form
 * strokes the body. The filled form fills it, and every detail inside the body becomes a hole (even-odd fill).
 */
object SwayIcons {
    val Desktop = glyph("desktop") {
        body("M5.5 4h13A2.5 2.5 0 0 1 21 6.5v7a2.5 2.5 0 0 1-2.5 2.5h-13A2.5 2.5 0 0 1 3 13.5v-7A2.5 2.5 0 0 1 5.5 4z")
        stroke("M12 16v4M8.5 20h7")
    }
    val Audio = glyph("audio") {
        body("M9 3h6a3 3 0 0 1 3 3v12a3 3 0 0 1-3 3H9a3 3 0 0 1-3-3V6a3 3 0 0 1 3-3z")
        dot(12f, 7.5f, 1.25f)
        ring(12f, 14.5f, 2.75f)
    }
    val Launch = glyph("launch") {
        body(
            " M6 4h2.5a2 2 0 0 1 2 2v2.5a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2z" +
                " M15.5 4H18a2 2 0 0 1 2 2v2.5a2 2 0 0 1-2 2h-2.5a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2z" +
                " M6 13.5h2.5a2 2 0 0 1 2 2V18a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2v-2.5a2 2 0 0 1 2-2z" +
                " M15.5 13.5H18a2 2 0 0 1 2 2V18a2 2 0 0 1-2 2h-2.5a2 2 0 0 1-2-2v-2.5a2 2 0 0 1 2-2z",
        )
    }
    val Keyboard = glyph("keyboard") {
        body("M5 5.5h14a2.5 2.5 0 0 1 2.5 2.5v8a2.5 2.5 0 0 1-2.5 2.5H5A2.5 2.5 0 0 1 2.5 16V8A2.5 2.5 0 0 1 5 5.5z")
        for (y in listOf(9.5f, 12.5f)) {
            for (x in listOf(7.5f, 10.5f, 13.5f, 16.5f)) dot(x, y, 1f)
        }
        line(9f, 15.5f, 15f, 15.5f)
    }
    val More = glyph("more") {
        body(circle(5f, 12f, 1.75f) + circle(12f, 12f, 1.75f) + circle(19f, 12f, 1.75f))
    }
    val Plus = glyph("plus") { stroke("M12 5v14M5 12h14") }
    val Swap = glyph("swap") { stroke("M7.5 4 4 7.5 7.5 11M4 7.5h14M16.5 13l3.5 3.5-3.5 3.5M20 16.5H6") }
    val Visibility = glyph("visibility") {
        body("M2.5 12C5 7.5 8.5 5 12 5s7 2.5 9.5 7c-2.5 4.5-6 7-9.5 7s-7-2.5-9.5-7z")
        ring(12f, 12f, 3f)
    }
    val Map = glyph("map") {
        body("M5.5 4h13A2.5 2.5 0 0 1 21 6.5v11a2.5 2.5 0 0 1-2.5 2.5h-13A2.5 2.5 0 0 1 3 17.5v-11A2.5 2.5 0 0 1 5.5 4z")
        line(10f, 4f, 10f, 20f)
        line(10f, 12f, 21f, 12f)
    }
    val List = glyph("list") {
        stroke("M9.5 6.5H20M9.5 12H20M9.5 17.5H20")
        dot(4.75f, 6.5f, 1.4f)
        dot(4.75f, 12f, 1.4f)
        dot(4.75f, 17.5f, 1.4f)
    }
    val Fullscreen = glyph("fullscreen") {
        stroke(
            " M4 9V5.5A1.5 1.5 0 0 1 5.5 4H9M15 4h3.5A1.5 1.5 0 0 1 20 5.5V9" +
                " M20 15v3.5a1.5 1.5 0 0 1-1.5 1.5H15M9 20H5.5A1.5 1.5 0 0 1 4 18.5V15",
        )
    }
    val Float = glyph("float") {
        body("M10 9h8a2 2 0 0 1 2 2v7a2 2 0 0 1-2 2h-8a2 2 0 0 1-2-2v-7a2 2 0 0 1 2-2z")
        stroke("M4 15V6a2 2 0 0 1 2-2h9")
    }
    val Move = glyph("move") {
        stroke("M12 3v18M3 12h18M9 6l3-3 3 3M9 18l3 3 3-3M6 9l-3 3 3 3M18 9l3 3-3 3")
    }
    val Close = glyph("close") { stroke("M6 6l12 12M18 6 6 18") }
    val Lock = glyph("lock") {
        body(LOCK_BODY)
        stroke("M8 10.5V8a4 4 0 0 1 8 0v2.5")
        dot(12f, 15.5f, 1.5f)
    }
    val LockOpen = glyph("lock_open") {
        body(LOCK_BODY)
        stroke("M8 10.5V8a4 4 0 0 1 7.75-1.4")
        dot(12f, 15.5f, 1.5f)
    }
    val Mic = glyph("mic") {
        body(MIC_BODY)
        stroke(MIC_STAND)
    }
    val MicOff = glyph("mic_off") {
        body(MIC_BODY)
        stroke("$MIC_STAND M4 4l16 16")
    }
    val Volume = glyph("volume") {
        body(SPEAKER_BODY)
        stroke("M16 9a4 4 0 0 1 0 6M18.5 6.5a7.5 7.5 0 0 1 0 11")
    }
    val VolumeOff = glyph("volume_off") {
        body(SPEAKER_BODY)
        stroke("M16.5 9.5l4.5 5M21 9.5l-4.5 5")
    }
    val Coffee = glyph("coffee") {
        body("M4 9h12v5a5 5 0 0 1-5 5H9a5 5 0 0 1-5-5z")
        stroke("M16 10.5h1.5a2.5 2.5 0 0 1 0 5H16M8 3.5v2M12 3.5v2")
    }
    val Play = glyph("play") {
        body("M8 5.5v13a1 1 0 0 0 1.5.9l10.2-6.5a1 1 0 0 0 0-1.7L9.5 4.6A1 1 0 0 0 8 5.5z")
    }
    val Pause = glyph("pause") {
        body(
            " M7.5 5h1a1.5 1.5 0 0 1 1.5 1.5v11a1.5 1.5 0 0 1-1.5 1.5h-1A1.5 1.5 0 0 1 6 17.5v-11A1.5 1.5 0 0 1 7.5 5z" +
                " M15.5 5h1A1.5 1.5 0 0 1 18 6.5v11a1.5 1.5 0 0 1-1.5 1.5h-1a1.5 1.5 0 0 1-1.5-1.5v-11A1.5 1.5 0 0 1 15.5 5z",
        )
    }
    val Next = glyph("next") {
        body("M5 6.2v11.6a1 1 0 0 0 1.6.8l8-5.8a1 1 0 0 0 0-1.6l-8-5.8A1 1 0 0 0 5 6.2z")
        stroke("M19 5.5v13")
    }
    val Previous = glyph("previous") {
        body("M19 6.2v11.6a1 1 0 0 1-1.6.8l-8-5.8a1 1 0 0 1 0-1.6l8-5.8A1 1 0 0 1 19 6.2z")
        stroke("M5 5.5v13")
    }
    val Search = glyph("search") {
        body(circle(10.5f, 10.5f, 6.5f))
        stroke("M15.5 15.5 20 20")
    }
    val Terminal = glyph("terminal") {
        body("M5.5 4h13A2.5 2.5 0 0 1 21 6.5v11a2.5 2.5 0 0 1-2.5 2.5h-13A2.5 2.5 0 0 1 3 17.5v-11A2.5 2.5 0 0 1 5.5 4z")
        // Two capsules would overlap at the elbow and even-odd would fill it again, so the chevron has its own hole.
        cut(
            stroke = "M7.5 9l3 3-3 3",
            hole = "M6.79 9.71L9.09 12L6.79 14.29A1 1 0 0 0 8.21 15.71L11.91 12L8.21 8.29A1 1 0 0 0 6.79 9.71z",
        )
        line(12.5f, 15f, 16.5f, 15f)
    }
    val Shield = glyph("shield") {
        body("M12 3l7 3v5.5c0 4.5-3 8-7 9.5-4-1.5-7-5-7-9.5V6z")
    }
    val Scratchpad = glyph("scratchpad") {
        body("M6.5 10h11A2.5 2.5 0 0 1 20 12.5v5a2.5 2.5 0 0 1-2.5 2.5h-11A2.5 2.5 0 0 1 4 17.5v-5A2.5 2.5 0 0 1 6.5 10z")
        stroke("M6 6.5h12M8.5 3.5h7")
    }
    val Moon = glyph("moon") {
        body("M20 14.5A8 8 0 1 1 9.5 4a6.5 6.5 0 0 0 10.5 10.5z")
    }
    val Warning = glyph("warning") {
        body("M10.3 4.5a2 2 0 0 1 3.4 0l7.4 12.9a2 2 0 0 1-1.7 3H4.6a2 2 0 0 1-1.7-3z")
        line(12f, 9.5f, 12f, 13.5f)
        dot(12f, 16.75f, 1.2f)
    }
    val Check = glyph("check") { stroke("M5 12.5l4.5 4.5L19 7.5") }

    /** Every glyph, for tests and a preview. */
    val all: kotlin.collections.List<SwayIcon> = listOf(
        Desktop, Audio, Launch, Keyboard, More, Plus, Swap, Visibility, Map, List, Fullscreen, Float, Move, Close,
        Lock, LockOpen, Mic, MicOff, Volume, VolumeOff, Coffee, Play, Pause, Next, Previous, Search, Terminal,
        Shield, Scratchpad, Moon, Warning, Check,
    )
}

private const val LOCK_BODY =
    " M7.5 10.5h9A2.5 2.5 0 0 1 19 13v5a2.5 2.5 0 0 1-2.5 2.5h-9A2.5 2.5 0 0 1 5 18v-5a2.5 2.5 0 0 1 2.5-2.5z"
private const val MIC_BODY = "M9 6a3 3 0 0 1 6 0v5a3 3 0 0 1-6 0z"
private const val MIC_STAND = "M5.5 11a6.5 6.5 0 0 0 13 0M12 17.5V21"
private const val SPEAKER_BODY =
    " M3 10a1.5 1.5 0 0 1 1.5-1.5H7l4.3-3.6A1 1 0 0 1 13 5.7v12.6a1 1 0 0 1-1.7.8L7 15.5H4.5A1.5 1.5 0 0 1 3 14z"

private const val GRID = 24f
private const val STROKE = 2f
private const val HALF = STROKE / 2

private fun circle(cx: Float, cy: Float, r: Float): String =
    "M${cx - r} ${cy}a$r $r 0 1 0 ${2 * r} 0a$r $r 0 1 0 ${-2 * r} 0z"

/** A line as a closed capsule of the stroke width, so the filled form can cut it out of the body. */
private fun capsule(x1: Float, y1: Float, x2: Float, y2: Float): String {
    val length = hypot(x2 - x1, y2 - y1)
    val nx = -(y2 - y1) / length * HALF
    val ny = (x2 - x1) / length * HALF
    return "M${x1 + nx} ${y1 + ny}L${x2 + nx} ${y2 + ny}A$HALF $HALF 0 0 0 ${x2 - nx} ${y2 - ny}" +
        "L${x1 - nx} ${y1 - ny}A$HALF $HALF 0 0 0 ${x1 + nx} ${y1 + ny}z"
}

private class GlyphSpec {
    var body: String? = null
    val strokes = StringBuilder()
    val lines = StringBuilder()
    val holes = StringBuilder()
    val dots = StringBuilder()
    val rings = StringBuilder()

    fun body(path: String) {
        body = path
    }

    fun stroke(path: String) {
        strokes.append(path)
    }

    fun line(x1: Float, y1: Float, x2: Float, y2: Float) {
        require(x1 != x2 || y1 != y2) { "a line needs two points" }
        lines.append("M$x1 ${y1}L$x2 $y2")
        holes.append(capsule(x1, y1, x2, y2))
    }

    /** A detail with a hand-made hole outline, for shapes that [line] cannot cut cleanly. */
    fun cut(stroke: String, hole: String) {
        lines.append(stroke)
        holes.append(hole)
    }

    fun dot(cx: Float, cy: Float, r: Float) {
        dots.append(circle(cx, cy, r))
        holes.append(circle(cx, cy, r))
    }

    fun ring(cx: Float, cy: Float, r: Float) {
        rings.append(circle(cx, cy, r))
        holes.append(circle(cx, cy, r + HALF)).append(circle(cx, cy, r - HALF))
    }
}

private fun glyph(name: String, build: GlyphSpec.() -> Unit): SwayIcon {
    val spec = GlyphSpec().apply(build)
    val body = spec.body
    val outline = vector(name) {
        body?.let { stroked(it) }
        stroked(spec.strokes.toString() + spec.lines + spec.rings)
        filled(spec.dots.toString())
    }
    if (body == null) return SwayIcon(outline, outline)
    val filled = vector("${name}_filled") {
        filled(body + spec.holes)
        // The outer stroke keeps both forms the same size; holes stop at its inner edge.
        stroked(body)
        stroked(spec.strokes.toString())
    }
    return SwayIcon(outline, filled)
}

private fun vector(name: String, paths: ImageVector.Builder.() -> Unit): ImageVector =
    ImageVector.Builder(name, GRID.dp, GRID.dp, GRID, GRID).apply(paths).build()

private val Ink = SolidColor(Color.Black)

private fun ImageVector.Builder.stroked(path: String) {
    if (path.isEmpty()) return
    addPath(
        pathData = addPathNodes(path),
        stroke = Ink,
        strokeLineWidth = STROKE,
        strokeLineCap = StrokeCap.Round,
        strokeLineJoin = StrokeJoin.Round,
    )
}

private fun ImageVector.Builder.filled(path: String) {
    if (path.isEmpty()) return
    addPath(pathData = addPathNodes(path), pathFillType = PathFillType.EvenOdd, fill = Ink)
}
