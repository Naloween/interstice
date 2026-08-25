//! Minimal CSS grid layout. The walker ([`crate::html`]) collects a
//! `display:grid` element's children as a block list and hands them here with the
//! column weights parsed from `grid-template-columns`. We don't implement true grid
//! placement (no line-based placement, spanning, named areas, or auto rows): children
//! flow **row-major** into `weights.len()` columns, each column taking its weight's
//! share of the container width. That covers the common case — sidebars and card
//! grids — well enough to stop such layouts collapsing into a single column, which is
//! what `display:grid` falling back to `block` used to do.
//!
//! The output mirrors [`crate::table`]: a Column of Row containers whose cells are
//! percentage-width boxes, so columns line up across rows and the whole grid fills
//! whatever width the engine gives it.

use crate::css;
use crate::html::Block;

/// Build the engine block tree for a grid with the given collected `children`,
/// per-column `weights` (summing to 1), and `gap`. The remaining fields come from
/// the grid element's own computed style so it keeps its margin/padding/background
/// and float/position context, exactly like the flex path.
#[allow(clippy::too_many_arguments)]
pub fn build(
    children: Vec<Block>,
    weights: &[f32],
    gap: f32,
    margin: (f32, f32, f32, f32),
    padding: (f32, f32, f32, f32),
    background: Option<css::Rgba>,
    float: css::Float,
    clears: bool,
    position: css::Position,
    inset: (Option<f32>, Option<f32>, Option<f32>, Option<f32>),
    inset_pct: (Option<f32>, Option<f32>, Option<f32>, Option<f32>),
    node: Option<crate::dom::NodeId>,
) -> Block {
    let ncols = weights.len().max(1);
    let mut rows: Vec<Block> = Vec::new();
    let mut cells: Vec<Block> = Vec::new();
    let mut col = 0usize;

    for child in children {
        cells.push(cell_box(child, weights.get(col).copied().unwrap_or(1.0 / ncols as f32)));
        col += 1;
        if col == ncols {
            rows.push(row_box(std::mem::take(&mut cells), gap));
            col = 0;
        }
    }
    // A short final row (fewer children than columns) still gets emitted.
    if !cells.is_empty() {
        rows.push(row_box(cells, gap));
    }

    Block::Container {
        direction: css::FlexDirection::Column,
        justify: css::Justify::Start,
        align: css::Align::Stretch,
        gap,
        margin,
        padding,
        background,
        children: rows,
        float,
        clears,
        position,
        inset,
        inset_pct,
        width: css::WidthVal::Auto,
        height: css::WidthVal::Auto,
        node,
        center: false,
    }
}

/// One grid cell: a percentage-width column box holding its child block. Column
/// content stacks vertically (matching a grid track that contains block content).
fn cell_box(child: Block, weight: f32) -> Block {
    Block::Container {
        direction: css::FlexDirection::Column,
        justify: css::Justify::Start,
        align: css::Align::Stretch,
        gap: 0.0,
        margin: (0.0, 0.0, 0.0, 0.0),
        padding: (0.0, 0.0, 0.0, 0.0),
        background: None,
        children: vec![child],
        float: css::Float::None,
        clears: false,
        position: css::Position::Static,
        inset: (None, None, None, None),
        inset_pct: (None, None, None, None),
        width: css::WidthVal::Pct(weight),
        height: css::WidthVal::Auto,
        node: None,
        center: false,
    }
}

/// One grid row: a Row container spacing its cells by the column `gap`.
fn row_box(cells: Vec<Block>, gap: f32) -> Block {
    Block::Container {
        direction: css::FlexDirection::Row,
        justify: css::Justify::Start,
        align: css::Align::Stretch,
        gap,
        margin: (0.0, 0.0, 0.0, 0.0),
        padding: (0.0, 0.0, 0.0, 0.0),
        background: None,
        children: cells,
        float: css::Float::None,
        clears: false,
        position: css::Position::Static,
        inset: (None, None, None, None),
        inset_pct: (None, None, None, None),
        width: css::WidthVal::Auto,
        height: css::WidthVal::Auto,
        node: None,
        center: false,
    }
}
