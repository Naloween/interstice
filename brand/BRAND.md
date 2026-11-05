# Interstice — Brand and Art Direction

This is the reference for how anything Interstice looks: the mark, colours,
type, shapes, motion, and how they apply to each example. The source image is
`brand/board.png`. When a screen and this document disagree, fix the screen
or change this document first.

> **interstice** — *a space for things to interact.*
> Modular / open / extensible / forward-looking.

---

## 1. The idea

Interstice is made of modules that meet only through the space between them:
tables, subscriptions, reducer calls, nodes. The brand is that space: a dark,
quiet field with a single thin slit of warm light. Everything solid stays calm
and low-contrast. Light appears only where something is happening.

**The rule of light:** amber means *alive*: focused, running, selected, yours.
A screen should have very few lit things, so the eye goes to them first. Never
use amber for decoration, large fills or backgrounds.

**Restraint over ornament.** No gradients on UI surfaces, no drop-shadow
stacks, no saturated colour. Hierarchy comes from spacing, size, tracking and
the three greys.

---

## 2. The mark: the slit

The mark is the **I** of interstice drawn as a slit of light: a vertical
sliver, pointed at both ends and widest at its middle, with a warm ivory core
and an amber glow around it. It reads as light escaping through a gap, and
it stands in for the first letter of the wordmark.

| File | Use |
|---|---|
| `brand/mark.svg` | The slit alone, from 24 px up. |
| `brand/wordmark.svg` | The slit plus `NTERSTICE`: the logo. |
| `brand/icon.svg` | App icon: the slit on a rounded charcoal tile. |
| `brand/board.png` | The brand board (reference image). |

**Construction** (height `H` of the slit):

| Part | Geometry |
|---|---|
| Core | a hairline lens, `H` tall and `0.018 H` wide at the middle (at least 1.6 px), tapering to 0 at the ends; `core` colour fading to `amber` at its tips |
| Glow | `amber` around the lens, Gaussian with σ ≈ `0.022 H` (at least 1.6 px), at 60 %, strongest at the middle |
| Bloom | a vertical ellipse `0.3 H` × `0.9 H`, `amber` at 10 % → 0, centred |

The glow is always the core's own colour spread out: never a ring or an outline.
In the engine, the mark is rasterized once into a texture (`interstice_ui::theme::slit_rgba`)
and drawn as an image, because the 2D renderer has no blur.

**Wordmark:** the slit followed by `NTERSTICE` in Space Grotesk Regular,
uppercase, tracking **+0.32 em**, in `ivory`. The slit's height is 2.1× the
cap height (its tips fade, so it reads about 1.5× tall), its centre on the
letters' centre line, and the gap before the N is 0.62× the cap height. The letters are never lit; the slit carries the light.

**Tagline:** `A SPACE FOR THINGS TO INTERACT`, Space Grotesk Regular,
uppercase, tracking +0.22 em, in `gray`, about 0.42× the wordmark's size.

**Rules:**
- Clear space: one cap height on every side.
- On light backgrounds (`paper`), the letters become `charcoal` and the slit's
  core turns `amber` with the glow at half strength. Never invert the slit to dark.
- Don't recolour, rotate, outline or stretch the slit, and don't set the wordmark
  in another face or without its tracking.
- Animated use: only the glow may breathe (opacity 1 → 0.65 → 1 over 4 s), and
  only while something is starting or loading.

---

## 3. Colour

### Brand palette (from the board)

| Token | Hex | Role |
|---|---|---|
| `ivory` | `#E8E9E6` | Text, the wordmark |
| `charcoal` | `#181B1F` | Windows, panels: the main surface |
| `amber` | `#C59B60` | The light: focus, selection, primary action, "you" |
| `gray` | `#747B82` | Captions, labels, inactive icons |

### Interface tokens (derived)

