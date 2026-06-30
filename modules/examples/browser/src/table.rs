//! Automatic table layout. The walker ([`crate::html`]) parses a `<table>` into
//! the [`Grid`] model here — cells carrying their parsed content blocks plus
//! `colspan`/`rowspan` and a measured min/max content width. This module then runs
//! the standard CSS *automatic table layout* (satisfy column minimums, then grow
//! toward maximums proportionally to the slack) and emits the grid as nested
//! engine containers: the table is a Column of row Containers; each cell is a
//! percentage-width box so the same column lines up across every row. 1px grid
//! lines come for free from the row/table background bleeding through 1px gaps
//! between cells; headers get a bold, centred, lightly tinted treatment.

use interstice_ui::{FontStyle, TextWrap};

use crate::css;
use crate::html::Block;
use crate::tables::{TABLE_BORDER, TABLE_CELL_BG, TABLE_HEADER_BG};

/// Horizontal padding inside a cell (left + right), added to measured widths and
/// applied as the cell box padding so they agree.
pub const CELL_PAD_X: f32 = 12.0;
/// Per-side cell padding `(top, right, bottom, left)`.
const CELL_PAD: (f32, f32, f32, f32) = (4.0, 6.0, 4.0, 6.0);
/// Nominal width the auto algorithm distributes over. Only the *proportions*
/// matter — the table is emitted with percentage columns that fill whatever width
/// the engine gives it — so this just sets the min→max growth balance.
const NOMINAL_AVAIL: f32 = 880.0;

/// A table cell with its parsed content and span/measurement metadata.
pub struct Cell {
    pub blocks: Vec<Block>,
    pub colspan: usize,
    pub rowspan: usize,
    pub header: bool,
    /// Min (longest unbreakable word) and max (no-wrap) content width, padding
    /// already folded in.
    pub min_w: f32,
    pub max_w: f32,
}

pub struct GridRow {
    pub cells: Vec<Cell>,
}

pub struct Grid {
    pub rows: Vec<GridRow>,
}

/// Measure the min (narrowest, wrapping every word) and max (widest, no wrap)
/// content width of a run of blocks — the cell's intrinsic widths.
pub fn measure_blocks(blocks: &[Block]) -> (f32, f32) {
    let mut min = 0.0f32;
    let mut max = 0.0f32;
    for b in blocks {
        let (bmin, bmax) = measure_block(b);
        min = min.max(bmin);
        max = max.max(bmax);
    }
    (min, max)
}

fn measure_block(b: &Block) -> (f32, f32) {
    match b {
        Block::Text {
            text,
            size,
            bold,
            italic,
            ..
        } => {
            let style = FontStyle {
                bold: *bold,
                italic: *italic,
            };
            let min = interstice_ui::min_text_width(text, *size, &TextWrap::Words, style);
            let max = interstice_ui::text_width(text, *size, style);
            (min, max)
        }
        Block::Container {
            children, padding, ..
        } => {
            let (cmin, cmax) = measure_blocks(children);
            let pad = padding.1 + padding.3;
            (cmin + pad, cmax + pad)
        }
        // Images contribute a modest fixed width so they don't dominate a column.
        Block::Image { .. } => (40.0, 140.0),
        Block::FloatRow { float_box, flow, .. } => {
            let (a, b) = measure_block(float_box);
            let (c, d) = measure_blocks(flow);
            (a.max(c), b.max(d))
        }
        Block::Space { .. } => (0.0, 0.0),
    }
}

/// A placed cell: its starting column, how many columns it spans, and the cell.
struct Placed<'a> {
    col: usize,
    span: usize,
    cell: &'a Cell,
}

/// Place every cell into a column grid, honouring `colspan`/`rowspan` occupancy
/// from earlier rows. Returns the placements and the total column count.
fn place(grid: &Grid) -> (Vec<Placed<'_>>, usize) {
    let mut out = Vec::new();
    // Remaining rows each column is still occupied by an earlier cell's rowspan.
    let mut occupied: Vec<usize> = Vec::new();
    for row in &grid.rows {
        let mut col = 0usize;
        for cell in &row.cells {
            while col < occupied.len() && occupied[col] > 0 {
                col += 1;
            }
            let span = cell.colspan.max(1);
            if col + span > occupied.len() {
                occupied.resize(col + span, 0);
            }
            out.push(Placed { col, span, cell });
            let rs = cell.rowspan.max(1);
            if rs > 1 {
                for slot in occupied.iter_mut().take(col + span).skip(col) {
                    *slot = rs;
                }
            }
            col += span;
        }
        for o in occupied.iter_mut() {
            if *o > 0 {
                *o -= 1;
            }
        }
    }
    let ncols = out.iter().map(|p| p.col + p.span).max().unwrap_or(1).max(1);
    (out, ncols)
}

