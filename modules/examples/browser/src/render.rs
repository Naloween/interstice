//! Page build: parse the document + cascade, tear down the previous page, and
//! recursively materialise the resulting `Block` tree into `UiElement`s under the
//! viewport (the box-model / flex / float / position mapping onto the engine).

use interstice_sdk::*;

use crate::html::Block;
use crate::page::teardown_viewport;
use crate::tables::*;
use crate::ui;
use crate::ui::*;
use crate::widgets::{image_placeholder_el, space_el, text_el};
use crate::{css, html, url};

/// Build the viewport from the document (`html` served by `host`/`path`/`tls`) and
/// the resolved cascade (`sheets`, lowest priority first). Bumps the generation so
/// freshly created elements get collision-free ids. Mutates `nav` (generation +
/// next request id); the caller writes it back. The document is passed in rather
/// than read from `PageDoc` because, on the immediate (no-external-CSS) path, that
/// row was just inserted this run and isn't yet visible to a read.
#[allow(clippy::too_many_arguments)]
pub(crate) fn rebuild<Caps>(
    ctx: &ReducerContext<Caps>,
    nav: &mut NavState,
    html: &str,
    host: &str,
    path: &str,
    tls: bool,
    sheets: &[String],
) where
    Caps: CanRead<ui::UiElement>
        + CanInsert<ui::UiElement>
        + CanUpdate<ui::UiElement>
        + CanDelete<ui::UiElement>
        + CanRead<LinkMap>
        + CanDelete<LinkMap>
        + CanRead<ImageReq>
        + CanInsert<ImageReq>
        + CanDelete<ImageReq>
        + CanRead<ImageWaiter>
        + CanInsert<ImageWaiter>
        + CanDelete<ImageWaiter>
        + CanInsert<ImageFetchQueue>,
{
    let stylesheet = css::parse_all(sheets);
    let blocks = html::group_floats(html::parse_html(html, &stylesheet));

    teardown_viewport(ctx);

    nav.nav_gen += 1;
    let generation = nav.nav_gen;

    // Maps a resolved image URL to the in-flight request id fetching it, so repeated
    // images on the page share a single fetch + decoded texture.
    let mut url_reqs: Vec<(String, u64)> = Vec::new();
    let mut next_req = nav.next_req_id;
    let mut counter: u32 = 0;
    render_blocks(
        ctx,
        host,
        path,
        tls,
        generation,
        VIEWPORT_ID,
        &blocks,
        &mut url_reqs,
        &mut next_req,
        &mut counter,
        false,
    );

    nav.next_req_id = next_req;
}

