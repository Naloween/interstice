//! A small but real CSS engine: tokenize stylesheets into rules, match selectors
//! (type / `.class` / `#id` / `*` / descendant / child `>` / grouping, attribute
//! `[a=v]`, and structural pseudo-classes `:first-child`/`:nth-child()`/`:not()`/…)
//! with specificity, and resolve the cascade for an element into a set of property
//! overrides. Sibling combinators (`+`/`~`) are approximated as descendant (no
//! sibling context in the ancestor walk); dynamic pseudos (`:hover`/`:focus`/
//! `:visited`/…) and pseudo-elements (`::before`) never match statically. Only a
//! handful of properties are surfaced. `@media`/`@supports` blocks are flattened
//! (their inner rules always apply); other at-rules are skipped.

pub type Rgba = (f32, f32, f32, f32);

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Display {
    Block,
    Inline,
    InlineBlock,
    /// A flex container — its children are laid out along the main axis with
    /// `flex-direction` / `justify-content` / `align-items` (see [`FlexDirection`]
    /// etc.). `inline-flex` collapses to this too.
    Flex,
    /// A grid container (`display:grid`). Children flow row-major into the columns
    /// described by `grid-template-columns` (see [`Applied::grid_columns`]);
    /// `inline-grid` collapses to this too.
    Grid,
    None,
}

/// CSS `flex-direction` (only the two non-reversed axes are modelled).
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum FlexDirection {
    Row,
    Column,
}

/// CSS `justify-content` — main-axis distribution of flex children.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Justify {
    Start,
    Center,
    End,
    SpaceBetween,
    SpaceAround,
    SpaceEvenly,
}

/// CSS `align-items` — cross-axis alignment of flex children.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Align {
    Start,
    Center,
    End,
    Stretch,
}

/// CSS `float`. `None` ⇒ in normal flow.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Float {
    None,
    Left,
    Right,
}

/// CSS `position`. `fixed` maps to `Absolute` and `sticky` to `Relative`.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Position {
    Static,
    Relative,
    Absolute,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum TextAlign {
    Left,
    Center,
    Right,
}

impl TextAlign {
    /// Alignment factor for `UiElement::text_align` (0 left / 0.5 centre / 1 right).
    pub fn factor(self) -> f32 {
        match self {
            TextAlign::Left => 0.0,
            TextAlign::Center => 0.5,
            TextAlign::Right => 1.0,
        }
    }
}

/// An element's identity for selector matching.
pub struct ElemCtx {
    pub tag: String,
    pub id: Option<String>,
    pub classes: Vec<String>,
    /// Attributes (name lowercased, raw value) for `[attr]` / `[attr=v]` selectors.
    pub attrs: Vec<(String, String)>,
    /// 1-based position among element siblings, and the sibling count — for
    /// `:first-child` / `:last-child` / `:only-child` / `:nth-child()`. `0` count
    /// ⇒ unknown (root/synthetic); structural pseudos then don't match.
    pub index: usize,
    pub count: usize,
    /// 1-based position among same-tag siblings, and that count — for the
    /// `*-of-type` structural pseudos.
    pub type_index: usize,
    pub type_count: usize,
    /// `true` for the document root (`<html>`), for `:root`.
    pub is_root: bool,
}

/// A resolved CSS `width` value.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum WidthVal {
    /// Absolute pixels.
    Px(f32),
    /// A fraction (0.0–1.0) of the containing block's width.
    Pct(f32),
    /// `auto` — fill the available width (the default block behaviour).
    Auto,
    /// Shrink-to-fit: size the box to its content (CSS `inline-block` / a
    /// floated or `fit-content` box). Not produced by `parse_width`; synthesized
    /// for inline-block boxes so they don't stretch to their container.
    Fit,
}

/// A resolved `line-height`. Kept distinct from a plain px length because a
/// *unitless* line-height (`line-height: 1.5`) inherits as the **multiplier**
/// and re-resolves against each descendant's own font-size — so a 30px wordmark
/// under `body{line-height:1.5}` gets a 45px line box, not the body's 21px one.
/// A length/percentage resolves to a fixed px at parse time and inherits as-is.
#[derive(Clone, Copy, Debug)]
pub enum LineHeight {
    /// Unitless multiplier of the element's own font-size.
    Scale(f32),
    /// A fixed length in px.
    Px(f32),
}

/// Property overrides resolved from the author cascade for one element. `None`
/// ⇒ the property wasn't set by author CSS (keep the UA / inherited value).
/// `margin`/`padding` are per-side `[top, right, bottom, left]`, each side
/// independently set so the longhand (`margin-left`) merges over the shorthand.
#[derive(Default, Clone)]
pub struct Applied {
    pub color: Option<Rgba>,
    pub background: Option<Rgba>,
    pub font_size: Option<f32>,
    /// `line-height`: a unitless [`LineHeight::Scale`] (re-resolves per element)
    /// or a fixed [`LineHeight::Px`]. Resolved to px at text-block emission.
    pub line_height: Option<LineHeight>,
    /// `height`, reusing the same Px/Pct/Auto model as `width`.
    pub height: Option<WidthVal>,
    /// `max-height` resolved to px. Only the collapse idiom matters to us:
    /// `max-height:0` (paired with `overflow:hidden`) is the standard accordion
    /// "closed" state (e.g. the Wikipedia language list), which we treat as
    /// `display:none`. A non-zero cap is recorded but otherwise ignored.
    pub max_height: Option<f32>,
    /// `max-width`, reusing the Px/Pct model as `width`. Recorded mainly as a
    /// signal that a width-less `inline-block` is a structural box (e.g. the
    /// footer's `.other-project-text{max-width:65%}`) so it reifies and can sit
    /// beside its inline-level siblings, rather than folding into a text run.
    pub max_width: Option<WidthVal>,
    /// `font-weight`: `true` ⇒ bold (≥600 / `bold`/`bolder`), `false` ⇒ normal.
    pub font_weight: Option<bool>,
    /// `font-style`: `true` ⇒ italic (`italic`/`oblique`), `false` ⇒ normal.
    pub font_style: Option<bool>,
    pub text_align: Option<TextAlign>,
    pub display: Option<Display>,
    pub margin: [Option<f32>; 4],
    /// Which margin sides were authored as `auto` (`[t, r, b, l]`). `margin-left`
    /// *and* `margin-right` both `auto` on a definite-width block ⇒ the block is
    /// centered in its containing block (the `margin: 0 auto` idiom).
    pub margin_auto: [bool; 4],
    pub padding: [Option<f32>; 4],
    pub width: Option<WidthVal>,
    pub border_width: Option<f32>,
    pub border_color: Option<Rgba>,
    pub flex_direction: Option<FlexDirection>,
    pub justify: Option<Justify>,
    pub align: Option<Align>,
    /// `gap` (or `row-gap`/`column-gap`) in px — child spacing in a flex box.
    pub gap: Option<f32>,
    /// `float: left | right | none`.
    pub float: Option<Float>,
    /// `clear: left | right | both` ⇒ `true`; `none` ⇒ `false`. Breaks the
    /// preceding float context so this box drops below the float.
    pub clear: Option<bool>,
    /// `position`.
    pub position: Option<Position>,
    /// `top` / `right` / `bottom` / `left` offsets in px (`[t, r, b, l]`).
    pub inset: [Option<f32>; 4],
    /// The same four sides expressed as *percentages* (fractions, `[t, r, b, l]`),
    /// for offsets given as `%`. Resolved against the containing block at layout
    /// time by the engine; kept separate from `inset` (px) so both can coexist.
    pub inset_pct: [Option<f32>; 4],
    /// `grid-template-columns` resolved to per-column *weights* that sum to 1 (their
    /// relative share of the container width). `len()` is the column count. `None`
    /// unless this is a grid with an explicit template (see [`parse_grid_columns`]).
    pub grid_columns: Option<Vec<f32>>,
    /// The CSS `content` string (quotes stripped, escapes/`\NN` decoded). Only
    /// meaningful on a `::before`/`::after` [`Stylesheet::pseudo`] result; `None`
    /// (or `content: none`) ⇒ no generated box is produced.
    pub content: Option<String>,
    /// A single `background-image: url(...)` (the raw URL, resolved downstream like
    /// an `<img src>`). Only captured for plain-URL backgrounds — gradient values
    /// (including the sprite-atlas `linear-gradient(...),url(sprite.svg)` loading
    /// trick) are ignored, so this holds a genuine standalone image (e.g. a sized
    /// icon painted via CSS) rather than a sprite sheet.
    pub background_image: Option<String>,
    /// A sprite-sheet URL captured from the CSS-sprite loading trick, where the
    /// background is `linear-gradient(...),url(sheet)` (the gradient forces a
    /// repaint path in real browsers; here it just flags "this url is a sprite
    /// atlas, not a standalone image"). Combined with [`Self::background_position`]
    /// and a fixed width/height, it crops one icon cell out of the shared sheet.
    pub sprite_atlas: Option<String>,
    /// `background-position` as `[x, y]` in CSS px (keywords `left`/`top` ⇒ 0).
    /// Negative values shift the sheet, so the visible cell begins at `-position`.
    pub background_position: [Option<f32>; 2],
    /// Whether `background-size` was authored. Sprite cropping assumes the sheet is
    /// painted at its intrinsic size (UV = crop / intrinsic); a `background-size`
    /// rescales that space (e.g. the wordmark's `background-size:100%`), so we
    /// suppress the sprite crop when this is set rather than crop the wrong cell.
    pub background_size_set: bool,
}

