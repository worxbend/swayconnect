//! The ASCII map of `swayconnect layout` (`docs/PROTOCOL.md` 17.4.3). Pure: nothing here
//! writes.
//!
//! The map shows what the screen shows: the shown child of a tabbed or stacked container, the
//! floating windows on top in their order, and only the fullscreen window when there is one.
//! The legend below it lists every window of the workspace with where it is.

use std::collections::HashMap;

use serde_json::{Value, json};
use swayconnect_protocol::{Container, Desktop, Layout, Tile, Window, Workspace};

use super::error::CliError;
use super::render;

/// Characters across the map. A terminal cell is about twice as tall as it is wide.
const MAP_COLUMNS: usize = 72;
const MIN_ROWS: usize = 4;
const MAX_ROWS: usize = 40;
const FULL: u32 = 10_000;
const WHOLE_WORKSPACE: [u16; 4] = [0, 0, 10_000, 10_000];

const WORKSPACE_EDGE: char = '.';
const CORNER: char = '+';
const HORIZONTAL: char = '-';
const VERTICAL: char = '|';

/// Where a window of the workspace is, as the legend says it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Place {
    Tiled,
    Floating,
    Fullscreen,
    BehindTab,
    BehindFullscreen,
    NotInLayout,
}

impl Place {
    const fn as_str(self) -> &'static str {
        match self {
            Place::Tiled => "tiled",
            Place::Floating => "floating",
            Place::Fullscreen => "fullscreen",
            Place::BehindTab => "hidden tab",
            Place::BehindFullscreen => "behind fullscreen",
            Place::NotInLayout => "not in layout",
        }
    }
}

/// The workspace with `number`, or with `id`. Exactly one of the two is `Some`.
pub fn find_workspace(
    desktop: &Desktop,
    number: Option<u32>,
    id: Option<u64>,
) -> Result<&Workspace, CliError> {
    let (wanted, matches): (String, Vec<&Workspace>) = match (number, id) {
        (Some(number), None) => (
            number.to_string(),
            desktop
                .workspaces
                .iter()
                .filter(|workspace| workspace.number == Some(number))
                .collect(),
        ),
        (None, Some(id)) => (
            format!("with id {id}"),
            desktop
                .workspaces
                .iter()
                .filter(|workspace| workspace.id == id)
                .collect(),
        ),
        (Some(_), Some(_)) | (None, None) => {
            return Err(CliError::Usage("layout takes a number or --id, not both"));
        }
    };
    // Ids are unique in a valid state, so only a number can match several.
    match (matches.as_slice(), number) {
        ([], _) => Err(CliError::NoWorkspace(wanted)),
        ([workspace], _) | ([workspace, ..], None) => Ok(workspace),
        (_several, Some(number)) => Err(CliError::SeveralWorkspaces(number)),
    }
}

/// `{"workspace_id", "layout"}` with the layout in its wire form.
pub fn layout_json(desktop: &Desktop, workspace_id: u64) -> Value {
    let encoded = desktop.encode();
    let layout = encoded["workspaces"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|workspace| workspace["id"] == workspace_id)
        .map_or(Value::Null, |workspace| workspace["layout"].clone());
    json!({ "workspace_id": workspace_id, "layout": layout })
}

