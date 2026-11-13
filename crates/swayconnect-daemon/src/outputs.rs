//! Pure functions: the output nodes of `GET_TREE` to `Output`s, and the windows hidden in the
//! scratchpad to `ScratchpadWindow`s (`docs/PROTOCOL.md` 17.4.2). `GET_OUTPUTS` is never sent
//! (`docs/DESIGN.md` D22).

use swayconnect_protocol::limits::{MAX_OUTPUTS, MAX_WINDOWS};
use swayconnect_protocol::{Output, ScratchpadWindow, Transform};
use swayconnect_sway::Node;

use crate::layout::protocol_rect;
use crate::model::{
    INTERNAL_OUTPUT, IconLookup, SCRATCHPAD_WORKSPACE, WORKSPACE_TYPE, app_id_of, is_window, label,
    shown_outputs, title,
};

const DEFAULT_SCALE_MILLI: u64 = 1000;
/// Far above any real scale; keeps a hostile float from becoming a huge integer.
const MAX_SCALE_MILLI: f64 = 1_000_000.0;

/// The listed outputs in tree order, at most 16, and whether more were cut.
pub fn outputs(tree: &Node) -> (Vec<Output>, bool) {
    let mut outputs: Vec<Output> = shown_outputs(tree)
        .filter_map(output)
        .take(MAX_OUTPUTS + 1)
        .collect();
    let cut = outputs.len() > MAX_OUTPUTS;
    outputs.truncate(MAX_OUTPUTS);
    (outputs, cut)
}

/// `None` for an output node without a name or a usable rect: it cannot be shown.
fn output(node: &Node) -> Option<Output> {
    let current_workspace_id = node.current_workspace.as_deref().and_then(|name| {
        node.nodes
            .iter()
            .find(|workspace| {
                workspace.node_type == WORKSPACE_TYPE && workspace.name.as_deref() == Some(name)
            })
            .map(|workspace| workspace.id)
    });
    Some(Output {
        name: label(node.name.as_deref()?),
        model: model_text(node),
        rect: protocol_rect(node)?,
        scale_milli: scale_milli(node.scale),
        transform: node
            .transform
            .as_deref()
            .and_then(Transform::from_wire)
            .unwrap_or(Transform::Normal),
        // `GET_TREE` holds enabled outputs only; sway 1.12 always sends both flags.
        active: node.active.unwrap_or(true),
        power: node.power.unwrap_or(true),
        current_workspace_id,
    })
}

/// Make and model joined by a space, for display only.
fn model_text(node: &Node) -> Option<String> {
    let parts: Vec<&str> = [node.make.as_deref(), node.model.as_deref()]
        .into_iter()
        .flatten()
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect();
    (!parts.is_empty()).then(|| label(&parts.join(" ")))
}

fn scale_milli(scale: Option<f64>) -> u64 {
    scale
        .filter(|scale| scale.is_finite() && *scale > 0.0)
        // The value is finite, positive and capped, so the cast cannot wrap.
        .map_or(DEFAULT_SCALE_MILLI, |scale| {
            (scale * 1000.0).round().min(MAX_SCALE_MILLI) as u64
        })
}

/// The windows hidden in the scratchpad, at most 512, and whether more were cut. A shown
/// scratchpad window sits on a normal workspace and is not listed here.
pub fn scratchpad(tree: &Node, icon_of: IconLookup<'_>) -> (Vec<ScratchpadWindow>, bool) {
    let mut hidden = Vec::new();
    let scratch_workspaces = tree
        .nodes
        .iter()
        .filter(|output| output.name.as_deref() == Some(INTERNAL_OUTPUT))
        .flat_map(|output| &output.nodes)
        .filter(|workspace| workspace.name.as_deref() == Some(SCRATCHPAD_WORKSPACE));
    for workspace in scratch_workspaces {
        push_windows(workspace, &mut hidden);
    }
    let cut = hidden.len() > MAX_WINDOWS;
    hidden.truncate(MAX_WINDOWS);
    let windows = hidden
        .into_iter()
        .map(|node| ScratchpadWindow {
            id: node.id,
            app_id: app_id_of(node).map(label),
            title: node.name.as_deref().map(title),
            icon: icon_of(node),
        })
        .collect();
    (windows, cut)
}

/// Tiled first, then floating, as `model` orders `windows`. Depth is bounded by the JSON parser.
fn push_windows<'tree>(container: &'tree Node, found: &mut Vec<&'tree Node>) {
    for child in container.nodes.iter().chain(&container.floating_nodes) {
        if is_window(child) {
            found.push(child);
        } else {
            push_windows(child, found);
        }
    }
}

#[cfg(test)]
mod tests;
