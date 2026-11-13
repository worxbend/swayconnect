//! Pure function: sway tree, workspaces and version to `Desktop`. Written by slice S6b; the
//! v2 additions of `docs/PROTOCOL.md` 17.4 come from `layout` and `outputs`.
//!
//! Every string in the input is set by an application or by sway and is untrusted
//! (`docs/DESIGN.md` 4.5, `docs/PROTOCOL.md` 9.3).

use std::collections::{HashMap, HashSet};

use swayconnect_protocol::limits::{
    MAX_DESKTOP_BYTES, MAX_LABEL_SCALARS, MAX_TITLE_SCALARS, MAX_WINDOWS, MAX_WORKSPACE_NUMBER,
    MAX_WORKSPACES,
};
use swayconnect_protocol::text::replace_unsafe;
use swayconnect_protocol::{Desktop, IconHash, SwayStatus, Window, Workspace};
use swayconnect_sway::{Node, Version, WorkspaceInfo};

use crate::{layout, outputs};

pub(crate) const INTERNAL_OUTPUT: &str = "__i3";
pub(crate) const SCRATCHPAD_WORKSPACE: &str = "__i3_scratch";
pub(crate) const WORKSPACE_TYPE: &str = "workspace";
const FLOATING_TYPE: &str = "floating_con";
const TILED_TYPE: &str = "con";

/// The icon hash of a window node, when one was produced. The lookup reads every hint of
/// the node it needs (`app_id`, Xwayland class and instance, sandbox app id, pid).
pub type IconLookup<'a> = &'a dyn Fn(&Node) -> Option<IconHash>;

/// The state while sway is away.
pub fn offline_desktop() -> Desktop {
    Desktop {
        sway: SwayStatus {
            online: false,
            version: None,
        },
        workspaces: Vec::new(),
        windows: Vec::new(),
        truncated: false,
        outputs: Some(Vec::new()),
        scratchpad: Some(Vec::new()),
        layouts_cut: Some(false),
    }
}

/// [`online_desktop_with_icons`] for a server that has no icons: every `icon` is `null`.
pub fn online_desktop(tree: &Node, listed: &[WorkspaceInfo], version: &Version) -> Desktop {
    online_desktop_with_icons(tree, listed, version, &|_| None)
}

/// The state of a running sway, cut to the limits of `docs/PROTOCOL.md` section 14 and 17.4.5.
pub fn online_desktop_with_icons(
    tree: &Node,
    listed: &[WorkspaceInfo],
    version: &Version,
    icon_of: IconLookup<'_>,
) -> Desktop {
    let workspace_nodes = workspace_nodes(tree);
    let mut workspaces: Vec<Workspace> = listed
        .iter()
        .filter(|info| info.name.as_deref() != Some(SCRATCHPAD_WORKSPACE))
        .take(MAX_WORKSPACES + 1)
        .map(|info| workspace(info, workspace_nodes.get(&info.id).copied()))
        .collect();
    let mut windows = windows_of_listed_workspaces(tree, &workspaces, icon_of);
    let (outputs, outputs_cut) = outputs::outputs(tree);
    let (scratchpad, scratchpad_cut) = outputs::scratchpad(tree, icon_of);

    let truncated = workspaces.len() > MAX_WORKSPACES
        || windows.len() > MAX_WINDOWS
        || outputs_cut
        || scratchpad_cut;
    workspaces.truncate(MAX_WORKSPACES);
    windows.truncate(MAX_WINDOWS);

    let mut desktop = Desktop {
        sway: SwayStatus {
            online: true,
            version: Some(label(&version.human_readable)),
        },
        workspaces,
        windows,
        truncated,
        outputs: Some(outputs),
        scratchpad: Some(scratchpad),
        layouts_cut: Some(false),
    };
    keep_layout_windows_listed(&mut desktop);
    cut_to_size(&mut desktop);
    desktop
}

fn workspace(info: &WorkspaceInfo, node: Option<&Node>) -> Workspace {
    Workspace {
        id: info.id,
        number: info.num.and_then(workspace_number),
        name: label(info.name.as_deref().unwrap_or_default()),
        output: info.output.as_deref().map(label),
        focused: info.focused,
        visible: info.visible,
        urgent: info.urgent,
        rect: Some(node.and_then(layout::protocol_rect)),
        layout: Some(node.and_then(layout::workspace_layout)),
    }
}

/// Sway reports -1 for a workspace with no number (`man 7 sway-ipc`).
fn workspace_number(num: i64) -> Option<u32> {
    u64::try_from(num)
        .ok()
        .filter(|number| *number <= MAX_WORKSPACE_NUMBER)
        .and_then(|number| u32::try_from(number).ok())
}

/// The outputs a client sees: every one but sway's internal `__i3`.
pub(crate) fn shown_outputs(tree: &Node) -> impl Iterator<Item = &Node> {
    tree.nodes
        .iter()
        .filter(|output| output.name.as_deref() != Some(INTERNAL_OUTPUT))
}

/// Workspace nodes of the shown outputs by id, in tree order of the outputs.
fn workspace_nodes(tree: &Node) -> HashMap<u64, &Node> {
    shown_outputs(tree)
        .flat_map(|output| &output.nodes)
        .filter(|node| node.node_type == WORKSPACE_TYPE)
        .map(|node| (node.id, node))
        .collect()
}