/// The map, then the legend. `windows` holds every window of the state; the legend keeps
/// those of `workspace`.
pub fn layout_text(workspace: &Workspace, layout: &Layout, windows: &[Window]) -> String {
    let [width, height] = layout.size;
    let header = format!(
        "workspace {} (id {}), {width} x {height}",
        workspace.name, workspace.id
    );
    let mut canvas = Canvas::new(rows_for(layout.size));
    let mut places = HashMap::new();
    let app_of = |id: u64| {
        windows
            .iter()
            .find(|window| window.id == id)
            .and_then(|window| window.app_id.as_deref())
    };
    match layout.fullscreen {
        Some(fullscreen_id) => {
            mark_all(layout, Place::BehindFullscreen, &mut places);
            places.insert(fullscreen_id, Place::Fullscreen);
            let label = label(fullscreen_id, app_of(fullscreen_id));
            canvas.draw_box(WHOLE_WORKSPACE, &label, overwrite);
        }
        None => {
            let mut draw = |id: u64, r: [u16; 4], place: Place| {
                places.insert(id, place);
                // Tiles share borders; a floating window covers what is below it.
                let put = if place == Place::Floating {
                    overwrite
                } else {
                    join_borders
                };
                canvas.draw_box(r, &label(id, app_of(id)), put);
            };
            if let Some(tile) = &layout.tiled {
                visit_shown(tile, Place::Tiled, &mut draw);
            }
            for tile in &layout.floating {
                visit_shown(tile, Place::Floating, &mut draw);
            }
            mark_hidden_tabs(layout, &mut places);
        }
    }
    let rows = windows
        .iter()
        .filter(|window| window.workspace_id == workspace.id)
        .map(|window| legend_row(window, places.get(&window.id)))
        .collect();
    let legend = render::table(&["ID", "APP", "PLACE", "TITLE"], rows);
    format!("{header}\n{}\n{legend}", canvas.text())
}

fn legend_row(window: &Window, place: Option<&Place>) -> Vec<String> {
    let place = place.copied().unwrap_or(Place::NotInLayout);
    vec![
        window.id.to_string(),
        render::or_absent(window.app_id.as_ref()),
        place.as_str().to_owned(),
        render::or_absent(window.title.as_ref()),
    ]
}

/// Rows that keep the workspace's aspect ratio, in cells about twice as tall as wide.
fn rows_for([width, height]: [u64; 2]) -> usize {
    let columns = MAP_COLUMNS as u64;
    let rows = if width == 0 {
        MIN_ROWS as u64
    } else {
        columns.saturating_mul(height) / width.saturating_mul(2)
    };
    usize::try_from(rows).map_or(MAX_ROWS, |rows| rows.clamp(MIN_ROWS, MAX_ROWS))
}

/// Calls `draw` for each window tile the screen shows, in drawing order.
fn visit_shown(tile: &Tile, place: Place, draw: &mut impl FnMut(u64, [u16; 4], Place)) {
    match tile {
        Tile::Window { id, r } => draw(*id, *r, place),
        Tile::Container(container) => {
            for child in shown_children(container) {
                visit_shown(child, place, draw);
            }
        }
    }
}

/// Every child of a split container; the shown child of a tabbed or stacked one.
fn shown_children(container: &Container) -> &[Tile] {
    if container.kind.shows_one_child() {
        let shown = container.active.unwrap_or(0);
        container.children.get(shown..=shown).unwrap_or_default()
    } else {
        &container.children
    }
}

/// Marks each window of the layout that `visit_shown` did not reach.
fn mark_hidden_tabs(layout: &Layout, places: &mut HashMap<u64, Place>) {
    let mut hidden = HashMap::new();
    mark_all(layout, Place::BehindTab, &mut hidden);
    for (id, place) in hidden {
        places.entry(id).or_insert(place);
    }
}

fn mark_all(layout: &Layout, place: Place, places: &mut HashMap<u64, Place>) {
    let mut stack: Vec<&Tile> = layout.tiled.iter().chain(&layout.floating).collect();
    while let Some(tile) = stack.pop() {
        match tile {
            Tile::Window { id, .. } => {
                places.insert(*id, place);
            }
            Tile::Container(container) => stack.extend(&container.children),
        }
    }
}

/// `<id> <app_id>`, with every character that is not printable ASCII as `?`, so that the
/// map's columns stay aligned. The legend has the full text.
fn label(id: u64, app_id: Option<&str>) -> String {
    let app: String = app_id
        .unwrap_or_default()
        .chars()
        .map(|character| {
            if character == ' ' || character.is_ascii_graphic() {
                character
            } else {
                '?'
            }
        })
        .collect();
    format!("{id} {app}").trim_end().to_owned()
}

/// A grid of `rows + 1` lines of `MAP_COLUMNS + 1` cells: tile edges fall on cell lines, so
/// two neighbours share one border.
struct Canvas {
    cells: Vec<Vec<char>>,
    /// For each cell inside a box, the index into `labels` of the top box there.
    owners: Vec<Vec<Option<usize>>>,
    labels: Vec<(Span, Span, String)>,
}