/// Recursively materialise `blocks` as `UiElement`s under `parent`. A flex
/// `Container` becomes a container element with its children laid out beneath
/// it; every other block is a direct child of `parent`. `counter` hands out a
/// process-unique id suffix per element so ids stay distinct across nesting,
/// while `order` (the per-sibling index) drives layout order within a parent.
#[allow(clippy::too_many_arguments)]
fn render_blocks<Caps>(
    ctx: &ReducerContext<Caps>,
    host: &str,
    path: &str,
    tls: bool,
    generation: u32,
    parent: &str,
    blocks: &[Block],
    url_reqs: &mut Vec<(String, u64)>,
    next_req: &mut u64,
    counter: &mut u32,
    // True when `parent` is a flex row: an auto-width child is then content-sized
    // (so `justify-content` has slack to distribute) rather than filling the row.
    flex_row: bool,
) where
    Caps: CanRead<ui::UiElement>
        + CanInsert<ui::UiElement>
        + CanUpdate<ui::UiElement>
        + CanDelete<ui::UiElement>
        + CanRead<ImageReq>
        + CanInsert<ImageReq>
        + CanDelete<ImageReq>
        + CanRead<ImageWaiter>
        + CanInsert<ImageWaiter>
        + CanDelete<ImageWaiter>
        + CanInsert<ImageFetchQueue>,
{
    for (i, block) in blocks.iter().enumerate() {
        let id = format!("c{generation}_{}", *counter);
        *counter += 1;
        match block {
            Block::Text {
                text,
                size,
                color,
                bold,
                italic,
                align,
                background,
                margin,
                padding,
                width,
                height,
                line_height,
                border_w,
                border_c,
                spans,
                ..
            } => {
                // Inline links live as spans on the element now; hit-testing uses
                // `ui::link_at` (no per-element LinkMap row needed).
                let spans: Vec<ui::TextSpan> = spans
                    .iter()
                    .map(|s| ui::TextSpan {
                        start: s.start,
                        end: s.end,
                        color: s.color,
                        href: s.href.clone(),
                        bold: s.bold,
                        italic: s.italic,
                    })
                    .collect();
                let w = match width {
                    css::WidthVal::Auto if flex_row => Size::Fit,
                    css::WidthVal::Auto => Size::Grow,
                    css::WidthVal::Px(px) => Size::Fixed(px.max(0.0)),
                    css::WidthVal::Pct(f) => Size::Percent(*f),
                };
                // `height: auto` is the default (content-sized); an explicit
                // px/percent height fixes the box.
                let h = match height {
                    css::WidthVal::Auto => Size::Fit,
                    css::WidthVal::Px(px) => Size::Fixed(px.max(0.0)),
                    css::WidthVal::Pct(f) => Size::Percent(*f),
                };
                ui::create_element(
                    ctx,
                    text_el(
                        id.clone(),
                        parent.to_string(),
                        i as u32,
                        text.clone(),
                        *size,
                        *color,
                        *bold,
                        *italic,
                        *align,
                        *background,
                        *margin,
                        *padding,
                        w,
                        h,
                        *line_height,
                        *border_w,
                        *border_c,
                        spans,
                    ),
                );
            }
            Block::Space { height } => {
                ui::create_element(ctx, space_el(id, parent.to_string(), i as u32, *height));
            }
            Block::Image {
                url,
                inline_svg,
                position,
                inset,
                ..
            } => {
                // Inline `<svg>`: rasterize the serialized source right here and
                // attach the texture — no fetch, no waiter bookkeeping.
                if let Some(src) = inline_svg {
                    if let Some((tex_id, dw, dh)) =
                        crate::images::place_inline_svg(ctx, &id, src.as_bytes())
                    {
                        let mut el =
                            image_placeholder_el(id.clone(), parent.to_string(), i as u32);
                        el.width = Size::Fixed(dw);
                        el.height = Size::Fixed(dh);
                        el.background_color = TRANSPARENT;
                        el.image = Some(tex_id);
                        apply_position(&mut el, *position, *inset);
                        ui::create_element(ctx, el);
                    }
                    continue;
                }
                let Some(loc) = url::resolve(host, path, tls, url) else {
                    continue;
                };
                let key = loc.to_url();
                // Reuse an existing fetch for the same URL, or start a new one —
                // but cap distinct fetches per page so a pathological page (e.g.
                // hundreds of unique-URL images) can't exhaust the GPU or flood the
                // broker. Repeated images share a fetch and don't count against it.
                let req_id = match url_reqs.iter().find(|(u, _)| *u == key) {
                    Some((_, rid)) => *rid,
                    None => {
                        let rid = *next_req;
                        *next_req += 1;
                        url_reqs.push((key, rid));
                        let _ = ctx.current.tables.imagereq().insert(ImageReq {
                            req_id: rid,
                            url: url.clone(),
                        });
                        // Queue the fetch — DO NOT call http_get here (re-entrant
                        // into the broker from on_http; see ImageFetchQueue docs).
                        let _ = ctx.current.tables.imagefetchqueue().insert(ImageFetchQueue {
                            req_id: rid,
                            host: loc.host,
                            path: loc.path,
                            tls: loc.tls,
                        });
                        rid
                    }
                };
                let mut img_el = image_placeholder_el(id.clone(), parent.to_string(), i as u32);
                apply_position(&mut img_el, *position, *inset);
                ui::create_element(ctx, img_el);
                let _ = ctx.current.tables.imagewaiter().insert(ImageWaiter {
                    element_id: id,
                    req_id,
                });
            }
            Block::Container {
                direction,
                justify,
                align,
                gap,
                margin,
                padding,
                background,
                children,
                position,
                inset,
                width,
                ..
            } => {
                let mut el = container_el(
                    id.clone(),
                    parent.to_string(),
                    i as u32,
                    map_direction(*direction),
                    map_justify(*justify),
                    map_align(*align),
                    *gap,
                    *margin,
                    *padding,
                    *background,
                    flex_row,
                );
                // An explicit width (table cells) overrides the default flex
                // sizing so columns line up across rows.
                match width {
                    css::WidthVal::Auto => {}
                    css::WidthVal::Px(px) => el.width = Size::Fixed(px.max(0.0)),
                    css::WidthVal::Pct(f) => el.width = Size::Percent(*f),
                }
                apply_position(&mut el, *position, *inset);
                ui::create_element(ctx, el);
                render_blocks(
                    ctx,
                    host,
                    path,
                    tls,
                    generation,
                    &id,
                    children,
                    url_reqs,
                    next_req,
                    counter,
                    *direction == css::FlexDirection::Row,
                );
            }
            Block::FloatRow {
                side,
                float_box,
                flow,
            } => {
                let zero = (0.0, 0.0, 0.0, 0.0);
                // Row placing the float beside the following in-flow content. A
                // gutter separates the two columns.
                ui::create_element(
                    ctx,
                    container_el(
                        id.clone(),
                        parent.to_string(),
                        i as u32,
                        LayoutDirection::Row,
                        JustifyContent::Start,
                        AlignItems::Start,
                        12.0,
                        zero,
                        zero,
                        None,
                        flex_row,
                    ),
                );
                let (float_order, flow_order) = match side {
                    css::Float::Right => (1u32, 0u32),
                    _ => (0u32, 1u32),
                };
                // Float wrapper: shrink-to-fit, keeping the float's intrinsic width.
                let fw_id = format!("c{generation}_{}", *counter);
                *counter += 1;
                ui::create_element(
                    ctx,
                    container_el(
                        fw_id.clone(),
                        id.clone(),
                        float_order,
                        LayoutDirection::Column,
                        JustifyContent::Start,
                        AlignItems::Start,
                        0.0,
                        zero,
                        zero,
                        None,
                        true,
                    ),
                );
                render_blocks(
                    ctx,
                    host,
                    path,
                    tls,
                    generation,
                    &fw_id,
                    std::slice::from_ref(float_box.as_ref()),
                    url_reqs,
                    next_req,
                    counter,
                    true,
                );
                // Flow wrapper: grows to fill the width remaining beside the float.
                let cw_id = format!("c{generation}_{}", *counter);
                *counter += 1;
                ui::create_element(
                    ctx,
                    container_el(
                        cw_id.clone(),
                        id.clone(),
                        flow_order,
                        LayoutDirection::Column,
                        JustifyContent::Start,
                        AlignItems::Stretch,
                        0.0,
                        zero,
                        zero,
                        None,
                        false,
                    ),
                );
                render_blocks(
                    ctx,
                    host,
                    path,
                    tls,
                    generation,
                    &cw_id,
                    flow,
                    url_reqs,
                    next_req,
                    counter,
                    false,
                );
            }
        }
    }
}

