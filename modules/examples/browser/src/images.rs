//! Image handling: decode a completed fetch (PNG/JPEG/GIF/WebP via `image`, with
//! an SVG rasterization fallback), upload it as a shared texture, and attach it to
//! every element that was waiting on that URL.

use interstice_sdk::*;

use crate::bindings::graphics::*;
use crate::tables::*;
use crate::ui;
use crate::ui::*;

/// Font database for SVG `<text>`, built once and shared (Arc) across every SVG
/// decode. We ship a single embedded face (DejaVu Sans — broad Unicode coverage,
/// redistributable) and point all the generic family slots at it, since we have
/// no system fonts in wasm.
fn svg_fontdb() -> std::sync::Arc<resvg::usvg::fontdb::Database> {
    use std::sync::{Arc, OnceLock};
    static DB: OnceLock<Arc<resvg::usvg::fontdb::Database>> = OnceLock::new();
    DB.get_or_init(|| {
        // Reuse the engine's embedded face (interstice-ui) rather than shipping a
        // second copy in this module.
        let mut db = resvg::usvg::fontdb::Database::new();
        db.load_font_data(interstice_ui::FONT_TTF.to_vec());
        db.set_sans_serif_family("DejaVu Sans");
        db.set_serif_family("DejaVu Sans");
        db.set_monospace_family("DejaVu Sans");
        db.set_cursive_family("DejaVu Sans");
        db.set_fantasy_family("DejaVu Sans");
        Arc::new(db)
    })
    .clone()
}

/// Rasterize an SVG document to straight-alpha RGBA8, returning `(w, h, rgba)`,
/// or `None` if the bytes aren't valid SVG. Used as the fallback when the raster
/// decoders reject an image — Wikipedia and friends serve many icons/logos as
/// SVG. Dimensions come from the SVG's intrinsic size, capped so a large viewBox
/// can't allocate an enormous texture.
fn decode_svg(bytes: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    use resvg::{tiny_skia, usvg};

    let mut opt = usvg::Options::default();
    opt.fontdb = svg_fontdb();
    // Generic CSS families (sans-serif/serif/…) all resolve to our one embedded
    // face, and it's the fallback when an SVG names a font we don't ship.
    opt.font_family = "DejaVu Sans".to_string();
    let tree = usvg::Tree::from_data(bytes, &opt).ok()?;
    let size = tree.size();
    const MAX_DIM: f32 = 1024.0;
    let longest = size.width().max(size.height());
    if longest <= 0.0 {
        return None;
    }
    let scale = (MAX_DIM / longest).min(1.0);
    let w = ((size.width() * scale).ceil() as u32).max(1);
    let h = ((size.height() * scale).ceil() as u32).max(1);

    let mut pixmap = tiny_skia::Pixmap::new(w, h)?;
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );

    // tiny-skia stores premultiplied alpha; the texture pipeline blends with
    // straight alpha, so un-premultiply each pixel.
    let mut rgba = pixmap.take();
    for px in rgba.chunks_exact_mut(4) {
        let a = px[3] as u32;
        if a > 0 && a < 255 {
            for c in &mut px[0..3] {
                *c = ((*c as u32 * 255 + a / 2) / a).min(255) as u8;
            }
        }
    }
    Some((w, h, rgba))
}