/// Run automatic table layout and return each column's width as a fraction of the
/// table width (summing to 1).
fn column_fractions(grid: &Grid) -> Vec<f32> {
    let (placements, ncols) = place(grid);
    let mut min = vec![0.0f32; ncols];
    let mut max = vec![0.0f32; ncols];

    // Single-column cells set the column mins/maxes directly.
    for p in &placements {
        if p.span == 1 {
            min[p.col] = min[p.col].max(p.cell.min_w);
            max[p.col] = max[p.col].max(p.cell.max_w);
        }
    }
    // Spanning cells widen their covered columns if they aren't already wide
    // enough, distributing the deficit evenly.
    for p in &placements {
        if p.span > 1 {
            let cols = p.col..p.col + p.span;
            let cur_min: f32 = min[cols.clone()].iter().sum();
            if p.cell.min_w > cur_min {
                let add = (p.cell.min_w - cur_min) / p.span as f32;
                for k in cols.clone() {
                    min[k] += add;
                }
            }
            let cur_max: f32 = max[cols.clone()].iter().sum();
            if p.cell.max_w > cur_max {
                let add = (p.cell.max_w - cur_max) / p.span as f32;
                for k in cols {
                    max[k] += add;
                }
            }
        }
    }

    let total_min: f32 = min.iter().sum();
    let total_max: f32 = max.iter().sum();
    let widths: Vec<f32> = if total_max <= NOMINAL_AVAIL || (total_max - total_min) < 0.01 {
        max
    } else if total_min >= NOMINAL_AVAIL {
        min
    } else {
        let t = (NOMINAL_AVAIL - total_min) / (total_max - total_min);
        min.iter()
            .zip(&max)
            .map(|(mn, mx)| mn + (mx - mn) * t)
            .collect()
    };

    let sum: f32 = widths.iter().sum();
    if sum <= 0.0 {
        return vec![1.0 / ncols as f32; ncols];
    }
    widths.iter().map(|w| w / sum).collect()
}

/// Build the engine block tree for `grid`.
pub fn build(grid: Grid) -> Block {
    let fracs = column_fractions(&grid);

    let mut occupied: Vec<usize> = Vec::new();
    let mut rows: Vec<Block> = Vec::new();
    for row in grid.rows {
        let mut col = 0usize;
        let mut slots: Vec<Block> = Vec::new();
        let mut cells = row.cells.into_iter();
        loop {
            // Fill columns held by a carried rowspan with spacer cells so the
            // real cells stay column-aligned.
            while col < occupied.len() && occupied[col] > 0 {
                slots.push(cell_box(Vec::new(), frac_of(&fracs, col, 1), false));
                col += 1;
            }
            let Some(cell) = cells.next() else { break };
            let span = cell.colspan.max(1);
            let rs = cell.rowspan.max(1);
            if col + span > occupied.len() {
                occupied.resize(col + span, 0);
            }
            let frac = frac_of(&fracs, col, span);
            slots.push(cell_box(cell.blocks, frac, cell.header));
            if rs > 1 {
                for slot in occupied.iter_mut().take(col + span).skip(col) {
                    *slot = rs;
                }
            }
            col += span;
        }
        for o in occupied.iter_mut() {
            if *o > 0 {
                *o -= 1;
            }
        }
        rows.push(row_box(slots));
    }

    table_box(rows)
}

/// Sum the fractions of the `span` columns starting at `col`.
fn frac_of(fracs: &[f32], col: usize, span: usize) -> f32 {
    let end = (col + span).min(fracs.len());
    fracs.get(col..end).map(|s| s.iter().sum()).unwrap_or(0.0)
}

fn cell_box(children: Vec<Block>, frac: f32, header: bool) -> Block {
    let bg = if header { TABLE_HEADER_BG } else { TABLE_CELL_BG };
    Block::Container {
        direction: css::FlexDirection::Column,
        justify: css::Justify::Start,
        align: css::Align::Stretch,
        gap: 0.0,
        margin: (0.0, 0.0, 0.0, 0.0),
        padding: CELL_PAD,
        background: Some(bg),
        children,
        float: css::Float::None,
        clears: false,
        position: css::Position::Static,
        inset: (None, None, None, None),
        width: css::WidthVal::Pct(frac),
    }
}

fn row_box(cells: Vec<Block>) -> Block {
    Block::Container {
        direction: css::FlexDirection::Row,
        justify: css::Justify::Start,
        align: css::Align::Stretch,
        gap: 1.0, // reveals the row background ⇒ vertical grid lines
        margin: (0.0, 0.0, 0.0, 0.0),
        padding: (0.0, 0.0, 0.0, 0.0),
        background: Some(TABLE_BORDER),
        children: cells,
        float: css::Float::None,
        clears: false,
        position: css::Position::Static,
        inset: (None, None, None, None),
        width: css::WidthVal::Auto,
    }
}

fn table_box(rows: Vec<Block>) -> Block {
    Block::Container {
        direction: css::FlexDirection::Column,
        justify: css::Justify::Start,
        align: css::Align::Stretch,
        gap: 1.0, // horizontal grid lines between rows
        margin: (8.0, 0.0, 8.0, 0.0),
        padding: (1.0, 1.0, 1.0, 1.0), // outer border
        background: Some(TABLE_BORDER),
        children: rows,
        float: css::Float::None,
        clears: true, // tables sit in normal flow, below any preceding float
        position: css::Position::Static,
        inset: (None, None, None, None),
        width: css::WidthVal::Auto,
    }
}