impl Applied {
    /// Overlay `o` on top of `self` (later/higher-priority wins per-property).
    pub fn overlay(&mut self, o: &Applied) {
        if o.color.is_some() {
            self.color = o.color;
        }
        if o.background.is_some() {
            self.background = o.background;
        }
        if o.font_size.is_some() {
            self.font_size = o.font_size;
        }
        if o.line_height.is_some() {
            self.line_height = o.line_height;
        }
        if o.height.is_some() {
            self.height = o.height;
        }
        if o.max_height.is_some() {
            self.max_height = o.max_height;
        }
        if o.max_width.is_some() {
            self.max_width = o.max_width;
        }
        if o.font_weight.is_some() {
            self.font_weight = o.font_weight;
        }
        if o.font_style.is_some() {
            self.font_style = o.font_style;
        }
        if o.text_align.is_some() {
            self.text_align = o.text_align;
        }
        if o.display.is_some() {
            self.display = o.display;
        }
        for k in 0..4 {
            if o.margin[k].is_some() || o.margin_auto[k] {
                self.margin[k] = o.margin[k];
                self.margin_auto[k] = o.margin_auto[k];
            }
            if o.padding[k].is_some() {
                self.padding[k] = o.padding[k];
            }
        }
        if o.width.is_some() {
            self.width = o.width;
        }
        if o.border_width.is_some() {
            self.border_width = o.border_width;
        }
        if o.border_color.is_some() {
            self.border_color = o.border_color;
        }
        if o.flex_direction.is_some() {
            self.flex_direction = o.flex_direction;
        }
        if o.justify.is_some() {
            self.justify = o.justify;
        }
        if o.align.is_some() {
            self.align = o.align;
        }
        if o.gap.is_some() {
            self.gap = o.gap;
        }
        if o.float.is_some() {
            self.float = o.float;
        }
        if o.clear.is_some() {
            self.clear = o.clear;
        }
        if o.position.is_some() {
            self.position = o.position;
        }
        for k in 0..4 {
            if o.inset[k].is_some() {
                self.inset[k] = o.inset[k];
            }
            if o.inset_pct[k].is_some() {
                self.inset_pct[k] = o.inset_pct[k];
            }
        }
        if o.grid_columns.is_some() {
            self.grid_columns = o.grid_columns.clone();
        }
        if o.content.is_some() {
            self.content = o.content.clone();
        }
        if o.background_image.is_some() {
            self.background_image = o.background_image.clone();
        }
        if o.sprite_atlas.is_some() {
            self.sprite_atlas = o.sprite_atlas.clone();
        }
        if o.background_position[0].is_some() {
            self.background_position[0] = o.background_position[0];
        }
        if o.background_position[1].is_some() {
            self.background_position[1] = o.background_position[1];
        }
        if o.background_size_set {
            self.background_size_set = true;
        }
    }

    /// Resolved per-side margin in px `(top, right, bottom, left)`; unset → 0.
    pub fn margin_px(&self) -> (f32, f32, f32, f32) {
        let m = &self.margin;
        (
            m[0].unwrap_or(0.0),
            m[1].unwrap_or(0.0),
            m[2].unwrap_or(0.0),
            m[3].unwrap_or(0.0),
        )
    }
    /// Resolved per-side padding in px `(top, right, bottom, left)`; unset → 0.
    pub fn padding_px(&self) -> (f32, f32, f32, f32) {
        let p = &self.padding;
        (
            p[0].unwrap_or(0.0),
            p[1].unwrap_or(0.0),
            p[2].unwrap_or(0.0),
            p[3].unwrap_or(0.0),
        )
    }
}

/// An `[attr]` / `[attr op "value"]` attribute test.
#[derive(Clone)]
struct AttrSel {
    name: String,
    op: AttrOp,
    value: String,
}

#[derive(Clone, Copy, PartialEq)]
enum AttrOp {
    /// `[attr]` — present.
    Exists,
    /// `[attr=v]`.
    Eq,
    /// `[attr~=v]` — whitespace-separated word match.
    Includes,
    /// `[attr|=v]` — `v` or `v-…`.
    Dash,
    /// `[attr^=v]`.
    Prefix,
    /// `[attr$=v]`.
    Suffix,
    /// `[attr*=v]`.
    Substr,
}

/// A structural / functional pseudo-class we actually evaluate. Dynamic pseudos
/// (`:hover`, `:focus`, `:visited`, pseudo-elements, …) don't produce one of
/// these — instead the compound is flagged `never` so it can't match statically.
#[derive(Clone)]
enum Pseudo {
    FirstChild,
    LastChild,
    OnlyChild,
    Empty,
    Root,
    /// `:nth-child(an+b)` — coefficients `(a, b)`.
    NthChild(i32, i32),
    NthLastChild(i32, i32),
    FirstOfType,
    LastOfType,
    OnlyOfType,
    NthOfType(i32, i32),
    /// `:not(<simple>)` — negation of a single compound.
    Not(Box<Simple>),
}

/// The two generated-content pseudo-elements we support. A selector's subject
/// carrying one targets a synthetic box (before/after the element's content),
/// not the element itself, so such rules are matched only by the pseudo query.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PseudoEl {
    Before,
    After,
}

#[derive(Clone)]
struct Simple {
    /// `None` ⇒ universal (`*` or no type).
    tag: Option<String>,
    id: Option<String>,
    classes: Vec<String>,
    attrs: Vec<AttrSel>,
    pseudos: Vec<Pseudo>,
    /// A dynamic/unsupported pseudo (`:hover`, other `::x`, …) makes the whole
    /// compound unmatchable in static rendering.
    never: bool,
    /// `::before` / `::after` on this compound. When set, the compound matches a
    /// generated box rather than the element (see [`Stylesheet::pseudo`]).
    pseudo_el: Option<PseudoEl>,
}

/// How a compound relates to the one to its right in the selector chain.
#[derive(Clone, Copy, PartialEq)]
enum Combinator {
    /// Descendant (whitespace) — also the fallback for `+`/`~`.
    Descendant,
    /// Child (`>`) — must be the direct parent.
    Child,
}

struct Selector {
    /// Compound selectors outermost→subject (subject is last).
    parts: Vec<Simple>,
    /// `combos[k]` relates `parts[k]` to `parts[k+1]`; `len() == parts.len()-1`.
    combos: Vec<Combinator>,
    specificity: u32,
    /// The subject's `::before`/`::after`, lifted here for quick bucketing.
    pseudo_el: Option<PseudoEl>,
}

#[derive(Clone)]
struct Decl {
    prop: String,
    value: String,
}

struct Rule {
    selector: Selector,
    decls: Vec<Decl>,
    order: u32,
}

pub struct Stylesheet {
    rules: Vec<Rule>,
    // Candidate-rule indices keyed on the subject (rightmost) compound's most
    // selective discriminator, so resolving an element only tests rules that
    // could plausibly match it instead of scanning the whole sheet. Wikipedia &
    // friends ship many thousands of rules; without this the cascade is
    // O(elements × rules) and dominates page load. Each rule lands in exactly
    // one bucket (id ≻ class ≻ tag ≻ universal); the full `selector_matches`
    // still runs on every candidate, so the buckets only prune, never decide.
    by_id: std::collections::BTreeMap<String, Vec<usize>>,
    by_class: std::collections::BTreeMap<String, Vec<usize>>,
    by_tag: std::collections::BTreeMap<String, Vec<usize>>,
    universal: Vec<usize>,
    /// Indices of `::before`/`::after` rules, excluded from the normal buckets so
    /// they only apply through [`Stylesheet::pseudo`]. Small (generated content is
    /// rare relative to the whole sheet), so a flat scan per query is fine.
    pseudo_rules: Vec<usize>,
    /// Custom properties (`--name` → raw value) declared at document root scope
    /// (`:root`/`html`/`*`). Values may themselves reference other `var(...)`; we
    /// resolve them lazily/recursively at use. Nearly every modern design system
    /// (Wikipedia included) drives all colours/sizes/spacing through these, so
    /// without them the whole page falls back to UA defaults.
    vars: std::collections::HashMap<String, String>,
    /// The root element's font size in px (resolves `rem`). Defaults to 16; a
    /// `html{font-size:62.5%}` (the common "1rem = 10px" trick) makes it 10.
    root_font: f32,
}

// The root font size for the sheet currently being resolved, so the free
// `parse_len_px`/`calc` helpers can turn `rem` into px without threading it
// through every call site. Set at the top of `applied`/`inline`. The browser
// module is single-threaded wasm, but a thread-local keeps it sound.
thread_local! {
    static ROOT_FONT: std::cell::Cell<f32> = const { std::cell::Cell::new(16.0) };
}

fn root_font_px() -> f32 {
    ROOT_FONT.with(|c| c.get())
}

impl Stylesheet {
    /// Resolve the author cascade for `el` (with its `ancestors`, outermost
    /// first). `base_font` resolves relative `font-size` units (`em`/`%`).
    pub fn applied(&self, el: &ElemCtx, ancestors: &[ElemCtx], base_font: f32) -> Applied {
        ROOT_FONT.with(|c| c.set(self.root_font));
        // Gather only the rules whose subject discriminator this element carries
        // (its tag, id, each class) plus the always-eligible universal rules.
        let mut cand: Vec<usize> = self.universal.clone();
        if let Some(v) = self.by_tag.get(&el.tag.to_ascii_lowercase()) {
            cand.extend_from_slice(v);
        }
        if let Some(id) = &el.id {
            if let Some(v) = self.by_id.get(id) {
                cand.extend_from_slice(v);
            }
        }
        for c in &el.classes {
            if let Some(v) = self.by_class.get(c) {
                cand.extend_from_slice(v);
            }
        }

        let mut matched: Vec<&Rule> = cand
            .iter()
            .map(|&i| &self.rules[i])
            .filter(|r| selector_matches(&r.selector, el, ancestors))
            .collect();
        // Lowest priority first so later declarations overwrite earlier ones.
        matched.sort_by_key(|r| (r.selector.specificity, r.order));

        let mut a = Applied::default();
        for r in matched {
            for d in &r.decls {
                if d.prop.starts_with("--") {
                    continue; // custom-property definition, not a real property
                }
                let value = resolve_vars(&d.value, &self.vars, 0);
                apply_decl(&mut a, &d.prop, &value, base_font);
            }
        }
        a
    }

    /// Parse one inline `style="…"` attribute into overrides (highest priority),
    /// resolving `var(...)` against this sheet's root custom properties.
    pub fn inline(&self, style_attr: &str, base_font: f32) -> Applied {
        ROOT_FONT.with(|c| c.set(self.root_font));
        let mut a = Applied::default();
        for d in parse_decls(style_attr) {
            if d.prop.starts_with("--") {
                continue;
            }
            let value = resolve_vars(&d.value, &self.vars, 0);
            apply_decl(&mut a, &d.prop, &value, base_font);
        }
        a
    }

    /// Whether the sheet declares any `::before`/`::after` rules at all — a cheap
    /// gate so pages without generated content skip the per-element pseudo query.
    pub fn has_pseudo(&self) -> bool {
        !self.pseudo_rules.is_empty()
    }

    /// Resolve the `::before`/`::after` style for `el`. Only `pseudo_rules` are
    /// considered (matched against the element by their non-pseudo compound), and
    /// only those carrying the requested `which`. Returns `None` unless a `content`
    /// is produced — the caller emits a generated box only when content exists.
    pub fn pseudo(
        &self,
        el: &ElemCtx,
        ancestors: &[ElemCtx],
        which: PseudoEl,
        base_font: f32,
    ) -> Option<Applied> {
        ROOT_FONT.with(|c| c.set(self.root_font));
        let mut matched: Vec<&Rule> = self
            .pseudo_rules
            .iter()
            .map(|&i| &self.rules[i])
            .filter(|r| r.selector.pseudo_el == Some(which))
            .filter(|r| selector_matches(&r.selector, el, ancestors))
            .collect();
        if matched.is_empty() {
            return None;
        }
        matched.sort_by_key(|r| (r.selector.specificity, r.order));

        let mut a = Applied::default();
        for r in matched {
            for d in &r.decls {
                if d.prop.starts_with("--") {
                    continue;
                }
                let value = resolve_vars(&d.value, &self.vars, 0);
                apply_decl(&mut a, &d.prop, &value, base_font);
            }
        }
        // `content: none`/`normal` (⇒ `None` here) suppresses the box.
        a.content.as_ref()?;
        Some(a)
    }
}