/// Windows in layout tree order. The tree and the workspace list come from two requests;
/// a window of a workspace the list lacks is left out, so every `workspace_id` has its entry.
fn windows_of_listed_workspaces(
    tree: &Node,
    workspaces: &[Workspace],
    icon_of: IconLookup<'_>,
) -> Vec<Window> {
    let listed_ids: HashSet<u64> = workspaces.iter().map(|workspace| workspace.id).collect();
    let mut windows = Vec::new();
    for workspace_node in shown_outputs(tree).flat_map(|output| &output.nodes) {
        if workspace_node.node_type == WORKSPACE_TYPE && listed_ids.contains(&workspace_node.id) {
            let on_workspace = Placement {
                workspace_id: workspace_node.id,
                is_floating: false,
            };
            collect_windows(workspace_node, on_workspace, icon_of, &mut windows);
        }
    }
    windows
}

/// Where a container sits. Its descendants inherit it.
#[derive(Clone, Copy)]
struct Placement {
    workspace_id: u64,
    /// Sway gives the type `floating_con` only to the top container of a floating group; the
    /// windows of a floating split have the type `con` and float with it.
    is_floating: bool,
}

/// Tiled children first, then floating ones. The depth is bounded by the JSON parser.
fn collect_windows(
    container: &Node,
    placement: Placement,
    icon_of: IconLookup<'_>,
    windows: &mut Vec<Window>,
) {
    let floating = Placement {
        is_floating: true,
        ..placement
    };
    let tiled_children = container.nodes.iter().map(|child| (child, placement));
    let floating_children = container
        .floating_nodes
        .iter()
        .map(|child| (child, floating));
    for (child, placement) in tiled_children.chain(floating_children) {
        if is_window(child) {
            windows.push(window(child, placement, icon_of));
        } else {
            collect_windows(child, placement, icon_of, windows);
        }
    }
}

pub(crate) fn is_window(node: &Node) -> bool {
    let is_container_type = node.node_type == TILED_TYPE || node.node_type == FLOATING_TYPE;
    let has_client = node.pid.is_some() || node.app_id.is_some() || node.window.is_some();
    let is_leaf = node.nodes.is_empty() && node.floating_nodes.is_empty();
    is_container_type && has_client && is_leaf
}

/// The sway `app_id`, or the class of an Xwayland window. Raw: untrusted text.
pub(crate) fn app_id_of(node: &Node) -> Option<&str> {
    let xwayland_class = || node.window_properties.as_ref()?.class.as_deref();
    node.app_id.as_deref().or_else(xwayland_class)
}

fn window(node: &Node, placement: Placement, icon_of: IconLookup<'_>) -> Window {
    Window {
        id: node.id,
        workspace_id: placement.workspace_id,
        app_id: app_id_of(node).map(label),
        title: node.name.as_deref().map(title),
        focused: node.focused,
        urgent: node.urgent,
        floating: placement.is_floating || node.node_type == FLOATING_TYPE,
        fullscreen: node.fullscreen_mode.is_some_and(|mode| mode != 0),
        icon: Some(icon_of(node)),
    }
}

pub(crate) fn title(text: &str) -> String {
    safe_prefix(text, MAX_TITLE_SCALARS)
}

pub(crate) fn label(text: &str) -> String {
    safe_prefix(text, MAX_LABEL_SCALARS)
}

/// Replaces first, then cuts (`docs/PROTOCOL.md` 9.3 rule 3).
fn safe_prefix(text: &str, max_scalars: usize) -> String {
    replace_unsafe(text).chars().take(max_scalars).collect()
}

/// A layout tile names only a window of `windows` (17.4.3); the count cut can drop some.
fn keep_layout_windows_listed(desktop: &mut Desktop) {
    let listed: HashSet<u64> = desktop.windows.iter().map(|window| window.id).collect();
    let is_listed = |id: u64| listed.contains(&id);
    for workspace in &mut desktop.workspaces {
        if let Some(Some(layout)) = &mut workspace.layout {
            layout.retain_windows(&is_listed);
        }
    }
}

/// 17.4.5: drops every layout, then scratchpad entries from the end, then windows from the
/// end, then workspaces from the end, until the encoded state fits.
fn cut_to_size(desktop: &mut Desktop) {
    if fits(desktop) {
        return;
    }
    for workspace in &mut desktop.workspaces {
        workspace.layout = Some(None);
    }
    desktop.layouts_cut = Some(true);
    if fits(desktop) {
        return;
    }
    desktop.truncated = true;
    keep_fitting_prefix(desktop, |desktop| {
        desktop.scratchpad.get_or_insert_with(Vec::new)
    });
    if !fits(desktop) {
        keep_fitting_prefix(desktop, |desktop| &mut desktop.windows);
    }
    if !fits(desktop) {
        keep_fitting_prefix(desktop, |desktop| &mut desktop.workspaces);
    }
}

fn fits(desktop: &Desktop) -> bool {
    // A state that cannot be encoded cannot be sent either, so it does not fit.
    serde_json::to_vec(&desktop.encode()).is_ok_and(|encoded| encoded.len() <= MAX_DESKTOP_BYTES)
}

/// Cuts one list to its longest prefix with which `desktop` fits; to nothing if none does.
fn keep_fitting_prefix<T: Clone>(desktop: &mut Desktop, list: fn(&mut Desktop) -> &mut Vec<T>) {
    let mut entries = std::mem::take(list(desktop));
    // Invariant: a prefix of `fitting` entries fits or is empty; no prefix over `limit` fits.
    let (mut fitting, mut limit) = (0, entries.len());
    while fitting < limit {
        let candidate = fitting + (limit - fitting).div_ceil(2);
        *list(desktop) = entries[..candidate].to_vec();
        if fits(desktop) {
            fitting = candidate;
        } else {
            limit = candidate - 1;
        }
    }
    entries.truncate(fitting);
    *list(desktop) = entries;
}

#[cfg(test)]
mod tests;
