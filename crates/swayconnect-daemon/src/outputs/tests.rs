use swayconnect_protocol::{IconHash, Rect};
use swayconnect_testkit::fixtures;

use super::*;

fn fixture_tree() -> Node {
    serde_json::from_str(&fixtures::layout_tree()).expect("tree fixture")
}

fn output_node(id: u64, name: &str) -> Node {
    Node {
        id,
        node_type: "output".to_owned(),
        name: Some(name.to_owned()),
        rect: Some(swayconnect_sway::Rect {
            x: 0,
            y: 0,
            width: 10,
            height: 10,
        }),
        ..Node::default()
    }
}

fn root_of(outputs: Vec<Node>) -> Node {
    Node {
        node_type: "root".to_owned(),
        nodes: outputs,
        ..Node::default()
    }
}

#[test]
fn the_fixture_outputs_skip_the_internal_one() {
    let (outputs, cut) = outputs(&fixture_tree());

    assert!(!cut);
    assert_eq!(
        outputs,
        [
            Output {
                name: "TEST-1".to_owned(),
                model: Some("Example Displays Desk Panel 27".to_owned()),
                rect: Rect {
                    x: 0,
                    y: 0,
                    w: 2560,
                    h: 1440
                },
                scale_milli: 1000,
                transform: Transform::Normal,
                active: true,
                power: true,
                current_workspace_id: Some(4),
            },
            Output {
                name: "TEST-2".to_owned(),
                model: Some("Example Displays Side Panel 24".to_owned()),
                rect: Rect {
                    x: 2560,
                    y: 0,
                    w: 864,
                    h: 1536
                },
                scale_milli: 1250,
                transform: Transform::Rotate90,
                active: true,
                power: true,
                current_workspace_id: Some(14),
            },
        ]
    );
}

#[test]
fn outputs_are_cut_to_16() {
    let names: Vec<String> = (0..17).map(|index| format!("OUT-{index}")).collect();
    let tree = root_of(
        (0..17)
            .map(|index| output_node(index + 2, &names[index as usize]))
            .collect(),
    );

    let (outputs, cut) = outputs(&tree);

    assert!(cut);
    assert_eq!(outputs.len(), 16);
    assert_eq!(outputs[15].name, "OUT-15");
}

#[test]
fn odd_output_facts_fall_back_to_safe_values() {
    let mut odd = output_node(2, "OUT\u{1b}-1");
    odd.transform = Some("diagonal".to_owned());
    odd.scale = Some(f64::NAN);
    odd.make = Some("  ".to_owned());
    odd.model = Some("Panel\u{202e}".to_owned());
    odd.power = Some(false);
    odd.current_workspace = Some("missing".to_owned());
    let mut huge = output_node(3, "OUT-2");
    huge.scale = Some(1e300);
    let no_rect = Node {
        rect: None,
        ..output_node(4, "OUT-3")
    };

    let (outputs, _) = outputs(&root_of(vec![odd, huge, no_rect]));

    assert_eq!(outputs.len(), 2, "an output without a rect cannot be shown");
    let odd = &outputs[0];
    assert_eq!(odd.name, "OUT\u{FFFD}-1");
    assert_eq!(odd.transform, Transform::Normal);
    assert_eq!(odd.scale_milli, 1000);
    assert_eq!(odd.model.as_deref(), Some("Panel\u{FFFD}"));
    assert!(!odd.power);
    assert_eq!(odd.current_workspace_id, None);
    assert_eq!(outputs[1].scale_milli, 1_000_000);
    assert_eq!(outputs[1].model, None);
}

#[test]
fn the_scratchpad_lists_hidden_windows_with_their_icon() {
    let music = IconHash::of(b"music icon");
    let icon_of = |node: &Node| (node.app_id.as_deref() == Some("music")).then_some(music);

    let (hidden, cut) = scratchpad(&fixture_tree(), &icon_of);

    assert!(!cut);
    assert_eq!(
        hidden,
        [ScratchpadWindow {
            id: 16,
            app_id: Some("music".to_owned()),
            title: Some("Radio stream".to_owned()),
            icon: Some(music),
        }]
    );
}

#[test]
fn the_scratchpad_is_cut_to_512_windows() {
    let windows: Vec<Node> = (0..513)
        .map(|id| Node {
            id: 100 + id,
            node_type: "floating_con".to_owned(),
            app_id: Some("app".to_owned()),
            ..Node::default()
        })
        .collect();
    let scratch = Node {
        node_type: "workspace".to_owned(),
        name: Some(SCRATCHPAD_WORKSPACE.to_owned()),
        floating_nodes: windows,
        ..Node::default()
    };
    let internal = Node {
        nodes: vec![scratch],
        ..output_node(2, INTERNAL_OUTPUT)
    };

    let (hidden, cut) = scratchpad(&root_of(vec![internal]), &|_| None);

    assert!(cut);
    assert_eq!(hidden.len(), 512);
    assert_eq!(hidden[511].id, 611);
}