impl Canvas {
    fn new(rows: usize) -> Canvas {
        let mut canvas = Canvas {
            cells: vec![vec![' '; MAP_COLUMNS + 1]; rows + 1],
            owners: vec![vec![None; MAP_COLUMNS + 1]; rows + 1],
            labels: Vec::new(),
        };
        canvas.draw_frame(Span::of(0, FULL, MAP_COLUMNS), Span::of(0, FULL, rows));
        canvas
    }

    fn rows(&self) -> usize {
        self.cells.len() - 1
    }

    fn draw_frame(&mut self, columns: Span, rows: Span) {
        for column in columns.start..=columns.end {
            self.cells[rows.start][column] = WORKSPACE_EDGE;
            self.cells[rows.end][column] = WORKSPACE_EDGE;
        }
        for row in rows.start..=rows.end {
            self.cells[row][columns.start] = WORKSPACE_EDGE;
            self.cells[row][columns.end] = WORKSPACE_EDGE;
        }
    }

    /// A box for `r` in basis points, its inside cleared. `put` gives a cell from the one
    /// there and the box's own. The label is written by [`Canvas::text`], where the box
    /// still shows.
    fn draw_box(&mut self, [x, y, w, h]: [u16; 4], label: &str, put: fn(char, char) -> char) {
        let columns = Span::of(u32::from(x), u32::from(x) + u32::from(w), MAP_COLUMNS);
        let rows = Span::of(u32::from(y), u32::from(y) + u32::from(h), self.rows());
        let index = self.labels.len();
        for row in rows.start..=rows.end {
            for column in columns.start..=columns.end {
                let on_row_edge = row == rows.start || row == rows.end;
                let on_column_edge = column == columns.start || column == columns.end;
                let cell = match (on_row_edge, on_column_edge) {
                    (true, true) => CORNER,
                    (true, false) => HORIZONTAL,
                    (false, true) => VERTICAL,
                    (false, false) => ' ',
                };
                let current = &mut self.cells[row][column];
                *current = put(*current, cell);
                let inside = !on_row_edge && !on_column_edge;
                self.owners[row][column] = inside.then_some(index);
            }
        }
        self.labels.push((columns, rows, label.to_owned()));
    }

    /// Each label goes on the first line where the left end of its box's inside is not
    /// covered by a later box, and stops where a later box begins.
    fn text(mut self) -> String {
        for (index, (columns, rows, label)) in self.labels.iter().enumerate() {
            let first_column = columns.start + 1;
            let shows = |row: usize, column: usize| self.owners[row][column] == Some(index);
            let Some(row) = (rows.start + 1..rows.end).find(|&row| shows(row, first_column)) else {
                continue;
            };
            let free_cells = (first_column..columns.end).take_while(|&column| shows(row, column));
            for (column, character) in free_cells.zip(label.chars()) {
                self.cells[row][column] = character;
            }
        }
        let lines: Vec<String> = self
            .cells
            .iter()
            .map(|row| row.iter().collect::<String>().trim_end().to_owned())
            .collect();
        lines.join("\n")
    }
}

/// Where two different border lines meet, the cell becomes a corner.
fn join_borders(current: char, cell: char) -> char {
    let is_border = |character: char| [CORNER, HORIZONTAL, VERTICAL].contains(&character);
    if is_border(current) && is_border(cell) && current != cell {
        CORNER
    } else {
        cell
    }
}

fn overwrite(_current: char, cell: char) -> char {
    cell
}

/// First and last cell line of a tile edge pair, at least one cell apart.
#[derive(Clone, Copy)]
struct Span {
    start: usize,
    end: usize,
}

impl Span {
    /// `from` and `to` in basis points; values above 10000 count as 10000.
    fn of(from: u32, to: u32, cells: usize) -> Span {
        let cell = |basis_points: u32| basis_points.min(FULL) as usize * cells / FULL as usize;
        let end = cell(to).max(cell(from) + 1).min(cells);
        Span {
            start: cell(from).min(end - 1),
            end,
        }
    }
}

#[cfg(test)]
#[path = "layout_map_tests.rs"]
mod tests;
