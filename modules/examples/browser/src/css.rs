//! A small but real CSS engine: tokenize stylesheets into rules, match selectors
//! (type / `.class` / `#id` / `*` / descendant / grouping) with specificity, and
//! resolve the cascade for an element into a set of property overrides. This is
//! intentionally a pragmatic subset (see the roadmap): no attribute/pseudo
//! selectors (they're stripped), child/sibling combinators are approximated as
//! descendant, and only a handful of properties are surfaced. `@media`/`@supports`
//! blocks are flattened (their inner rules always apply); other at-rules are
//! skipped.

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
    pub text_align: Option<TextAlign>,
    pub display: Option<Display>,
    pub margin: [Option<f32>; 4],
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
        if o.text_align.is_some() {
            self.text_align = o.text_align;
        }
        if o.display.is_some() {
            self.display = o.display;
        }
        for k in 0..4 {
            if o.margin[k].is_some() {
                self.margin[k] = o.margin[k];
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

#[derive(Clone)]
struct Simple {
    /// `None` ⇒ universal (`*` or no type).
    tag: Option<String>,
    id: Option<String>,
    classes: Vec<String>,
}

struct Selector {
    /// Compound selectors outermost→subject (subject is last).
    parts: Vec<Simple>,
    specificity: u32,
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
}

impl Stylesheet {
    /// Resolve the author cascade for `el` (with its `ancestors`, outermost
    /// first). `base_font` resolves relative `font-size` units (`em`/`%`).
    pub fn applied(&self, el: &ElemCtx, ancestors: &[ElemCtx], base_font: f32) -> Applied {
        let mut matched: Vec<&Rule> = self
            .rules
            .iter()
            .filter(|r| selector_matches(&r.selector, el, ancestors))
            .collect();
        // Lowest priority first so later declarations overwrite earlier ones.
        matched.sort_by_key(|r| (r.selector.specificity, r.order));

        let mut a = Applied::default();
        for r in matched {
            for d in &r.decls {
                apply_decl(&mut a, &d.prop, &d.value, base_font);
            }
        }
        a
    }
}

/// Parse one inline `style="…"` attribute into overrides (highest priority).
pub fn parse_inline(style_attr: &str, base_font: f32) -> Applied {
    let mut a = Applied::default();
    for d in parse_decls(style_attr) {
        apply_decl(&mut a, &d.prop, &d.value, base_font);
    }
    a
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
    Stylesheet { rules }
}

// ── Matching ─────────────────────────────────────────────────────────────────

fn simple_matches(s: &Simple, e: &ElemCtx) -> bool {
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
    true
}

fn selector_matches(sel: &Selector, e: &ElemCtx, ancestors: &[ElemCtx]) -> bool {
    let parts = &sel.parts;
    let Some(subject) = parts.last() else {
        return false;
    };
    if !simple_matches(subject, e) {
        return false;
    }
    // The remaining compounds must match ancestors in order (a subsequence),
    // nearest ancestor bound to the rightmost remaining compound.
    let mut need = parts.len() as isize - 2;
    for anc in ancestors.iter().rev() {
        if need < 0 {
            break;
        }
        if simple_matches(&parts[need as usize], anc) {
            need -= 1;
        }
    }
    need < 0
}

// ── Selector parsing ─────────────────────────────────────────────────────────

fn parse_selector(prelude: &str) -> Option<Selector> {
    // Approximate child/sibling combinators as descendant; pseudo + attribute
    // selectors are stripped per-compound below.
    let normalized = prelude.replace('>', " ").replace('+', " ").replace('~', " ");
    let mut parts: Vec<Simple> = Vec::new();
    let mut specificity = 0u32;
    for tok in normalized.split_whitespace() {
        let Some(simple) = parse_compound(tok) else {
            continue;
        };
        specificity += simple.id.is_some() as u32 * 100
            + simple.classes.len() as u32 * 10
            + simple.tag.is_some() as u32;
        parts.push(simple);
    }
    if parts.is_empty() {
        return None;
    }
    Some(Selector { parts, specificity })
}

fn parse_compound(tok: &str) -> Option<Simple> {
    // Drop pseudo-classes/elements and attribute selectors — we only key off
    // type/id/class.
    let no_pseudo = tok.split(':').next().unwrap_or("");
    let cleaned = strip_brackets(no_pseudo);
    let chars: Vec<char> = cleaned.chars().collect();
    if chars.is_empty() {
        return None;
    }

    let mut tag = None;
    let mut id = None;
    let mut classes = Vec::new();
    let mut universal = false;
    let mut i = 0;

    if chars[0] != '.' && chars[0] != '#' {
        let start = 0;
        while i < chars.len() && chars[i] != '.' && chars[i] != '#' {
            i += 1;
        }
        let t: String = chars[start..i].iter().collect();
        if t == "*" {
            universal = true; // matches any element (specificity 0)
        } else if !t.is_empty() {
            tag = Some(t.to_ascii_lowercase());
        }
    }

    while i < chars.len() {
        let marker = chars[i];
        i += 1;
        let start = i;
        while i < chars.len() && chars[i] != '.' && chars[i] != '#' {
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

    if tag.is_none() && id.is_none() && classes.is_empty() && !universal {
        return None;
    }
    Some(Simple { tag, id, classes })
}

fn strip_brackets(s: &str) -> String {
    let mut out = String::new();
    let mut depth = 0u32;
    for c in s.chars() {
        match c {
            '[' => depth += 1,
            ']' => depth = depth.saturating_sub(1),
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out
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
                // Flatten: parse the inner rules as if at top level.
                while *i < chars.len() && chars[*i] != '{' && chars[*i] != ';' {
                    *i += 1;
                }
                if *i < chars.len() && chars[*i] == '{' {
                    *i += 1;
                    parse_block(chars, i, true, rules, order);
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
            // `background` shorthand: try the first token as a colour.
            let token = value.split_whitespace().next().unwrap_or(value);
            if let Some(c) = parse_color(token) {
                a.background = Some(c);
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
        "margin" => apply_box_shorthand(&mut a.margin, value, base_font),
        "margin-top" => set_side(&mut a.margin, 0, value, base_font),
        "margin-right" => set_side(&mut a.margin, 1, value, base_font),
        "margin-bottom" => set_side(&mut a.margin, 2, value, base_font),
        "margin-left" => set_side(&mut a.margin, 3, value, base_font),
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
        _ => {}
    }
}

/// Set one side of a `[top, right, bottom, left]` box from a single length.
fn set_side(sides: &mut [Option<f32>; 4], idx: usize, value: &str, base: f32) {
    if let Some(px) = parse_len_px(value, base) {
        sides[idx] = Some(px);
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
    let num = |s: &str| s.trim().parse::<f32>().ok();
    if let Some(n) = v.strip_suffix("px") {
        return num(n);
    }
    if let Some(n) = v.strip_suffix("rem") {
        return num(n).map(|x| x * base);
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
    parse_len_px(&v, base).map(WidthVal::Px)
}

/// Parse a CSS colour: `rgb()/rgba()` here, otherwise hex / named via [`style`].
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
            .split(|c| c == ',' || c == '/')
            .filter_map(|p| p.trim().trim_end_matches('%').parse::<f32>().ok())
            .collect();
        if nums.len() >= 3 {
            let a = nums.get(3).copied().unwrap_or(1.0);
            return Some((nums[0] / 255.0, nums[1] / 255.0, nums[2] / 255.0, a));
        }
        return None;
    }
    crate::style::parse_color(s)
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
        return parse_num(n).map(|x| x * base);
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
        // grid/table/etc. aren't laid out yet — render as block so content
        // still shows.
        _ => Some(Display::Block),
    }
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