/// Parse a sequence of stylesheets (in cascade order, lowest first) into one
/// rule set. Source order is preserved across sheets.
pub fn parse_all(sheets: &[String]) -> Stylesheet {
    let mut rules: Vec<Rule> = Vec::new();
    let mut order: u32 = 0;
    for sheet in sheets {
        let stripped = strip_comments(sheet);
        let chars: Vec<char> = stripped.chars().collect();
        let mut i = 0;
        parse_block(&chars, &mut i, false, &mut rules, &mut order);
    }

    // Bucket each rule by its subject's most selective discriminator (id ≻ class
    // ≻ tag ≻ universal) so `applied` can prune candidates per element.
    let mut by_id: std::collections::BTreeMap<String, Vec<usize>> = Default::default();
    let mut by_class: std::collections::BTreeMap<String, Vec<usize>> = Default::default();
    let mut by_tag: std::collections::BTreeMap<String, Vec<usize>> = Default::default();
    let mut universal: Vec<usize> = Vec::new();
    let mut pseudo_rules: Vec<usize> = Vec::new();
    for (idx, rule) in rules.iter().enumerate() {
        // `::before`/`::after` rules never apply to the element itself.
        if rule.selector.pseudo_el.is_some() {
            pseudo_rules.push(idx);
            continue;
        }
        match rule.selector.parts.last() {
            Some(subj) if subj.id.is_some() => {
                by_id.entry(subj.id.clone().unwrap()).or_default().push(idx);
            }
            Some(subj) if !subj.classes.is_empty() => {
                by_class.entry(subj.classes[0].clone()).or_default().push(idx);
            }
            Some(subj) if subj.tag.is_some() => {
                by_tag.entry(subj.tag.clone().unwrap()).or_default().push(idx);
            }
            // A bare universal subject (`*`) matches anything.
            _ => universal.push(idx),
        }
    }

    // Collect root-scope custom properties (in document order, later wins) and
    // the root font size. A rule counts as "root scope" when its subject is
    // `:root`, `html`, or the bare universal `*` — this captures the design-token
    // `:root{…}` blocks every modern site ships without leaking component-scoped
    // overrides. (Dark-theme tokens live under `@media prefers-color-scheme:dark`,
    // which `media_matches` already drops, so light values win here.)
    let mut vars: std::collections::HashMap<String, String> = Default::default();
    let mut root_font: f32 = 16.0;
    for rule in &rules {
        if !is_root_scope(&rule.selector) {
            continue;
        }
        for d in &rule.decls {
            if let Some(name) = d.prop.strip_prefix("--") {
                vars.insert(name.to_string(), d.value.clone());
            } else if d.prop == "font-size" {
                // Resolve against the previous root value so `62.5%` → 10px etc.
                if let Some(px) = root_font_size(&d.value, root_font) {
                    root_font = px;
                }
            }
        }
    }

    Stylesheet { rules, by_id, by_class, by_tag, universal, pseudo_rules, vars, root_font }
}

/// True when a selector targets the document root (`:root`, `html`, or a bare
/// `*`) as a single simple selector — where global custom properties live.
fn is_root_scope(sel: &Selector) -> bool {
    if sel.parts.len() != 1 {
        return false;
    }
    let s = &sel.parts[0];
    if !s.classes.is_empty() || s.id.is_some() || !s.attrs.is_empty() {
        return false;
    }
    let is_html = s.tag.as_deref() == Some("html");
    let is_root_pseudo = s.pseudos.iter().any(|p| matches!(p, Pseudo::Root));
    let is_universal = s.tag.is_none() && s.pseudos.is_empty();
    !s.never && (is_html || is_root_pseudo || is_universal)
}

/// Resolve a root `font-size` value to px. `%`/`em` are relative to the current
/// root size (16 initially); `rem` too (root-relative). Returns `None` if
/// unparseable.
fn root_font_size(value: &str, current_root: f32) -> Option<f32> {
    let v = value.trim().to_ascii_lowercase();
    if let Some(n) = v.strip_suffix('%') {
        return n.trim().parse::<f32>().ok().map(|p| current_root * p / 100.0);
    }
    if let Some(n) = v.strip_suffix("rem").or_else(|| v.strip_suffix("em")) {
        return n.trim().parse::<f32>().ok().map(|e| current_root * e);
    }
    if let Some(n) = v.strip_suffix("px") {
        return n.trim().parse::<f32>().ok();
    }
    if let Some(n) = v.strip_suffix("pt") {
        return n.trim().parse::<f32>().ok().map(|p| p * 1.3333);
    }
    match v.as_str() {
        "small" => Some(13.0),
        "medium" => Some(16.0),
        "large" => Some(18.0),
        "x-large" => Some(24.0),
        _ => v.parse::<f32>().ok(),
    }
}

/// Resolve every `var(--name[, fallback])` in a value against `vars`, recursively
/// (custom properties frequently reference other custom properties). Missing
/// variables fall back to the comma fallback, or empty. `depth` guards cycles.
fn resolve_vars(value: &str, vars: &std::collections::HashMap<String, String>, depth: u8) -> String {
    if depth > 24 || !value.contains("var(") {
        return value.to_string();
    }
    let chars: Vec<char> = value.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i..].starts_with(&['v', 'a', 'r', '(']) {
            // Find the matching close paren for this `var(`.
            let open = i + 3; // index of '('
            let mut depth_p = 0;
            let mut j = open;
            let mut end = None;
            while j < chars.len() {
                match chars[j] {
                    '(' => depth_p += 1,
                    ')' => {
                        depth_p -= 1;
                        if depth_p == 0 {
                            end = Some(j);
                            break;
                        }
                    }
                    _ => {}
                }
                j += 1;
            }
            let Some(end) = end else {
                out.extend(&chars[i..]);
                break;
            };
            let inner: String = chars[open + 1..end].iter().collect();
            // Split into name and (optional) fallback at the top-level comma.
            let (name, fallback) = split_var_args(&inner);
            let key = name.trim().trim_start_matches("--").to_ascii_lowercase();
            let replacement = if let Some(raw) = vars.get(&key) {
                resolve_vars(raw, vars, depth + 1)
            } else if let Some(fb) = fallback {
                resolve_vars(fb.trim(), vars, depth + 1)
            } else {
                String::new()
            };
            out.push_str(&replacement);
            i = end + 1;
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

/// Split a `var()` body into `(name, Some(fallback))` at the first top-level
/// comma (fallbacks may themselves contain commas / nested `var()`).
fn split_var_args(inner: &str) -> (&str, Option<&str>) {
    let chars: Vec<char> = inner.chars().collect();
    let mut depth = 0;
    for (idx, c) in chars.iter().enumerate() {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            ',' if depth == 0 => return (&inner[..byte_index(inner, idx)], Some(&inner[byte_index(inner, idx) + 1..])),
            _ => {}
        }
    }
    (inner, None)
}

/// Byte offset of the `n`-th char in `s` (for slicing on a char index).
fn byte_index(s: &str, char_idx: usize) -> usize {
    s.char_indices().nth(char_idx).map(|(b, _)| b).unwrap_or(s.len())
}

// ── Matching ─────────────────────────────────────────────────────────────────

fn simple_matches(s: &Simple, e: &ElemCtx) -> bool {
    if s.never {
        return false;
    }
    if let Some(t) = &s.tag {
        if !t.eq_ignore_ascii_case(&e.tag) {
            return false;
        }
    }
    if let Some(id) = &s.id {
        if e.id.as_deref() != Some(id.as_str()) {
            return false;
        }
    }
    for c in &s.classes {
        if !e.classes.iter().any(|x| x == c) {
            return false;
        }
    }
    for a in &s.attrs {
        if !attr_matches(a, e) {
            return false;
        }
    }
    for p in &s.pseudos {
        if !pseudo_matches(p, e) {
            return false;
        }
    }
    true
}

fn attr_matches(a: &AttrSel, e: &ElemCtx) -> bool {
    let Some((_, val)) = e.attrs.iter().find(|(k, _)| k.eq_ignore_ascii_case(&a.name)) else {
        return false;
    };
    match a.op {
        AttrOp::Exists => true,
        AttrOp::Eq => val == &a.value,
        AttrOp::Includes => val.split_whitespace().any(|w| w == a.value),
        AttrOp::Dash => val == &a.value || val.starts_with(&format!("{}-", a.value)),
        AttrOp::Prefix => !a.value.is_empty() && val.starts_with(&a.value),
        AttrOp::Suffix => !a.value.is_empty() && val.ends_with(&a.value),
        AttrOp::Substr => !a.value.is_empty() && val.contains(&a.value),
    }
}

/// Evaluate `an + b`: is the 1-based `idx` in the series (n = 0, 1, 2, …)?
fn nth_matches(a: i32, b: i32, idx: usize) -> bool {
    let i = idx as i32;
    if a == 0 {
        return i == b;
    }
    // i = a*n + b ⇒ n = (i - b) / a must be a non-negative integer.
    let diff = i - b;
    diff % a == 0 && diff / a >= 0
}

fn pseudo_matches(p: &Pseudo, e: &ElemCtx) -> bool {
    match p {
        Pseudo::Root => e.is_root,
        // Structural pseudos need known sibling positions (`count == 0` ⇒ unknown).
        Pseudo::FirstChild => e.count != 0 && e.index == 1,
        Pseudo::LastChild => e.count != 0 && e.index == e.count,
        Pseudo::OnlyChild => e.count == 1,
        Pseudo::Empty => false, // we only build this ctx for non-empty elements
        Pseudo::NthChild(a, b) => e.count != 0 && nth_matches(*a, *b, e.index),
        Pseudo::NthLastChild(a, b) => e.count != 0 && nth_matches(*a, *b, e.count - e.index + 1),
        Pseudo::FirstOfType => e.type_count != 0 && e.type_index == 1,
        Pseudo::LastOfType => e.type_count != 0 && e.type_index == e.type_count,
        Pseudo::OnlyOfType => e.type_count == 1,
        Pseudo::NthOfType(a, b) => e.type_count != 0 && nth_matches(*a, *b, e.type_index),
        Pseudo::Not(inner) => !simple_matches(inner, e),
    }
}

fn selector_matches(sel: &Selector, e: &ElemCtx, ancestors: &[ElemCtx]) -> bool {
    let parts = &sel.parts;
    let Some(subject) = parts.last() else {
        return false;
    };
    if !simple_matches(subject, e) {
        return false;
    }
    // Walk the remaining compounds right→left against the ancestor chain
    // (nearest ancestor first). A `Descendant` combinator may skip ancestors; a
    // `Child` combinator must bind to the immediately-enclosing one.
    let mut need = parts.len() as isize - 2;
    let mut anc_iter = ancestors.iter().rev();
    while need >= 0 {
        let combo = sel.combos[need as usize];
        let part = &parts[need as usize];
        let mut bound = false;
        for anc in anc_iter.by_ref() {
            if simple_matches(part, anc) {
                bound = true;
                break;
            }
            if combo == Combinator::Child {
                // Direct parent didn't match and child combinators can't skip.
                return false;
            }
        }
        if !bound {
            return false;
        }
        need -= 1;
    }
    true
}