/// A flex container element (`display:flex`). Block-level, so it fills the
/// available width and sizes its height to content; its children lay out along
/// `direction` with `justify`/`align`/`gap` from the CSS engine.
#[allow(clippy::too_many_arguments)]
fn container_el(
    id: String,
    parent: String,
    order: u32,
    direction: LayoutDirection,
    justify: JustifyContent,
    align: AlignItems,
    gap: f32,
    margin: (f32, f32, f32, f32),
    padding: (f32, f32, f32, f32),
    background: Option<(f32, f32, f32, f32)>,
    // True when nested inside a flex row: size to content instead of filling.
    flex_row: bool,
) -> UiElement {
    let zero = (0.0, 0.0, 0.0, 0.0);
    UiElement {
        id,
        parent: Some(parent),
        order,
        width: if flex_row { Size::Fit } else { Size::Grow },
        height: Size::Fit,
        layout_direction: direction,
        justify_content: justify,
        align_items: align,
        gap,
        padding_sides: if padding != zero { Some(padding) } else { None },
        margin_sides: if margin != zero { Some(margin) } else { None },
        background_color: background.unwrap_or(TRANSPARENT),
        ..Default::default()
    }
}

/// Map the browser CSS flex enums onto the UI engine's layout enums.
fn map_direction(d: css::FlexDirection) -> LayoutDirection {
    match d {
        css::FlexDirection::Row => LayoutDirection::Row,
        css::FlexDirection::Column => LayoutDirection::Column,
    }
}

fn map_justify(j: css::Justify) -> JustifyContent {
    match j {
        css::Justify::Start => JustifyContent::Start,
        css::Justify::Center => JustifyContent::Center,
        css::Justify::End => JustifyContent::End,
        css::Justify::SpaceBetween => JustifyContent::SpaceBetween,
        css::Justify::SpaceAround => JustifyContent::SpaceAround,
        css::Justify::SpaceEvenly => JustifyContent::SpaceEvenly,
    }
}

fn map_align(a: css::Align) -> AlignItems {
    match a {
        css::Align::Start => AlignItems::Start,
        css::Align::Center => AlignItems::Center,
        css::Align::End => AlignItems::End,
        css::Align::Stretch => AlignItems::Stretch,
    }
}

fn map_position(p: css::Position) -> Position {
    match p {
        css::Position::Static => Position::Static,
        css::Position::Relative => Position::Relative,
        css::Position::Absolute => Position::Absolute,
    }
}

/// Stamp CSS `position` + `(top, right, bottom, left)` offsets onto an element.
fn apply_position(
    el: &mut UiElement,
    position: css::Position,
    inset: (Option<f32>, Option<f32>, Option<f32>, Option<f32>),
) {
    el.position = map_position(position);
    let (t, r, b, l) = inset;
    el.pos_top = t;
    el.pos_right = r;
    el.pos_bottom = b;
    el.pos_left = l;
}
