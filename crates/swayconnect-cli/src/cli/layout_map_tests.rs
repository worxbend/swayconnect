use swayconnect_protocol::{ContainerKind, SwayStatus};

use super::*;

const WORKSPACE_ID: u64 = 4;

fn workspace(id: u64, number: Option<u32>) -> Workspace {
    Workspace {
        id,
        number,
        name: number.map_or_else(|| "mail".to_owned(), |number| number.to_string()),
        output: Some("TEST-1".to_owned()),
        focused: id == WORKSPACE_ID,
        visible: id == WORKSPACE_ID,
        urgent: false,
        rect: None,
        layout: None,
    }
}

fn window(id: u64, app_id: &str) -> Window {
    Window {
        id,
        workspace_id: WORKSPACE_ID,
        app_id: Some(app_id.to_owned()),
        title: Some(format!("title {id}")),
        focused: false,
        urgent: false,
        floating: id == 13,
        fullscreen: false,
        icon: None,
    }
}

fn leaf(id: u64, r: [u16; 4]) -> Tile {
    Tile::Window { id, r }
}

fn container(kind: ContainerKind, r: [u16; 4], children: Vec<Tile>, active: usize) -> Tile {
    Tile::Container(Container {
        kind,
        r,
        children,
        active: kind.shows_one_child().then_some(active),
    })
}

/// Shaped like workspace 4 of the testkit's layout fixture: 6 on the left; 8 above the tabs 9
/// and 12 on the right, 12 shown; 13 floating across the middle border, above the tabs.
fn layout() -> Layout {
    let tabs = container(
        ContainerKind::Tabbed,
        [5000, 5000, 5000, 5000],
        vec![
            leaf(9, [5000, 5000, 5000, 5000]),
            leaf(12, [5000, 5000, 5000, 5000]),
        ],
        1,
    );
    let right = container(
        ContainerKind::SplitV,
        [5000, 0, 5000, 10000],
        vec![leaf(8, [5000, 0, 5000, 5000]), tabs],
        0,
    );
    Layout {
        size: [2560, 1410],
        fullscreen: None,
        tiled: Some(container(
            ContainerKind::SplitH,
            [0, 0, 10000, 10000],
            vec![leaf(6, [0, 0, 5000, 10000]), right],
            0,
        )),
        floating: vec![leaf(13, [3750, 1500, 2500, 2500])],
    }
}

fn windows() -> Vec<Window> {
    let mut windows: Vec<Window> = [
        (6, "notes"),
        (8, "terminal"),
        (9, "docs"),
        (12, "mail\u{e9}"),
        (13, "calc"),
        (21, "lost"),
    ]
    .into_iter()
    .map(|(id, app_id)| window(id, app_id))
    .collect();
    let mut elsewhere = window(7, "Browser");
    elsewhere.workspace_id = 5;
    windows.push(elsewhere);
    windows
}

fn map_and_legend(text: &str) -> (Vec<&str>, Vec<Vec<&str>>) {
    let lines: Vec<&str> = text.lines().collect();
    let legend_start = lines
        .iter()
        .position(|line| line.starts_with("ID"))
        .expect("legend header");
    let legend = lines[legend_start..]
        .iter()
        .map(|line| line.split_whitespace().collect())
        .collect();
    (lines[1..legend_start].to_vec(), legend)
}

#[test]
fn the_map_draws_what_the_screen_shows_and_the_legend_says_where_each_window_is() {
    let text = layout_text(&workspace(WORKSPACE_ID, Some(1)), &layout(), &windows());
    let (map, legend) = map_and_legend(&text);

    assert!(
        text.starts_with("workspace 1 (id 4), 2560 x 1410\n"),
        "{text}"
    );
    // 72 columns at 2560 x 1410: 72 * 1410 / 5120 = 19 rows, 20 cell lines.
    assert_eq!(map.len(), 20, "{text}");
    assert_eq!(
        map[0],
        format!("+{}+{}+", "-".repeat(35), "-".repeat(35)),
        "{text}"
    );
    assert!(map[1].starts_with("|6 notes "), "{text}");
    assert!(map[1].contains("|8 terminal"), "{text}");
    assert!(
        map[10].contains("|12 mail?"),
        "a non-ASCII app id keeps the columns: {text}"
    );
    assert!(map.iter().any(|line| line.contains("|13 calc")), "{text}");
    assert!(
        !text.contains("9 docs"),
        "a hidden tab is not drawn: {text}"
    );
    assert!(map.iter().all(|line| line.is_ascii()), "{text}");

    let expected = [
        vec!["ID", "APP", "PLACE", "TITLE"],
        vec!["6", "notes", "tiled", "title", "6"],
        vec!["8", "terminal", "tiled", "title", "8"],
        vec!["9", "docs", "hidden", "tab", "title", "9"],
        vec!["12", "mail\u{e9}", "tiled", "title", "12"],
        vec!["13", "calc", "floating", "title", "13"],
        vec!["21", "lost", "not", "in", "layout", "title", "21"],
    ];
    assert_eq!(legend, expected, "{text}");
}