// ── Selector parsing ─────────────────────────────────────────────────────────

/// A token in a complex selector: a compound (`div.foo`) or a combinator.
enum SelTok {
    Compound(String),
    Combinator(Combinator),
}

/// Split a complex selector into compounds + combinators, respecting `[...]` and
/// `:not(...)` nesting (whitespace/combinators inside them are not separators).
fn tokenize_selector(prelude: &str) -> Vec<SelTok> {
    let mut toks = Vec::new();
    let mut buf = String::new();
    let mut depth = 0i32; // `[` and `(` depth
    let flush = |buf: &mut String, toks: &mut Vec<SelTok>| {
        if !buf.trim().is_empty() {
            toks.push(SelTok::Compound(std::mem::take(buf)));
        } else {
            buf.clear();
        }
    };
    for c in prelude.chars() {
        match c {
            '[' | '(' => {
                depth += 1;
                buf.push(c);
            }
            ']' | ')' => {
                depth = (depth - 1).max(0);
                buf.push(c);
            }
            _ if depth > 0 => buf.push(c),
            '>' => {
                flush(&mut buf, &mut toks);
                toks.push(SelTok::Combinator(Combinator::Child));
            }
            '+' | '~' => {
                // Sibling combinators — approximated as descendant (no sibling
                // context in the ancestor walk).
                flush(&mut buf, &mut toks);
                toks.push(SelTok::Combinator(Combinator::Descendant));
            }
            c if c.is_whitespace() => flush(&mut buf, &mut toks),
            _ => buf.push(c),
        }
    }
    flush(&mut buf, &mut toks);
    toks
}

fn parse_selector(prelude: &str) -> Option<Selector> {
    let mut parts: Vec<Simple> = Vec::new();
    let mut combos: Vec<Combinator> = Vec::new();
    let mut specificity = 0u32;
    let mut pending = Combinator::Descendant;
    for tok in tokenize_selector(prelude) {
        match tok {
            // An explicit combinator overrides the whitespace-implied descendant.
            SelTok::Combinator(c) => pending = c,
            SelTok::Compound(s) => {
                let Some(simple) = parse_compound(&s) else {
                    pending = Combinator::Descendant;
                    continue;
                };
                specificity += simple.id.is_some() as u32 * 100
                    + (simple.classes.len() + simple.attrs.len() + simple.pseudos.len()) as u32 * 10
                    + simple.tag.is_some() as u32;
                if !parts.is_empty() {
                    combos.push(pending);
                }
                parts.push(simple);
                pending = Combinator::Descendant;
            }
        }
    }
    if parts.is_empty() {
        return None;
    }
    let pseudo_el = parts.last().and_then(|p| p.pseudo_el);
    Some(Selector { parts, combos, specificity, pseudo_el })
}

/// Parse one compound (`div.foo#bar[a=b]:nth-child(2n+1):not(.x)`) into a
/// [`Simple`]. Returns `None` only when the compound is empty.
fn parse_compound(tok: &str) -> Option<Simple> {
    let chars: Vec<char> = tok.trim().chars().collect();
    if chars.is_empty() {
        return None;
    }

    let mut tag = None;
    let mut id = None;
    let mut classes = Vec::new();
    let mut attrs = Vec::new();
    let mut pseudos = Vec::new();
    let mut never = false;
    let mut universal = false;
    let mut pseudo_el = None;
    let mut i = 0;

    // Leading type / universal.
    if chars[0] != '.' && chars[0] != '#' && chars[0] != '[' && chars[0] != ':' {
        let start = i;
        while i < chars.len() && !matches!(chars[i], '.' | '#' | '[' | ':') {
            i += 1;
        }
        let t: String = chars[start..i].iter().collect();
        if t == "*" {
            universal = true;
        } else if !t.is_empty() {
            tag = Some(t.to_ascii_lowercase());
        }
    }

    while i < chars.len() {
        match chars[i] {
            '.' | '#' => {
                let marker = chars[i];
                i += 1;
                let start = i;
                while i < chars.len() && !matches!(chars[i], '.' | '#' | '[' | ':') {
                    i += 1;
                }
                let name: String = chars[start..i].iter().collect();
                if name.is_empty() {
                    continue;
                }
                match marker {
                    '#' => id = Some(name),
                    '.' => classes.push(name),
                    _ => {}
                }
            }
            '[' => {
                let start = i + 1;
                while i < chars.len() && chars[i] != ']' {
                    i += 1;
                }
                let inner: String = chars[start..i].iter().collect();
                if i < chars.len() {
                    i += 1; // consume ']'
                }
                if let Some(a) = parse_attr_sel(&inner) {
                    attrs.push(a);
                } else {
                    never = true; // unparseable attribute selector never matches
                }
            }
            ':' => {
                // `::` ⇒ pseudo-element (never matches statically).
                let mut colons = 0;
                while i < chars.len() && chars[i] == ':' {
                    colons += 1;
                    i += 1;
                }
                let start = i;
                // Read the pseudo name up to the next selector marker or `(`.
                while i < chars.len() && !matches!(chars[i], '.' | '#' | '[' | ':' | '(') {
                    i += 1;
                }
                let mut name: String = chars[start..i].iter().collect();
                // A functional pseudo (`:nth-child(…)`, `:not(…)`) — consume its
                // balanced argument parens (which may themselves contain `.`/`:`).
                if i < chars.len() && chars[i] == '(' {
                    let arg_start = i;
                    let mut depth = 0i32;
                    while i < chars.len() {
                        match chars[i] {
                            '(' => depth += 1,
                            ')' => {
                                depth -= 1;
                                i += 1;
                                if depth == 0 {
                                    break;
                                }
                                continue;
                            }
                            _ => {}
                        }
                        i += 1;
                    }
                    let args: String = chars[arg_start..i].iter().collect();
                    name.push_str(&args);
                }
                if colons >= 2 {
                    // Pseudo-element. `::before`/`::after` target a generated box
                    // (handled by the pseudo query); any other never matches.
                    match name.as_str() {
                        "before" => pseudo_el = Some(PseudoEl::Before),
                        "after" => pseudo_el = Some(PseudoEl::After),
                        _ => never = true,
                    }
                    continue;
                }
                match parse_pseudo(&name) {
                    PseudoParse::Eval(p) => pseudos.push(p),
                    PseudoParse::Transparent => {}
                    PseudoParse::Never => never = true,
                }
            }
            _ => {
                i += 1;
            }
        }
    }

    if tag.is_none()
        && id.is_none()
        && classes.is_empty()
        && attrs.is_empty()
        && pseudos.is_empty()
        && !universal
        && !never
        && pseudo_el.is_none()
    {
        return None;
    }
    Some(Simple { tag, id, classes, attrs, pseudos, never, pseudo_el })
}

/// Parse the inside of `[...]` (name / `name op value`, value optionally quoted).
fn parse_attr_sel(inner: &str) -> Option<AttrSel> {
    let s = inner.trim();
    if s.is_empty() {
        return None;
    }
    // Find the operator (`~= |= ^= $= *= =`), if any.
    let ops: [(&str, AttrOp); 6] = [
        ("~=", AttrOp::Includes),
        ("|=", AttrOp::Dash),
        ("^=", AttrOp::Prefix),
        ("$=", AttrOp::Suffix),
        ("*=", AttrOp::Substr),
        ("=", AttrOp::Eq),
    ];
    for (sym, op) in ops {
        if let Some(pos) = s.find(sym) {
            let name = s[..pos].trim();
            let mut value = s[pos + sym.len()..].trim();
            // Strip a trailing case-insensitivity flag (`i`/`s`).
            if let Some(stripped) = value.strip_suffix(" i").or_else(|| value.strip_suffix(" I")) {
                value = stripped.trim();
            }
            let value = unquote(value);
            if name.is_empty() {
                return None;
            }
            return Some(AttrSel { name: name.to_ascii_lowercase(), op, value });
        }
    }
    // Bare `[name]`.
    Some(AttrSel { name: s.to_ascii_lowercase(), op: AttrOp::Exists, value: String::new() })
}

fn unquote(s: &str) -> String {
    let s = s.trim();
    let bytes = s.as_bytes();
    if bytes.len() >= 2
        && (bytes[0] == b'"' || bytes[0] == b'\'')
        && bytes[bytes.len() - 1] == bytes[0]
    {
        s[1..s.len() - 1].to_string()
    } else {
        s.to_string()
    }
}

enum PseudoParse {
    /// A structural pseudo we evaluate.
    Eval(Pseudo),
    /// Ignored (`:link`, `:any-link`, …) — the compound can still match.
    Transparent,
    /// Dynamic/unsupported — the compound never matches statically.
    Never,
}

/// Parse a single pseudo (`name` or `name(args)`), lowercased name expected.
fn parse_pseudo(raw: &str) -> PseudoParse {
    let raw = raw.trim();
    let (name, args) = match raw.split_once('(') {
        Some((n, rest)) => (n.trim().to_ascii_lowercase(), rest.trim_end_matches(')').trim()),
        None => (raw.to_ascii_lowercase(), ""),
    };
    match name.as_str() {
        "first-child" => PseudoParse::Eval(Pseudo::FirstChild),
        "last-child" => PseudoParse::Eval(Pseudo::LastChild),
        "only-child" => PseudoParse::Eval(Pseudo::OnlyChild),
        "empty" => PseudoParse::Eval(Pseudo::Empty),
        "root" => PseudoParse::Eval(Pseudo::Root),
        "first-of-type" => PseudoParse::Eval(Pseudo::FirstOfType),
        "last-of-type" => PseudoParse::Eval(Pseudo::LastOfType),
        "only-of-type" => PseudoParse::Eval(Pseudo::OnlyOfType),
        "nth-child" => parse_nth(args).map_or(PseudoParse::Never, |(a, b)| {
            PseudoParse::Eval(Pseudo::NthChild(a, b))
        }),
        "nth-last-child" => parse_nth(args).map_or(PseudoParse::Never, |(a, b)| {
            PseudoParse::Eval(Pseudo::NthLastChild(a, b))
        }),
        "nth-of-type" => parse_nth(args).map_or(PseudoParse::Never, |(a, b)| {
            PseudoParse::Eval(Pseudo::NthOfType(a, b))
        }),
        "not" => match parse_compound(args) {
            Some(inner) => PseudoParse::Eval(Pseudo::Not(Box::new(inner))),
            None => PseudoParse::Never,
        },
        // Link-ish pseudos: treat as transparent so link styling still applies.
        "link" | "any-link" | "local-link" => PseudoParse::Transparent,
        // Everything else (hover/focus/active/visited/target/checked/… and
        // unknown) is a dynamic state we can't have statically.
        _ => PseudoParse::Never,
    }
}

