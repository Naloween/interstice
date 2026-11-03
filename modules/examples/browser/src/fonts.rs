//! Embedded fonts. A wasm module has no access to the OS font directory, so
//! every face the engine may need is compiled in and registered with Parley.
//!
//! Generic families follow what Firefox on a stock Linux desktop resolves via
//! fontconfig: DejaVu for `sans-serif` / `serif` / `monospace`. The Liberation
//! faces (metric-compatible with Arial / Times New Roman) are registered both
//! under their own names and under the proprietary names sites commonly ask for,
//! mirroring fontconfig's standard aliases. Droid Sans Fallback covers CJK.

use parley::fontique::{Blob, FamilyId, FontInfoOverride, GenericFamily, Script};
use std::sync::Arc;

macro_rules! font {
    ($f:literal) => {
        include_bytes!(concat!("../assets/fonts/", $f)) as &'static [u8]
    };
}

const DEJAVU_SANS: &[&[u8]] = &[
    font!("DejaVuSans.ttf"),
    font!("DejaVuSans-Bold.ttf"),
    font!("DejaVuSans-Oblique.ttf"),
    font!("DejaVuSans-BoldOblique.ttf"),
];
const DEJAVU_SERIF: &[&[u8]] = &[
    font!("DejaVuSerif.ttf"),
    font!("DejaVuSerif-Bold.ttf"),
    font!("DejaVuSerif-Italic.ttf"),
    font!("DejaVuSerif-BoldItalic.ttf"),
];
const DEJAVU_MONO: &[&[u8]] = &[font!("DejaVuSansMono.ttf"), font!("DejaVuSansMono-Bold.ttf")];
const LIBERATION_SANS: &[&[u8]] = &[
    font!("LiberationSans-Regular.ttf"),
    font!("LiberationSans-Bold.ttf"),
    font!("LiberationSans-Italic.ttf"),
    font!("LiberationSans-BoldItalic.ttf"),
];
const LIBERATION_SERIF: &[&[u8]] = &[
    font!("LiberationSerif-Regular.ttf"),
    font!("LiberationSerif-Bold.ttf"),
    font!("LiberationSerif-Italic.ttf"),
    font!("LiberationSerif-BoldItalic.ttf"),
];
const CJK: &[u8] = font!("DroidSansFallbackFull.ttf");

/// Build a Parley font context holding every embedded face. Cheap to clone
/// (the collection shares its data), so one is built per document.
pub fn font_context() -> parley::FontContext {
    thread_local! {
        static BASE: parley::FontContext = build();
    }
    BASE.with(|b| b.clone())
}

fn build() -> parley::FontContext {
    let mut fc = parley::FontContext::default();
    let coll = &mut fc.collection;

    let mut reg = |faces: &[&'static [u8]], name: Option<&str>| -> FamilyId {
        let mut id = None;
        for &f in faces {
            let over = name.map(|n| FontInfoOverride { family_name: Some(n), ..Default::default() });
            let r = coll.register_fonts(Blob::new(Arc::new(f)), over);
            if id.is_none() {
                id = r.first().map(|(fid, _)| *fid);
            }
        }
        id.expect("embedded font failed to load")
    };

    let sans = reg(DEJAVU_SANS, None);
    let serif = reg(DEJAVU_SERIF, None);
    let mono = reg(DEJAVU_MONO, None);
    reg(LIBERATION_SANS, None);
    reg(LIBERATION_SERIF, None);
    for alias in ["Arial", "Helvetica", "Helvetica Neue", "Segoe UI", "Roboto", "Verdana", "Tahoma"] {
        reg(LIBERATION_SANS, Some(alias));
    }
    for alias in ["Times New Roman", "Times", "Georgia"] {
        reg(LIBERATION_SERIF, Some(alias));
    }
    for alias in ["Courier New", "Courier", "Menlo", "Consolas", "Monaco"] {
        reg(DEJAVU_MONO, Some(alias));
    }
    let cjk = reg(&[CJK], None);

    coll.set_generic_families(GenericFamily::SansSerif, [sans, cjk].into_iter());
    coll.set_generic_families(GenericFamily::SystemUi, [sans, cjk].into_iter());
    coll.set_generic_families(GenericFamily::UiSansSerif, [sans].into_iter());
    coll.set_generic_families(GenericFamily::Serif, [serif, cjk].into_iter());
    coll.set_generic_families(GenericFamily::UiSerif, [serif].into_iter());
    coll.set_generic_families(GenericFamily::Monospace, [mono].into_iter());
    coll.set_generic_families(GenericFamily::UiMonospace, [mono].into_iter());
    coll.set_generic_families(GenericFamily::Cursive, [serif].into_iter());
    coll.set_generic_families(GenericFamily::Fantasy, [serif].into_iter());
    coll.set_generic_families(GenericFamily::Emoji, [sans].into_iter());

    for s in [*b"Hani", *b"Hira", *b"Kana", *b"Hang", *b"Bopo", *b"Jpan", *b"Kore", *b"Hans", *b"Hant"] {
        coll.set_fallbacks(Script::from_bytes(s), [cjk].into_iter());
    }
    for s in [*b"Latn", *b"Grek", *b"Cyrl", *b"Zyyy", *b"Zinh", *b"Arab", *b"Hebr", *b"Armn", *b"Geor"] {
        coll.set_fallbacks(Script::from_bytes(s), [sans].into_iter());
    }
    fc
}