#[test]
fn a_floating_window_lies_on_top_of_the_tiles() {
    let text = layout_text(&workspace(WORKSPACE_ID, Some(1)), &layout(), &windows());
    let (map, _) = map_and_legend(&text);

    // Cell lines 2 to 7 hold window 13 between columns 27 and 45; it hides the tile border
    // at column 36, and its own border is not joined to the tiles below.
    assert_eq!(&map[2][27..=45], format!("+{}+", "-".repeat(17)), "{text}");
    assert!(map[3][27..].starts_with("|13 calc  "), "{text}");
    for line in &map[3..7] {
        assert_eq!(line.chars().nth(36), Some(' '), "{text}");
        assert_eq!(line.chars().nth(45), Some('|'), "{text}");
    }
}

#[test]
fn a_fullscreen_window_is_the_only_one_drawn() {
    let mut layout = layout();
    layout.fullscreen = Some(8);

    let text = layout_text(&workspace(WORKSPACE_ID, Some(1)), &layout, &windows());
    let (map, legend) = map_and_legend(&text);

    assert_eq!(map[0], format!("+{}+", "-".repeat(71)), "{text}");
    assert!(map[1].starts_with("|8 terminal"), "{text}");
    assert!(!text.contains("6 notes"), "{text}");
    assert_eq!(legend[2][..3], ["8", "terminal", "fullscreen"]);
    assert_eq!(legend[1][..4], ["6", "notes", "behind", "fullscreen"]);
}

#[test]
fn an_empty_layout_draws_only_the_workspace_edge_and_tiny_tiles_keep_a_cell() {
    let mut layout = Layout {
        size: [0, 0],
        fullscreen: None,
        tiled: None,
        floating: Vec::new(),
    };
    let empty = layout_text(&workspace(WORKSPACE_ID, Some(1)), &layout, &[]);
    let (map, legend) = map_and_legend(&empty);
    assert_eq!(map.len(), MIN_ROWS + 1, "{empty}");
    assert_eq!(map[0], ".".repeat(73));
    assert_eq!(legend, [vec!["ID", "APP", "PLACE", "TITLE"]]);

    layout.floating = vec![leaf(13, [10000, 10000, 0, 0]), leaf(6, [0, 0, 1, 1])];
    let tiny = layout_text(&workspace(WORKSPACE_ID, Some(1)), &layout, &windows());
    let (map, _) = map_and_legend(&tiny);
    assert!(map[0].starts_with("++"), "{tiny}");
    assert!(map[MIN_ROWS - 1].ends_with("++"), "{tiny}");
}

fn desktop(workspaces: Vec<Workspace>) -> Desktop {
    Desktop {
        sway: SwayStatus {
            online: true,
            version: None,
        },
        outputs: None,
        workspaces,
        windows: Vec::new(),
        scratchpad: None,
        layouts_cut: None,
        truncated: false,
    }
}

#[test]
fn a_workspace_is_found_by_its_number_or_its_id() {
    let desktop = desktop(vec![
        workspace(4, Some(1)),
        workspace(5, Some(2)),
        workspace(6, Some(2)),
        workspace(16, None),
    ]);

    let found = |number, id| find_workspace(&desktop, number, id).map(|workspace| workspace.id);
    assert_eq!(found(Some(1), None).ok(), Some(4));
    assert_eq!(found(None, Some(16)).ok(), Some(16));
    assert!(matches!(
        found(Some(2), None),
        Err(CliError::SeveralWorkspaces(2))
    ));
    assert!(matches!(found(Some(9), None), Err(CliError::NoWorkspace(text)) if text == "9"));
    assert!(
        matches!(found(None, Some(9)), Err(CliError::NoWorkspace(text)) if text == "with id 9")
    );
    assert!(matches!(found(None, None), Err(CliError::Usage(_))));
}

#[test]
fn the_json_form_is_the_wire_layout_of_the_workspace() {
    let mut with_layout = workspace(4, Some(1));
    with_layout.layout = Some(Some(layout()));
    let desktop = desktop(vec![with_layout, workspace(5, Some(2))]);

    let value = layout_json(&desktop, 4);

    assert_eq!(value["workspace_id"], 4);
    assert_eq!(value["layout"]["size"], json!([2560, 1410]));
    assert_eq!(value["layout"]["floating"][0]["w"], 13);
    assert_eq!(value["layout"]["tiled"]["c"][1]["c"][1]["a"], 1);
}