| Token | Hex | Use |
|---|---|---|
| `ink` | `#0F1012` | Wells: inputs, terminals, the game arena, the browser's page well |
| `ground` | `#151618` | Desktop wallpaper, page background behind panels |
| `charcoal` | `#181B1F` | Window bodies, panels, the top bar |
| `raised` | `#212429` | Title bars, tiles, buttons, menus, list rows on hover |
| `line` | `#2C2F35` | Hairlines, borders and dividers (1 px) |
| `ivory` | `#E8E9E6` | Primary text; never pure white |
| `muted` | `#8A9097` | Secondary text that must be read at small sizes |
| `gray` | `#747B82` | Captions, uppercase labels, placeholders (13 px and up) |
| `dim` | `#4A4F56` | Disabled glyphs, inactive dots; never text that must be read |
| `amber` | `#C59B60` | See §1 |
| `amber-hi` | `#DDB97F` | Hover on amber things |
| `core` | `#F6E9D2` | The hot centre of the slit and of other glows |
| `success` | `#7FAF8C` | Done, connected |
| `danger` | `#D0796B` | Errors, destructive actions |
| `info` | `#7E9FBF` | Neutral notices, links inside content |
| `paper` | `#EEECE6` | The light-mode background (docs, print) |

There is deliberately no warning colour: a yellow would compete with amber.
A warning is `ivory` text with a `danger` dot.

**Tints** (a token at a fixed alpha over its background):
- `amber` at 10 %: selected row; at 35 %: focused window border; at 18 %: the top bar's hairline.
- `ivory` at 4 %: hovered row; at 8 %: window border; at 12 %: input border.
- Scrim behind overlays: `ink` at 72 %.

**Contrast** (WCAG, measured):

| Pair | Ratio |
|---|---|
| `ivory` on `ink` / `charcoal` / `raised` | 15.6 / 14.2 / 12.8 |
| `muted` on `charcoal` / `raised` | 5.4 / 4.8 |
| `gray` on `charcoal` | 4.0: labels 13 px+ and uppercase captions only |
| `amber` on `charcoal` | 6.8 |
| `ink` on `amber` (primary button label) | 7.5 |
| `success` / `danger` / `info` on `charcoal` | 6.9 / 5.5 / 6.3 |

### Categorical colours (players, series, avatars)

When things must be told apart (agar players, chart series), use these quiet
hues in this order. Never use `amber`, which is reserved for "you" or the selection.

`#7E9FBF` steel · `#B9789A` mauve · `#7FAF8C` sage · `#9A8CC4` lavender ·
`#6FB0A8` teal · `#C9826B` clay

---

## 4. Type

| Role | Family | Weights |
|---|---|---|
| UI, titles, the wordmark | **Space Grotesk** | Regular (400), Medium (500) |
| Numbers that change, ids, clocks, code, logs, terminals | **IBM Plex Mono** | Regular, Medium |
| Glyph fallback (Greek, Cyrillic, symbols; CJK) | DejaVu Sans; Droid Sans Fallback | |

All are SIL Open Font License or free licences and are embedded in the engine
(`crates/interstice-ui/assets`, `modules/defaults/graphics/assets`). There are
no italics: emphasis is Medium, never slanted.

**Scale** (px): 11 caption · 12 small · 13 UI body · 15 reading body ·
17 window title · 22 heading · 34 display.

**Uppercase labels** (section headers like `APPS`, `LEADERBOARD`, panel
captions) are 11 px Space Grotesk, tracking +0.18 em, in `gray`. They are how
the board labels its sections. Use them instead of bold headings.

**Rules:**
- Sentence case for everything that isn't an uppercase label or the wordmark.
- Any number that changes (scores, clocks, ids, counts) is mono, so it doesn't jitter.
- At most one weight change per element.

---

## 5. Shape, space and depth

- **Spacing:** a 4 px grid, using 4 · 8 · 12 · 16 · 24 · 32 · 48.
- **Radii:** 4 for chips · 6 for buttons and inputs · 10 for windows and menus ·
  16 for app tiles and the app icon · full for pills and dots.
- **Borders:** 1 px hairlines (`line`, or `ivory` at 8 %). Borders separate
  things; there are almost no shadows.
- **Elevation:** a window gets one soft shadow (black at 30 %, offset 0/10,
  grown 6 px, four steps of decreasing alpha). A focused window keeps the same
  shadow and swaps its hairline to `amber` at 35 %.
- **Hairline layout:** like the board, sections may be separated by 1 px
  `line` rules with an uppercase label on them. Prefer that to boxes in boxes.

