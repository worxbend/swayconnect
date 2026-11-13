//! Pure function: one sway workspace node to its `Layout` (`docs/PROTOCOL.md` 17.4.3).
//!
//! Geometry is trusted to be numbers only; nothing here reads a string sway got from an app.

use swayconnect_protocol::limits::{MAX_BASIS_POINTS, MAX_INT, MAX_LAYOUT_DEPTH};
use swayconnect_protocol::{BasisRect, Container, ContainerKind, Layout, Rect, Tile};
use swayconnect_sway::{Node, Rect as SwayRect};

use crate::model::is_window;

/// The area of `node` as a protocol `Rect`. `None` when sway gave none, or one with a
/// negative size or a corner outside `sint`.
pub fn protocol_rect(node: &Node) -> Option<Rect> {
    let rect = node.rect?;
    let is_sint = |value: i64| value.unsigned_abs() <= MAX_INT;
    if !is_sint(rect.x) || !is_sint(rect.y) {
        return None;
    }
    Some(Rect {
        x: rect.x,
        y: rect.y,
        w: u64::try_from(rect.width).ok()?,
        h: u64::try_from(rect.height).ok()?,
    })
}

/// The layout of `workspace`. `None` when the workspace has no area to measure against.
pub fn workspace_layout(workspace: &Node) -> Option<Layout> {
    let area = protocol_rect(workspace).filter(|area| area.w > 0 && area.h > 0)?;
    let floating = workspace
        .floating_nodes
        .iter()
        .filter_map(|node| tile(node, area))
        .map(capped)
        .collect();
    Some(Layout {
        size: [area.w, area.h],
        fullscreen: fullscreen_window(workspace),
        tiled: tiled_root(workspace, area).map(capped),
        floating,
    })
}

/// Rule 1: the root stays a container. A split whose only child is a container hands the root
/// to that child, so sway's wrapper containers do not show.
fn tiled_root(workspace: &Node, area: Rect) -> Option<Tile> {
    let mut root = workspace;
    while let [only] = root.nodes.as_slice() {
        if is_window(only) || kind(root).shows_one_child() {
            break;
        }
        root = only;
    }
    container(root, area).map(Tile::Container)
}

/// A window tile, or a container tile with split containers of one child replaced by it.
/// `None` for a container with no window below it.
fn tile(node: &Node, area: Rect) -> Option<Tile> {
    if is_window(node) {
        return Some(Tile::Window {
            id: node.id,
            r: basis_rect(node.rect, area),
        });
    }
    let mut container = container(node, area)?;
    if container.kind.shows_one_child() || container.children.len() > 1 {
        Some(Tile::Container(container))
    } else {
        container.children.pop()
    }
}

/// `node` with its tiled children; `None` when none of them holds a window.
fn container(node: &Node, area: Rect) -> Option<Container> {
    let mut kept_ids = Vec::with_capacity(node.nodes.len());
    let mut children = Vec::with_capacity(node.nodes.len());
    for child in &node.nodes {
        if let Some(tile) = tile(child, area) {
            kept_ids.push(child.id);
            children.push(tile);
        }
    }
    if children.is_empty() {
        return None;
    }
    let kind = kind(node);
    // Rule 6: the first id of sway's focus list that is still a child.
    let active = kind.shows_one_child().then(|| {
        node.focus
            .iter()
            .find_map(|id| kept_ids.iter().position(|kept| kept == id))
    });
    Some(Container {
        kind,
        r: basis_rect(node.rect, area),
        children,
        active: active.flatten(),
    })
}

/// Sway's `layout` text; anything that is not a known container kind counts as `h`.
fn kind(node: &Node) -> ContainerKind {
    match node.layout.as_deref() {
        Some("splitv") => ContainerKind::SplitV,
        Some("tabbed") => ContainerKind::Tabbed,
        Some("stacked") => ContainerKind::Stacked,
        _ => ContainerKind::SplitH,
    }
}

/// Rule 4: only a window's own mode counts; the workspace's `fullscreen_mode` and a fullscreen
/// container are ignored.
fn fullscreen_window(workspace: &Node) -> Option<u64> {
    let mut children = workspace.nodes.iter().chain(&workspace.floating_nodes);
    children.find_map(|child| {
        if is_window(child) {
            child
                .fullscreen_mode
                .is_some_and(|mode| mode != 0)
                .then_some(child.id)
        } else {
            fullscreen_window(child)
        }
    })
}

/// Rule 2: below 16 container levels the remaining window tiles go into the deepest container.
fn capped(tile: Tile) -> Tile {
    match tile {
        Tile::Container(mut container) => {
            cap_depth(&mut container, 1);
            Tile::Container(container)
        }
        window @ Tile::Window { .. } => window,
    }
}

fn cap_depth(container: &mut Container, level: usize) {
    if level < MAX_LAYOUT_DEPTH {
        for child in &mut container.children {
            if let Tile::Container(inner) = child {
                cap_depth(inner, level + 1);
            }
        }
        return;
    }
    let mut windows = Vec::new();
    let mut active = None;
    for (index, child) in std::mem::take(&mut container.children)
        .into_iter()
        .enumerate()
    {
        if container.active == Some(index) {
            active = Some(windows.len());
        }
        push_windows(child, &mut windows);
    }
    container.children = windows;
    container.active = active;
}

/// Every container has a child, so each one adds at least one window tile.
fn push_windows(tile: Tile, windows: &mut Vec<Tile>) {
    match tile {
        Tile::Container(container) => {
            for child in container.children {
                push_windows(child, windows);
            }
        }
        window @ Tile::Window { .. } => windows.push(window),
    }
}

/// `[x, y, w, h]` of `rect` in basis points of `area`, each clamped to 0..10000. A node
/// without a rect gets zeros.
fn basis_rect(rect: Option<SwayRect>, area: Rect) -> BasisRect {
    let Some(rect) = rect else {
        return [0; 4];
    };
    [
        basis_points(i128::from(rect.x) - i128::from(area.x), area.w),
        basis_points(i128::from(rect.y) - i128::from(area.y), area.h),
        basis_points(i128::from(rect.width), area.w),
        basis_points(i128::from(rect.height), area.h),
    ]
}

/// `length / extent` in basis points, rounded half up. `extent` is never 0.
fn basis_points(length: i128, extent: u64) -> u16 {
    let extent = i128::from(extent);
    let full = i128::from(MAX_BASIS_POINTS);
    let length = length.clamp(0, extent);
    let points = (2 * length * full + extent) / (2 * extent);
    u16::try_from(points).map_or(MAX_BASIS_POINTS, |points| points.min(MAX_BASIS_POINTS))
}

#[cfg(test)]
mod tests;