/// Parse an `An+B` micro-syntax (`odd`, `even`, `3`, `2n`, `2n+1`, `-n+3`).
fn parse_nth(s: &str) -> Option<(i32, i32)> {
    let s: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    let s = s.to_ascii_lowercase();
    match s.as_str() {
        "odd" => return Some((2, 1)),
        "even" => return Some((2, 0)),
        "" => return None,
        _ => {}
    }
    if let Some(npos) = s.find('n') {
        let a_str = &s[..npos];
        let a = match a_str {
            "" | "+" => 1,
            "-" => -1,
            _ => a_str.parse::<i32>().ok()?,
        };
        let b_str = &s[npos + 1..];
        let b = if b_str.is_empty() { 0 } else { b_str.parse::<i32>().ok()? };
        Some((a, b))
    } else {
        // Plain integer ⇒ a == 0.
        Some((0, s.parse::<i32>().ok()?))
    }
}

// ── Stylesheet tokenizing ────────────────────────────────────────────────────

fn strip_comments(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(chars.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '/' && i + 1 < chars.len() && chars[i + 1] == '*' {
            i += 2;
            while i + 1 < chars.len() && !(chars[i] == '*' && chars[i + 1] == '/') {
                i += 1;
            }
            i += 2;
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

/// Skip a balanced `{ … }` block starting at `i` (`chars[i] == '{'`). Returns the
/// index just past the matching `}`.
/// Nominal viewport width (px) used to evaluate width media queries. The render
/// surface is ~800px wide, so mobile breakpoints (`max-width:480/768`) don't
/// match and the desktop layout is kept.
const MEDIA_VIEWPORT_W: f32 = 800.0;

/// Decide whether an `@media` prelude matches our fixed environment: a
/// screen (not print), light colour scheme, 1× density, ~800px-wide viewport.
/// Comma-separated queries are OR'd; `and`-joined conditions within a query are
/// AND'd. Unknown features are treated as matching so we never drop desktop
/// rules we simply don't model.
fn media_matches(prelude: &str) -> bool {
    let cond = prelude
        .trim()
        .trim_start_matches("@media")
        .trim()
        .to_ascii_lowercase();
    if cond.is_empty() {
        return true;
    }
    cond.split(',').any(|q| media_query_matches(q.trim()))
}

fn media_query_matches(q: &str) -> bool {
    let negate = q.starts_with("not ");
    let q = q
        .trim_start_matches("not ")
        .trim_start_matches("only ")
        .trim();
    let result = q.split(" and ").all(|c| media_cond_matches(c.trim()));
    if negate {
        !result
    } else {
        result
    }
}

fn media_cond_matches(c: &str) -> bool {
    if c.is_empty() {
        return true;
    }
    if !c.starts_with('(') {
        // A media *type* keyword (screen/print/all/speech).
        return !matches!(c, "print" | "speech");
    }
    let inner = c.trim_start_matches('(').trim_end_matches(')');
    let mut parts = inner.splitn(2, ':');
    let feat = parts.next().unwrap_or("").trim();
    let val = parts.next().unwrap_or("").trim();
    match feat {
        "prefers-color-scheme" => val != "dark",
        "prefers-reduced-motion" | "prefers-reduced-data" => val.is_empty() || val == "no-preference",
        "max-width" | "max-device-width" => media_len_px(val).is_none_or(|n| MEDIA_VIEWPORT_W <= n),
        "min-width" | "min-device-width" => media_len_px(val).is_none_or(|n| MEDIA_VIEWPORT_W >= n),
        "orientation" => val != "portrait",
        // Hi-DPI queries pull in 2× sprite backgrounds; stay at 1×.
        f if f.contains("device-pixel-ratio") || f.contains("resolution") || f.contains("dpi") => {
            false
        }
        _ => true, // width-agnostic / unmodelled feature → keep the rules
    }
}

fn media_len_px(v: &str) -> Option<f32> {
    let v = v.trim();
    if let Some(n) = v.strip_suffix("px") {
        return n.trim().parse().ok();
    }
    v.parse().ok()
}

fn skip_block(chars: &[char], mut i: usize) -> usize {
    let mut depth = 0i32;
    while i < chars.len() {
        match chars[i] {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                i += 1;
                if depth == 0 {
                    return i;
                }
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    i
}

fn parse_block(
    chars: &[char],
    i: &mut usize,
    stop_at_brace: bool,
    rules: &mut Vec<Rule>,
    order: &mut u32,
) {
    while *i < chars.len() {
        while *i < chars.len() && chars[*i].is_whitespace() {
            *i += 1;
        }
        if *i >= chars.len() {
            break;
        }
        let c = chars[*i];
        if c == '}' {
            *i += 1;
            if stop_at_brace {
                return;
            }
            continue;
        }
        if c == '@' {
            let ks = *i;
            while *i < chars.len()
                && !chars[*i].is_whitespace()
                && chars[*i] != '{'
                && chars[*i] != ';'
            {
                *i += 1;
            }
            let kw: String = chars[ks..*i].iter().collect();
            let kwl = kw.to_ascii_lowercase();
            if kwl.starts_with("@media") || kwl.starts_with("@supports") {
                // Read the condition prelude up to the block's `{`.
                let cs = *i;
                while *i < chars.len() && chars[*i] != '{' && chars[*i] != ';' {
                    *i += 1;
                }
                let cond: String = chars[cs..*i].iter().collect();
                // `@supports` (feature queries) — assume supported and flatten.
                // `@media` — only flatten when the condition matches our viewport
                // (screen, light scheme, nominal width); otherwise skip the block
                // so mobile breakpoints / print / dark-scheme rules don't clobber
                // the desktop light rendering.
                let matches = kwl.starts_with("@supports") || media_matches(&cond);
                if *i < chars.len() && chars[*i] == '{' {
                    *i += 1;
                    if matches {
                        parse_block(chars, i, true, rules, order);
                    } else {
                        // Skip to the matching close brace of this at-rule block.
                        *i = skip_block(chars, *i - 1);
                    }
                } else if *i < chars.len() && chars[*i] == ';' {
                    *i += 1;
                }
            } else {
                // Skip the whole at-rule (statement or block).
                while *i < chars.len() && chars[*i] != ';' && chars[*i] != '{' {
                    *i += 1;
                }
                if *i < chars.len() && chars[*i] == '{' {
                    *i = skip_block(chars, *i);
                } else if *i < chars.len() && chars[*i] == ';' {
                    *i += 1;
                }
            }
            continue;
        }

        // An ordinary rule: prelude up to '{', then a declaration block to '}'.
        let ps = *i;
        while *i < chars.len() && chars[*i] != '{' && chars[*i] != '}' {
            *i += 1;
        }
        if *i >= chars.len() || chars[*i] == '}' {
            if *i < chars.len() {
                *i += 1;
            }
            continue;
        }
        let prelude: String = chars[ps..*i].iter().collect();
        *i += 1; // consume '{'
        let bs = *i;
        while *i < chars.len() && chars[*i] != '}' {
            *i += 1;
        }
        let body: String = chars[bs..*i].iter().collect();
        if *i < chars.len() {
            *i += 1; // consume '}'
        }

        let decls = parse_decls(&body);
        if decls.is_empty() {
            *order += 1;
            continue;
        }
        for sel_str in prelude.split(',') {
            if let Some(sel) = parse_selector(sel_str) {
                rules.push(Rule {
                    selector: sel,
                    decls: decls.clone(),
                    order: *order,
                });
            }
        }
        *order += 1;
    }
}

fn parse_decls(body: &str) -> Vec<Decl> {
    let mut out = Vec::new();
    for chunk in body.split(';') {
        let mut kv = chunk.splitn(2, ':');
        let prop = kv.next().unwrap_or("").trim().to_ascii_lowercase();
        let value = match kv.next() {
            Some(v) => v.replace("!important", "").trim().to_string(),
            None => continue,
        };
        if prop.is_empty() || value.is_empty() {
            continue;
        }
        out.push(Decl { prop, value });
    }
    out
}

// ── Declaration application ──────────────────────────────────────────────────

fn apply_decl(a: &mut Applied, prop: &str, value: &str, base_font: f32) {
    match prop {
        "color" => {
            if let Some(c) = parse_color(value) {
                a.color = Some(c);
            }
        }
        "background-color" | "background" => {
            // `background` shorthand can carry the colour anywhere (`url(...) #fff`),
            // or a gradient — pull out a representative colour.
            if let Some(c) = background_color(value) {
                a.background = Some(c);
            }
            if let Some(u) = plain_background_url(value) {
                a.background_image = Some(u);
            } else if let Some(u) = sprite_atlas_url(value) {
                a.sprite_atlas = Some(u);
            }
        }
        "background-image" => {
            if let Some(u) = plain_background_url(value) {
                a.background_image = Some(u);
            } else if let Some(u) = sprite_atlas_url(value) {
                a.sprite_atlas = Some(u);
            }
        }
        "background-position" => {
            let (x, y) = parse_background_position(value, base_font);
            if x.is_some() {
                a.background_position[0] = x;
            }
            if y.is_some() {
                a.background_position[1] = y;
            }
        }
        "background-position-x" => {
            if let Some(v) = parse_len_px(value.trim(), base_font).or_else(|| match value.trim() {
                "left" => Some(0.0),
                _ => None,
            }) {
                a.background_position[0] = Some(v);
            }
        }
        "background-position-y" => {
            if let Some(v) = parse_len_px(value.trim(), base_font).or_else(|| match value.trim() {
                "top" => Some(0.0),
                _ => None,
            }) {
                a.background_position[1] = Some(v);
            }
        }
        "background-size" => {
            if !value.trim().is_empty() {
                a.background_size_set = true;
            }
        }
        "font-size" => {
            // Ignore non-positive sizes (e.g. `font-size:0`, used to collapse
            // inline-block whitespace) — they'd zero out real text otherwise.
            if let Some(s) = parse_font_size(value, base_font) {
                if s > 0.0 {
                    a.font_size = Some(s);
                }
            }
        }
        "line-height" => {
            if let Some(lh) = parse_line_height(value, base_font) {
                a.line_height = Some(lh);
            }
        }
        "font-weight" => {
            let v = value.trim().to_ascii_lowercase();
            let bold = match v.as_str() {
                "bold" | "bolder" => Some(true),
                "normal" | "lighter" => Some(false),
                _ => v.parse::<u32>().ok().map(|n| n >= 600),
            };
            if let Some(b) = bold {
                a.font_weight = Some(b);
            }
        }
        "font-style" => {
            match value.trim().to_ascii_lowercase().as_str() {
                "italic" | "oblique" => a.font_style = Some(true),
                "normal" => a.font_style = Some(false),
                _ => {}
            }
        }
        "text-align" => {
            if let Some(al) = parse_align(value) {
                a.text_align = Some(al);
            }
        }
        "display" => {
            if let Some(d) = parse_display(value) {
                a.display = Some(d);
            }
        }
        "margin" => {
            apply_box_shorthand(&mut a.margin, value, base_font);
            a.margin_auto = margin_auto_shorthand(value);
        }
        "margin-top" => {
            set_side(&mut a.margin, 0, value, base_font);
            a.margin_auto[0] = value.trim().eq_ignore_ascii_case("auto");
        }
        "margin-right" => {
            set_side(&mut a.margin, 1, value, base_font);
            a.margin_auto[1] = value.trim().eq_ignore_ascii_case("auto");
        }
        "margin-bottom" => {
            set_side(&mut a.margin, 2, value, base_font);
            a.margin_auto[2] = value.trim().eq_ignore_ascii_case("auto");
        }
        "margin-left" => {
            set_side(&mut a.margin, 3, value, base_font);
            a.margin_auto[3] = value.trim().eq_ignore_ascii_case("auto");
        }
        "padding" => apply_box_shorthand(&mut a.padding, value, base_font),
        "padding-top" => set_side(&mut a.padding, 0, value, base_font),
        "padding-right" => set_side(&mut a.padding, 1, value, base_font),
        "padding-bottom" => set_side(&mut a.padding, 2, value, base_font),
        "padding-left" => set_side(&mut a.padding, 3, value, base_font),
        "width" => {
            if let Some(w) = parse_width(value, base_font) {
                a.width = Some(w);
            }
        }
        "height" => {
            if let Some(h) = parse_width(value, base_font) {
                a.height = Some(h);
            }
        }
        "max-height" => {
            let v = value.trim().to_ascii_lowercase();
            // `none` clears any cap; a bare `0` is the collapsed-accordion case.
            if v == "none" {
                a.max_height = None;
            } else if v == "0" {
                a.max_height = Some(0.0);
            } else if let Some(px) = parse_len_px(&v, base_font) {
                a.max_height = Some(px);
            }
        }
        "max-width" => {
            if value.trim().eq_ignore_ascii_case("none") {
                a.max_width = None;
            } else if let Some(w) = parse_width(value, base_font) {
                a.max_width = Some(w);
            }
        }
        "border" => apply_border_shorthand(a, value, base_font),
        "border-width" => {
            let first = value.split_whitespace().next().unwrap_or(value);
            if let Some(px) = parse_len_px(first, base_font) {
                a.border_width = Some(px);
            }
        }
        "border-color" => {
            let first = value.split_whitespace().next().unwrap_or(value);
            if let Some(c) = parse_color(first) {
                a.border_color = Some(c);
            }
        }
        "border-style" => {
            let s = value.trim().to_ascii_lowercase();
            if s == "none" || s == "hidden" {
                a.border_width = Some(0.0);
            }
        }
        "grid-template-columns" => {
            if let Some(cols) = parse_grid_columns(value, base_font) {
                a.grid_columns = Some(cols);
            }
        }
        "content" => {
            a.content = parse_content(value);
        }
        "flex-direction" => {
            if let Some(d) = parse_flex_direction(value) {
                a.flex_direction = Some(d);
            }
        }
        "justify-content" => {
            if let Some(j) = parse_justify(value) {
                a.justify = Some(j);
            }
        }
        "align-items" => {
            if let Some(al) = parse_align_items(value) {
                a.align = Some(al);
            }
        }
        // `gap` shorthand is `row-gap column-gap`; we model a single spacing, so
        // take the first length for any of the three properties.
        "gap" | "row-gap" | "column-gap" | "grid-gap" => {
            let first = value.split_whitespace().next().unwrap_or(value);
            if let Some(px) = parse_len_px(first, base_font) {
                a.gap = Some(px.max(0.0));
            }
        }
        "float" => {
            if let Some(f) = parse_float(value) {
                a.float = Some(f);
            }
        }
        "clear" => {
            let v = value.trim().to_ascii_lowercase();
            a.clear = Some(matches!(v.as_str(), "left" | "right" | "both"));
        }
        "position" => {
            if let Some(p) = parse_position(value) {
                a.position = Some(p);
            }
        }
        "top" => set_inset_side(a, 0, value, base_font),
        "right" => set_inset_side(a, 1, value, base_font),
        "bottom" => set_inset_side(a, 2, value, base_font),
        "left" => set_inset_side(a, 3, value, base_font),
        _ => {}
    }
}

/// Parse a CSS `content` value into the literal string to render, or `None` when
/// it produces no box (`none`/`normal`) or is a construct we don't support
/// (`attr()`, `counter()`, images, …). Handles one or more space-separated quoted
/// strings (concatenated) and CSS `\NN` hex escapes inside them — covers the
/// separators/bullets/icons real sites put in generated content.
fn parse_content(value: &str) -> Option<String> {
    let v = value.trim();
    if v.is_empty() || v.eq_ignore_ascii_case("none") || v.eq_ignore_ascii_case("normal") {
        return None;
    }
    let chars: Vec<char> = v.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    let mut saw_string = false;
    while i < chars.len() {
        let c = chars[i];
        if c == '"' || c == '\'' {
            saw_string = true;
            let quote = c;
            i += 1;
            while i < chars.len() && chars[i] != quote {
                if chars[i] == '\\' {
                    // CSS escape: `\` followed by up to 6 hex digits (optional
                    // trailing space), else the literal next char.
                    i += 1;
                    let start = i;
                    while i < chars.len() && i - start < 6 && chars[i].is_ascii_hexdigit() {
                        i += 1;
                    }
                    if i > start {
                        let code = u32::from_str_radix(
                            &chars[start..i].iter().collect::<String>(),
                            16,
                        )
                        .ok();
                        if let Some(ch) = code.and_then(char::from_u32) {
                            out.push(ch);
                        }
                        if i < chars.len() && chars[i] == ' ' {
                            i += 1; // one optional whitespace terminates the escape
                        }
                    } else if i < chars.len() {
                        out.push(chars[i]);
                        i += 1;
                    }
                } else {
                    out.push(chars[i]);
                    i += 1;
                }
            }
            i += 1; // closing quote
        } else {
            // Skip whitespace/other tokens between strings; a non-string token
            // (e.g. `attr(...)`) means we can't fully render it — bail out.
            if !c.is_whitespace() {
                return None;
            }
            i += 1;
        }
    }
    if saw_string {
        Some(out)
    } else {
        None
    }
}

/// Set one side of a `[top, right, bottom, left]` box from a single length.
fn set_side(sides: &mut [Option<f32>; 4], idx: usize, value: &str, base: f32) {
    if let Some(px) = parse_len_px(value, base) {
        sides[idx] = Some(px);
    }
}

/// Set one `position` inset side, routing a `%` value to `inset_pct` (as a
/// fraction) and any other length to `inset` (px). A side set to one clears the
/// other so a later percentage overrides an earlier px offset and vice versa.
fn set_inset_side(a: &mut Applied, idx: usize, value: &str, base: f32) {
    let v = value.trim();
    if let Some(pct) = v.strip_suffix('%') {
        if let Ok(n) = pct.trim().parse::<f32>() {
            a.inset_pct[idx] = Some(n / 100.0);
            a.inset[idx] = None;
        }
    } else if let Some(px) = parse_len_px(v, base) {
        a.inset[idx] = Some(px);
        a.inset_pct[idx] = None;
    }
}

/// Expand a `margin`/`padding` shorthand (1–4 lengths) into the four sides.
fn apply_box_shorthand(sides: &mut [Option<f32>; 4], value: &str, base: f32) {
    let toks: Vec<f32> = value
        .split_whitespace()
        .map(|t| parse_len_px(t, base).unwrap_or(0.0))
        .collect();
    let (t, r, b, l) = match toks.len() {
        1 => (toks[0], toks[0], toks[0], toks[0]),
        2 => (toks[0], toks[1], toks[0], toks[1]),
        3 => (toks[0], toks[1], toks[2], toks[1]),
        n if n >= 4 => (toks[0], toks[1], toks[2], toks[3]),
        _ => return,
    };
    *sides = [Some(t), Some(r), Some(b), Some(l)];
}

/// Which sides of a `margin` shorthand were `auto`, in `[t, r, b, l]` order
/// (same 1–4 value expansion as [`apply_box_shorthand`]).
fn margin_auto_shorthand(value: &str) -> [bool; 4] {
    let a: Vec<bool> = value
        .split_whitespace()
        .map(|t| t.eq_ignore_ascii_case("auto"))
        .collect();
    match a.len() {
        1 => [a[0], a[0], a[0], a[0]],
        2 => [a[0], a[1], a[0], a[1]],
        3 => [a[0], a[1], a[2], a[1]],
        n if n >= 4 => [a[0], a[1], a[2], a[3]],
        _ => [false; 4],
    }
}

/// Parse a `border` shorthand: any of `<width> <style> <color>` in any order.
fn apply_border_shorthand(a: &mut Applied, value: &str, base: f32) {
    let mut width: Option<f32> = None;
    let mut color: Option<Rgba> = None;
    let mut has_style = false;
    let mut none = false;
    for tok in value.split_whitespace() {
        let tl = tok.to_ascii_lowercase();
        if tl == "none" || tl == "hidden" {
            none = true;
        } else if is_border_style(&tl) {
            has_style = true;
        } else if let Some(px) = parse_len_px(tok, base) {
            width = Some(px);
        } else if let Some(c) = parse_color(tok) {
            color = Some(c);
        }
    }
    if none {
        a.border_width = Some(0.0);
        return;
    }
    if let Some(w) = width {
        a.border_width = Some(w);
    } else if has_style || color.is_some() {
        // A border was requested without an explicit width → CSS `medium`.
        a.border_width = Some(2.0);
    }
    if let Some(c) = color {
        a.border_color = Some(c);
    }
}

fn is_border_style(s: &str) -> bool {
    matches!(
        s,
        "solid" | "dashed" | "dotted" | "double" | "groove" | "ridge" | "inset" | "outset"
    )
}

/// Parse a CSS length to absolute px. Returns `None` for `auto` or units we
/// can't resolve without a containing block (`%`, viewport units).
fn parse_len_px(value: &str, base: f32) -> Option<f32> {
    let v = value.trim().to_ascii_lowercase();
    if v == "0" {
        return Some(0.0);
    }
    if v == "auto" {
        return None;
    }
    if let Some(inner) = v.strip_prefix("calc(").and_then(|r| r.strip_suffix(')')) {
        return eval_calc(inner, base);
    }
    let num = |s: &str| s.trim().parse::<f32>().ok();
    if let Some(n) = v.strip_suffix("px") {
        return num(n);
    }
    if let Some(n) = v.strip_suffix("rem") {
        return num(n).map(|x| x * root_font_px()); // root-relative, not element
    }
    if let Some(n) = v.strip_suffix("em") {
        return num(n).map(|x| x * base);
    }
    if let Some(n) = v.strip_suffix("pt") {
        return num(n).map(|x| x * 1.3333);
    }
    if v.ends_with('%') {
        return None; // needs the containing block — unresolved here
    }
    num(&v) // bare number → treat as px
}

/// Parse a CSS `width`: `auto`, a percentage, or an absolute length.
fn parse_width(value: &str, base: f32) -> Option<WidthVal> {
    let v = value.trim().to_ascii_lowercase();
    if v == "auto" {
        return Some(WidthVal::Auto);
    }
    if let Some(n) = v.strip_suffix('%') {
        return n.trim().parse::<f32>().ok().map(|x| WidthVal::Pct(x / 100.0));
    }
    // `calc()` with only absolute terms resolves to px; a `%` inside makes it
    // unresolvable here (needs the containing block) → treat as auto.
    parse_len_px(&v, base).map(WidthVal::Px)
}

/// Evaluate a `calc(...)` body to absolute px. Returns `None` if it references a
/// containing-block-relative unit (`%`, viewport units) we can't resolve here.
/// Supports `+ - * /`, parentheses, and px/em/rem/pt/bare-number terms.
fn eval_calc(expr: &str, base: f32) -> Option<f32> {
    let toks = calc_tokenize(expr, base)?;
    let mut pos = 0;
    let v = calc_expr(&toks, &mut pos)?;
    if pos == toks.len() { Some(v) } else { None }
}

/// A `calc` token: a resolved numeric value, an operator, or a paren.
#[derive(Clone, Copy)]
enum CalcTok {
    Num(f32),
    Op(char),
    Open,
    Close,
}

fn calc_tokenize(expr: &str, base: f32) -> Option<Vec<CalcTok>> {
    let chars: Vec<char> = expr.chars().collect();
    let mut toks = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
        } else if c == '(' {
            toks.push(CalcTok::Open);
            i += 1;
        } else if c == ')' {
            toks.push(CalcTok::Close);
            i += 1;
        } else if matches!(c, '+' | '-' | '*' | '/') {
            toks.push(CalcTok::Op(c));
            i += 1;
        } else {
            // A length/number term: consume up to the next operator/paren/space.
            let start = i;
            while i < chars.len() && !matches!(chars[i], '+' | '-' | '*' | '/' | '(' | ')' | ' ') {
                i += 1;
            }
            let term: String = chars[start..i].iter().collect();
            if term.trim_end_matches(|c: char| c.is_ascii_alphabetic()).ends_with('%') {
                return None; // `%` term — unresolvable here
            }
            let v = if let Ok(n) = term.parse::<f32>() {
                n
            } else {
                parse_len_px(&term, base)?
            };
            toks.push(CalcTok::Num(v));
        }
    }
    Some(toks)
}

// Recursive-descent: expr := term (('+'|'-') term)* ; term := factor (('*'|'/') factor)*.
fn calc_expr(toks: &[CalcTok], pos: &mut usize) -> Option<f32> {
    let mut v = calc_term(toks, pos)?;
    while let Some(CalcTok::Op(op @ ('+' | '-'))) = toks.get(*pos).copied() {
        *pos += 1;
        let rhs = calc_term(toks, pos)?;
        v = if op == '+' { v + rhs } else { v - rhs };
    }
    Some(v)
}

fn calc_term(toks: &[CalcTok], pos: &mut usize) -> Option<f32> {
    let mut v = calc_factor(toks, pos)?;
    while let Some(CalcTok::Op(op @ ('*' | '/'))) = toks.get(*pos).copied() {
        *pos += 1;
        let rhs = calc_factor(toks, pos)?;
        v = if op == '*' { v * rhs } else { v / rhs };
    }
    Some(v)
}

fn calc_factor(toks: &[CalcTok], pos: &mut usize) -> Option<f32> {
    match toks.get(*pos).copied()? {
        CalcTok::Num(n) => {
            *pos += 1;
            Some(n)
        }
        CalcTok::Open => {
            *pos += 1;
            let v = calc_expr(toks, pos)?;
            matches!(toks.get(*pos), Some(CalcTok::Close)).then(|| {
                *pos += 1;
                v
            })
        }
        // A leading unary minus (`calc(-5px + …)`).
        CalcTok::Op('-') => {
            *pos += 1;
            Some(-calc_factor(toks, pos)?)
        }
        _ => None,
    }
}

/// Extract a representative background colour from a `background`(-color) value:
/// a `linear-/radial-gradient(...)`'s first colour stop, or the first token that
/// parses as a colour (`url(...) no-repeat #fff`). `None` ⇒ no colour (stays
/// transparent, e.g. an image-only background).
/// Extract a standalone `url(...)` from a `background` / `background-image` value.
/// Returns `None` when the value contains any `gradient(` — the sprite-atlas
/// loading trick pairs `linear-gradient(transparent,transparent)` with the sheet
/// URL, and painting a whole atlas as one icon is wrong — so we only surface a
/// genuine single-image background. `url("…")`/`url('…')`/`url(…)` all handled;
/// `data:`/`none` are rejected.
fn plain_background_url(value: &str) -> Option<String> {
    if value.to_ascii_lowercase().contains("gradient(") {
        return None;
    }
    let start = value.find("url(")? + "url(".len();
    let rest = &value[start..];
    let end = rest.find(')')?;
    let raw = rest[..end].trim().trim_matches(|c| c == '"' || c == '\'').trim();
    if raw.is_empty() || raw.starts_with("data:") || raw.eq_ignore_ascii_case("none") {
        return None;
    }
    Some(raw.to_string())
}

/// Extract the sprite-sheet `url(...)` from a CSS-sprite background value: one that
/// pairs a gradient with a URL (`linear-gradient(transparent,transparent),url(s.svg)`
/// — the trick used to force a background repaint). Only returns a URL when a
/// `gradient(` IS present (that's what distinguishes an atlas from a standalone
/// image, which [`plain_background_url`] handles). `data:` sheets are rejected.
fn sprite_atlas_url(value: &str) -> Option<String> {
    if !value.to_ascii_lowercase().contains("gradient(") {
        return None;
    }
    let start = value.find("url(")? + "url(".len();
    let rest = &value[start..];
    let end = rest.find(')')?;
    let raw = rest[..end].trim().trim_matches(|c| c == '"' || c == '\'').trim();
    if raw.is_empty() || raw.starts_with("data:") || raw.eq_ignore_ascii_case("none") {
        return None;
    }
    Some(raw.to_string())
}

/// Parse `background-position` into `(x, y)` CSS-px. Handles the common shapes for
/// a sprite cell: two lengths (`-47px -47px`), a single length (`y` defaults to
/// centre → we leave it unset), and the `left`/`top` keywords (⇒ 0). Percentages
/// and `right`/`bottom`/`center` need the box/image size, so they stay unset.
fn parse_background_position(value: &str, base: f32) -> (Option<f32>, Option<f32>) {
    let toks: Vec<&str> = value.split_whitespace().collect();
    let axis = |t: &str, horiz: bool| -> Option<f32> {
        match t {
            "left" if horiz => Some(0.0),
            "top" if !horiz => Some(0.0),
            _ => parse_len_px(t, base),
        }
    };
    match toks.as_slice() {
        [x] => (axis(x, true), None),
        [x, y, ..] => (axis(x, true), axis(y, false)),
        _ => (None, None),
    }
}

fn background_color(value: &str) -> Option<Rgba> {
    let lower = value.to_ascii_lowercase();
    if let Some(gpos) = lower.find("gradient(") {
        // Take the substring inside the outermost gradient parens.
        let after = &value[gpos + "gradient(".len()..];
        let mut depth = 1i32;
        let mut end = after.len();
        for (i, c) in after.char_indices() {
            match c {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        end = i;
                        break;
                    }
                }
                _ => {}
            }
        }
        let inner = &after[..end];
        // Comma-split at depth 0; the first arg that parses as a colour is the
        // first stop (skips a leading direction like `to right` / `45deg`).
        for part in split_top_commas(inner) {
            let tok = part.trim().split_whitespace().next().unwrap_or("");
            if let Some(c) = parse_color(tok) {
                return Some(c);
            }
            // Also try the whole part (for `rgb(1, 2, 3)` spanning commas).
            if let Some(c) = parse_color(part.trim()) {
                return Some(c);
            }
        }
        return None;
    }
    // Non-gradient: try the value as a whole, then each whitespace token.
    if let Some(c) = parse_color(value.trim()) {
        return Some(c);
    }
    value.split_whitespace().find_map(parse_color)
}

/// Split on top-level commas (not inside parens) — for gradient/rgb argument lists.
fn split_top_commas(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut buf = String::new();
    let mut depth = 0i32;
    for c in s.chars() {
        match c {
            '(' => {
                depth += 1;
                buf.push(c);
            }
            ')' => {
                depth -= 1;
                buf.push(c);
            }
            ',' if depth == 0 => out.push(std::mem::take(&mut buf)),
            _ => buf.push(c),
        }
    }
    if !buf.trim().is_empty() {
        out.push(buf);
    }
    out
}

/// Parse a CSS colour: `rgb()/rgba()/hsl()/hsla()` here, otherwise hex / named
/// via [`style`].
pub fn parse_color(s: &str) -> Option<Rgba> {
    let s = s.trim();
    let lower = s.to_ascii_lowercase();
    if lower == "transparent" {
        return Some((0.0, 0.0, 0.0, 0.0));
    }
    if let Some(inner) = lower
        .strip_prefix("rgba(")
        .or_else(|| lower.strip_prefix("rgb("))
    {
        let inner = inner.trim_end_matches(')');
        let nums: Vec<f32> = inner
            .split(|c| c == ',' || c == '/' || c == ' ')
            .filter(|p| !p.trim().is_empty())
            .filter_map(|p| p.trim().trim_end_matches('%').parse::<f32>().ok())
            .collect();
        if nums.len() >= 3 {
            let a = nums.get(3).copied().unwrap_or(1.0);
            return Some((nums[0] / 255.0, nums[1] / 255.0, nums[2] / 255.0, a));
        }
        return None;
    }
    if let Some(inner) = lower
        .strip_prefix("hsla(")
        .or_else(|| lower.strip_prefix("hsl("))
    {
        let inner = inner.trim_end_matches(')');
        let parts: Vec<&str> = inner
            .split(|c| c == ',' || c == '/' || c == ' ')
            .filter(|p| !p.trim().is_empty())
            .collect();
        if parts.len() >= 3 {
            let h = parts[0].trim().trim_end_matches("deg").parse::<f32>().ok()?;
            let sat = parts[1].trim().trim_end_matches('%').parse::<f32>().ok()? / 100.0;
            let l = parts[2].trim().trim_end_matches('%').parse::<f32>().ok()? / 100.0;
            let a = parts
                .get(3)
                .and_then(|p| p.trim().trim_end_matches('%').parse::<f32>().ok())
                .unwrap_or(1.0);
            let (r, g, b) = hsl_to_rgb(h, sat, l);
            return Some((r, g, b, a));
        }
        return None;
    }
    crate::style::parse_color(s)
}

/// HSL (h in degrees, s/l in 0..1) → RGB in 0..1.
fn hsl_to_rgb(h: f32, s: f32, l: f32) -> (f32, f32, f32) {
    let h = ((h % 360.0) + 360.0) % 360.0;
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - (((h / 60.0) % 2.0) - 1.0).abs());
    let m = l - c / 2.0;
    let (r, g, b) = match h as i32 {
        0..=59 => (c, x, 0.0),
        60..=119 => (x, c, 0.0),
        120..=179 => (0.0, c, x),
        180..=239 => (0.0, x, c),
        240..=299 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    (r + m, g + m, b + m)
}

fn parse_font_size(value: &str, base: f32) -> Option<f32> {
    let v = value.trim().to_ascii_lowercase();
    let parse_num = |s: &str| s.trim().parse::<f32>().ok();
    if let Some(n) = v.strip_suffix("px") {
        return parse_num(n);
    }
    if let Some(n) = v.strip_suffix("pt") {
        return parse_num(n).map(|x| x * 1.3333);
    }
    if let Some(n) = v.strip_suffix("rem") {
        return parse_num(n).map(|x| x * root_font_px()); // root-relative
    }
    if let Some(n) = v.strip_suffix("em") {
        return parse_num(n).map(|x| x * base);
    }
    if let Some(n) = v.strip_suffix('%') {
        return parse_num(n).map(|x| x / 100.0 * base);
    }
    match v.as_str() {
        "xx-small" => Some(base * 0.6),
        "x-small" => Some(base * 0.75),
        "small" => Some(base * 0.875),
        "medium" => Some(base),
        "large" => Some(base * 1.25),
        "x-large" => Some(base * 1.5),
        "xx-large" => Some(base * 2.0),
        "smaller" => Some(base * 0.85),
        "larger" => Some(base * 1.2),
        _ => parse_num(&v), // bare number → treat as px
    }
}

/// Resolve `line-height` to px against the element's font size `base`. A bare
/// number is a multiplier of the font size (CSS unitless line-height); `normal`
/// is left for the engine's natural metrics (returns `None`); lengths/percentages
/// resolve like a font size.
fn parse_line_height(value: &str, base: f32) -> Option<LineHeight> {
    let v = value.trim().to_ascii_lowercase();
    if v == "normal" {
        return None;
    }
    // Unitless ⇒ carry the multiplier so it re-resolves against each element's
    // own font-size (CSS inherited-multiplier semantics), rather than freezing
    // it to px here against `body`'s size and inheriting a too-small line box.
    if let Ok(mult) = v.parse::<f32>() {
        return (mult > 0.0).then_some(LineHeight::Scale(mult));
    }
    parse_font_size(&v, base)
        .filter(|px| *px > 0.0)
        .map(LineHeight::Px)
}

fn parse_align(value: &str) -> Option<TextAlign> {
    match value.trim().to_ascii_lowercase().as_str() {
        "left" | "start" => Some(TextAlign::Left),
        "center" => Some(TextAlign::Center),
        "right" | "end" => Some(TextAlign::Right),
        // No real justification (monospaced 8x8 font) — fall back to left.
        "justify" => Some(TextAlign::Left),
        _ => None,
    }
}

fn parse_display(value: &str) -> Option<Display> {
    match value.trim().to_ascii_lowercase().as_str() {
        "none" => Some(Display::None),
        "inline" => Some(Display::Inline),
        "inline-block" => Some(Display::InlineBlock),
        "block" => Some(Display::Block),
        "flex" | "inline-flex" => Some(Display::Flex),
        "grid" | "inline-grid" => Some(Display::Grid),
        // table/etc. aren't laid out yet — render as block so content still shows.
        _ => Some(Display::Block),
    }
}

/// Parse `grid-template-columns` into per-column weights summing to 1 (each column's
/// share of the container width). We don't do true track sizing — every track is
/// reduced to a single weight and the columns are stretched to fill the container:
/// `fr` uses its factor; lengths/`%` use their resolved size as the weight;
/// `auto`/`min-content`/`max-content` count as one unit. `repeat(N, …)` expands, and
/// `repeat(auto-fill|auto-fit, minmax(min, …))` estimates a column count from a
/// nominal width. Returns `None` for `none`/unparseable templates (→ single column).
fn parse_grid_columns(value: &str, base_font: f32) -> Option<Vec<f32>> {
    let v = value.trim().to_ascii_lowercase();
    if v.is_empty() || v == "none" || v == "auto" || v == "initial" || v == "inherit" {
        return None;
    }
    let mut weights: Vec<f32> = Vec::new();
    for tok in split_track_list(&v) {
        expand_track(&tok, base_font, &mut weights);
    }
    if weights.is_empty() {
        return None;
    }
    let sum: f32 = weights.iter().sum();
    if sum <= 0.0 {
        let n = weights.len();
        return Some(vec![1.0 / n as f32; n]);
    }
    Some(weights.iter().map(|w| w / sum).collect())
}

/// Split a track list on whitespace, but keep `repeat(…)`/`minmax(…)` (which contain
/// spaces and commas) as single tokens by tracking parenthesis depth.
fn split_track_list(v: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut cur = String::new();
    for c in v.chars() {
        match c {
            '(' => {
                depth += 1;
                cur.push(c);
            }
            ')' => {
                depth -= 1;
                cur.push(c);
            }
            c if c.is_whitespace() && depth == 0 => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            c => cur.push(c),
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Nominal container width used only to estimate a column count for
/// `repeat(auto-fill|auto-fit, minmax(min, …))`. Proportions are what matter.
const GRID_NOMINAL_W: f32 = 880.0;

/// Expand one track token into one or more column weights appended to `out`.
fn expand_track(tok: &str, base_font: f32, out: &mut Vec<f32>) {
    if let Some(inner) = tok.strip_prefix("repeat(").and_then(|s| s.strip_suffix(')')) {
        // repeat(count, tracks…)
        let (count_str, tracks) = match inner.split_once(',') {
            Some((c, t)) => (c.trim(), t.trim()),
            None => return,
        };
        let track_weights: Vec<f32> = {
            let mut w = Vec::new();
            for t in split_track_list(tracks) {
                expand_track(&t, base_font, &mut w);
            }
            if w.is_empty() {
                w.push(1.0);
            }
            w
        };
        let count = if count_str == "auto-fill" || count_str == "auto-fit" {
            // Estimate how many columns fit: nominal width / the pattern's min width.
            let min_each = track_min_px(tracks, base_font).max(1.0);
            ((GRID_NOMINAL_W / min_each).floor() as usize).max(1)
        } else {
            count_str.parse::<usize>().unwrap_or(1).max(1)
        };
        for _ in 0..count {
            out.extend_from_slice(&track_weights);
        }
        return;
    }
    out.push(track_weight(tok, base_font));
}

/// A single (non-`repeat`) track's weight. `fr` → factor; length/`%` → resolved px
/// (so wider tracks get more space); `auto`/content keywords → 1; `minmax(a,b)` →
/// its max side's weight.
fn track_weight(tok: &str, base_font: f32) -> f32 {
    if let Some(inner) = tok.strip_prefix("minmax(").and_then(|s| s.strip_suffix(')')) {
        let max = inner.split_once(',').map(|(_, b)| b.trim()).unwrap_or(inner);
        return track_weight(max, base_font);
    }
    if let Some(fr) = tok.strip_suffix("fr") {
        return fr.trim().parse::<f32>().unwrap_or(1.0).max(0.0);
    }
    if tok == "auto" || tok == "min-content" || tok == "max-content" {
        return 1.0;
    }
    if let Some(pct) = tok.strip_suffix('%') {
        return pct.trim().parse::<f32>().map(|p| p / 100.0).unwrap_or(1.0);
    }
    parse_len_px(tok, base_font)
        .map(|px| (px / GRID_NOMINAL_W).max(0.02))
        .unwrap_or(1.0)
}

/// Minimum px width of a track pattern, for `auto-fill`/`auto-fit` count estimation.
fn track_min_px(tracks: &str, base_font: f32) -> f32 {
    let first = split_track_list(tracks).into_iter().next().unwrap_or_default();
    if let Some(inner) = first.strip_prefix("minmax(").and_then(|s| s.strip_suffix(')')) {
        let min = inner.split_once(',').map(|(a, _)| a.trim()).unwrap_or(inner);
        return parse_len_px(min, base_font).unwrap_or(GRID_NOMINAL_W);
    }
    parse_len_px(&first, base_font).unwrap_or(GRID_NOMINAL_W)
}

fn parse_flex_direction(value: &str) -> Option<FlexDirection> {
    match value.trim().to_ascii_lowercase().as_str() {
        // `*-reverse` isn't modelled; fall back to the forward axis.
        "row" | "row-reverse" => Some(FlexDirection::Row),
        "column" | "column-reverse" => Some(FlexDirection::Column),
        _ => None,
    }
}

fn parse_justify(value: &str) -> Option<Justify> {
    match value.trim().to_ascii_lowercase().as_str() {
        "flex-start" | "start" | "left" | "normal" => Some(Justify::Start),
        "center" => Some(Justify::Center),
        "flex-end" | "end" | "right" => Some(Justify::End),
        "space-between" => Some(Justify::SpaceBetween),
        "space-around" => Some(Justify::SpaceAround),
        "space-evenly" => Some(Justify::SpaceEvenly),
        _ => None,
    }
}

fn parse_align_items(value: &str) -> Option<Align> {
    match value.trim().to_ascii_lowercase().as_str() {
        "flex-start" | "start" => Some(Align::Start),
        "center" => Some(Align::Center),
        "flex-end" | "end" => Some(Align::End),
        "stretch" | "normal" => Some(Align::Stretch),
        _ => None,
    }
}

fn parse_float(value: &str) -> Option<Float> {
    match value.trim().to_ascii_lowercase().as_str() {
        "left" => Some(Float::Left),
        "right" => Some(Float::Right),
        "none" => Some(Float::None),
        _ => None,
    }
}

fn parse_position(value: &str) -> Option<Position> {
    match value.trim().to_ascii_lowercase().as_str() {
        "static" => Some(Position::Static),
        // `sticky` behaves like `relative` until its scroll threshold; approximate.
        "relative" | "sticky" => Some(Position::Relative),
        // `fixed` anchors to the viewport; we anchor to the nearest positioned
        // ancestor (or the surface), which matches for top-level overlays.
        "absolute" | "fixed" => Some(Position::Absolute),
        _ => None,
    }
}