---

## 6. Motion

| What | Duration | Easing |
|---|---|---|
| Hover, press | 120 ms | linear |
| Window open, close, focus change | 180 ms | ease-out |
| Toast in / out | 220 ms in, 180 ms out; stays 4 s | ease-out |
| Breathing glow (starting / loading only) | 4 s loop | sine |

Things that open fade and rise 6 px. Things that close fade. Nothing bounces.
Motion means a state change, so idle screens are still.

---

## 7. Components

- **Top bar** (desktop): 36 px, `charcoal`, bottom hairline in `amber` at 18 %.
  - Left: the wordmark at 13 px. Then an `APPS` label button, lit amber while
    the launcher is open.
  - Right: running-service dots, then the clock in mono `muted`.
- **Window:**
  - Body: `charcoal`; title bar 34 px in `charcoal`, separated from the body by a `line` hairline.
  - Title: 13 px `ivory`, with the app's subtitle after it in `gray`.
  - Close: a `×` at 18 px in `gray`, turning `danger` (with a faint danger tint behind it) on hover.
  - Focus: a 1 px `amber` slit at the left end of the title, plus the amber
    hairline. The title bar is never recoloured.
- **Buttons:**
  - Primary: `amber` fill, `ink` label, Medium.
  - Secondary: `raised` fill, `ivory` label.
  - Quiet: no fill, `muted` label that turns `ivory` on hover.
  - Destructive: `danger` label on `raised`.
  - One primary per view.
- **Inputs:** `ink` fill, 1 px `ivory` at 12 %, `gray` placeholder. Focused:
  the border is `amber` at 60 %.
- **Lists:**
  - Rows are 30 px; hovered: `ivory` at 4 %.
  - Selected: `amber` at 10 % plus a 2 px `amber` bar on the left.
- **Chips** (commits, tags): mono 11, 4 px radius, `raised` with a `line` border.
  The current one has an `amber` border and `amber` text.
- **Status dots:** 6 px circles. `amber` = running or yours, `success` = done,
  `danger` = failed, `dim` = stopped.
- **Toasts:** a pill on `raised` with a `line` hairline, a status dot and one
  line of 13 px text. The full message goes to the log.
- **App tiles** (launcher):
  - Shape: 16 px radius, `raised`, 1 px `line`.
  - Content: a monochrome glyph in `ivory` on `charcoal`, the name in 15 px, a
    one-line blurb in `gray`. A running app shows an `amber` dot.
  - Hover: the border becomes `amber` at 35 %.

---

## 8. Applying it

| Where | What changes |
|---|---|
| **Desktop** | The wallpaper is `ground`, with the slit large and faint in the middle and the wordmark and tagline below it (the brand board's hero). Top bar, windows, launcher and toasts follow §7. While the launcher is up, the wallpaper is scrimmed and the tiles sit on a hairline grid under an `APPS` label. |
| **Agar** | The arena is `ink` with a `line` grid every 48 px and a hairline border at the world edge. Your cell is `amber` with a `core` rim and a soft glow; other players use the categorical colours; pellets are `gray`. The HUD uses uppercase labels and mono numbers; the leaderboard is a hairline panel in which your row is lit. |
| **Notes** | §7 components. The history is a column of mono chips, and the head is lit. |
| **UI demo** | Shows the components themselves, in the tokens. |
| **Browser** | The chrome (tabs, toolbar, address field, menus) is dark `charcoal` / `raised` with hairlines. The active tab has a 2 px `amber` bar on top. The new-tab page is the wordmark plus the address field on `ground`. Web page content is never themed. |
| **Other examples** (graphics, audio, hello) | Use the tokens for every colour; no example picks its own grey. |
| **README and docs** | `wordmark.svg` at the top. |

**Implementation:** the tokens live once, in `interstice_ui::theme`: colours as
RGBA tuples, the spacing and radius scales, the uppercase-label helper, and the
procedural brand textures (the slit, the bloom). `interstice-ui`, graphics and
every example use it. A value missing from the theme is added there, never
hard-coded in a module. The browser chrome is HTML/CSS, so it repeats the
tokens as CSS variables in `chrome.rs`, with a comment pointing here.