/// Handle a completed image fetch for `req_id`: decode it once into a shared
/// texture, then attach that texture to every element that was waiting on it. If
/// the fetch errored or the bytes don't decode, just drop the waiters (the
/// placeholder stays). Either way the `ImageWaiter` rows for this request are
/// cleared.
pub(crate) fn place_image<Caps>(ctx: &ReducerContext<Caps>, req_id: u64, bytes: &[u8], ok: bool)
where
    Caps: CanRead<ui::UiElement>
        + CanUpdate<ui::UiElement>
        + CanRead<ImageWaiter>
        + CanDelete<ImageWaiter>,
{
    // Which elements were waiting on this fetch.
    let waiters: Vec<String> = ctx
        .current
        .tables
        .imagewaiter()
        .scan()
        .into_iter()
        .filter(|w| w.req_id == req_id)
        .map(|w| w.element_id)
        .collect();
    for id in &waiters {
        let _ = ctx.current.tables.imagewaiter().delete(id.clone());
    }

    // Decode to straight-alpha RGBA8. Try the raster decoders first (PNG/JPEG);
    // if those don't recognise the bytes, fall back to SVG rasterization, since
    // sites like Wikipedia serve many icons/logos as SVG (which `image` can't
    // decode).
    let raster = if ok { image::load_from_memory(bytes).ok() } else { None };
    let (w, h, raw) = if let Some(img) = raster {
        let rgba = img.to_rgba8();
        let (w, h) = rgba.dimensions();
        (w, h, rgba.into_raw())
    } else if let Some(svg) = if ok { decode_svg(bytes) } else { None } {
        svg
    } else {
        let fmt = image::guess_format(bytes).ok();
        ctx.log(&format!(
            "browser: image not shown req={req_id} ok={ok} bytes={} fmt={:?}",
            bytes.len(),
            fmt
        ));
        return;
    };
    if w == 0 || h == 0 {
        return;
    }

    // One texture per distinct fetch, shared by every waiter.
    let tex_id = format!("imgtex_{req_id}");
    let _ = ctx.graphics().reducers.create_texture(
        tex_id.clone(),
        TextureDescriptorInput {
            width: w,
            height: h,
            format: "rgba8unorm".to_string(),
            mip_levels: 1,
            sample_count: 1,
            usage: TextureUsageFlags {
                copy_src: false,
                copy_dst: true,
                texture_binding: true,
                storage_binding: false,
                render_attachment: false,
            },
        },
        raw,
    );

    let (dw, dh) = display_size(w, h);
    for id in &waiters {
        // Only attach if the element still exists (the user may have navigated away).
        if let Some(mut el) = ctx.current.tables.uielement().get(id.clone()) {
            el.width = Size::Fixed(dw);
            el.height = Size::Fixed(dh);
            el.background_color = TRANSPARENT;
            el.image = Some(tex_id.clone());
            let _ = ctx.current.tables.uielement().update(el);
        }
    }
}

/// Rasterize an inline `<svg>` immediately (no fetch) and upload it as a texture
/// keyed to `element_id`. Returns `(texture_id, display_w, display_h)` for the
/// caller to stamp onto the element, or `None` if the source isn't valid SVG.
/// Unlike [`place_image`] there are no waiters: the source is already in hand at
/// render time, so we decode + upload synchronously.
pub(crate) fn place_inline_svg<Caps>(
    ctx: &ReducerContext<Caps>,
    element_id: &str,
    bytes: &[u8],
) -> Option<(String, f32, f32)> {
    let (w, h, raw) = decode_svg(bytes)?;
    if w == 0 || h == 0 {
        return None;
    }
    let tex_id = format!("svgtex_{element_id}");
    let _ = ctx.graphics().reducers.create_texture(
        tex_id.clone(),
        TextureDescriptorInput {
            width: w,
            height: h,
            format: "rgba8unorm".to_string(),
            mip_levels: 1,
            sample_count: 1,
            usage: TextureUsageFlags {
                copy_src: false,
                copy_dst: true,
                texture_binding: true,
                storage_binding: false,
                render_attachment: false,
            },
        },
        raw,
    );
    let (dw, dh) = display_size(w, h);
    Some((tex_id, dw, dh))
}

/// Cap image display width so large images don't blow out the layout; height
/// scales proportionally.
fn display_size(w: u32, h: u32) -> (f32, f32) {
    const MAX_W: f32 = 600.0;
    let (wf, hf) = (w as f32, h as f32);
    if wf <= MAX_W {
        (wf, hf)
    } else {
        (MAX_W, hf * (MAX_W / wf))
    }
}
