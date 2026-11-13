package com.swayconnect.android.ui.common

import com.swayconnect.protocol.Window

/** A container that shows one child at a time. */
enum class GroupKind { Tabbed, Stacked }

/** A window's place in its tabbed or stacked container: [position] of [count], from 1. */
data class GroupPosition(val kind: GroupKind, val position: Int, val count: Int) {
    init {
        require(position in 1..count) { "position $position is outside 1..$count" }
    }
}

/** A state pill of a window card. */
sealed interface WindowMark {
    data object Focused : WindowMark

    data object Urgent : WindowMark

    data object Fullscreen : WindowMark

    data object Floating : WindowMark

    data class InGroup(val group: GroupPosition) : WindowMark
}

/** The pills of a window card, in the order of docs/UX.md 5.2: Focused, Urgent, Fullscreen, Floating, group. */
fun windowMarks(window: Window, group: GroupPosition?): List<WindowMark> = buildList {
    if (window.focused) add(WindowMark.Focused)
    if (window.urgent) add(WindowMark.Urgent)
    if (window.fullscreen) add(WindowMark.Fullscreen)
    if (window.floating) add(WindowMark.Floating)
    if (group != null) add(WindowMark.InGroup(group))
}
