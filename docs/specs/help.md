# Module spec: `rvision::help` + `rvision::widgets::help_pane` + `edit` help viewer

- **Status:** Draft
- **Phase:** 10 (polish — help system)
- **Related ADRs:** 0014 (doc process), 0015 (canvas/coords), 0017 (modal
  dialogs / focus-in-draw), 0018 (editor's bespoke driver loop), 0021/0022
  (the clipboard convention the content documents), and a new ADR for the help
  **format + model** (see *Open questions*). The decision to ship a *topic-list*
  viewer now and leave *full hypertext* + a *non-modal desktop help window* as
  later work is recorded in the roadmap.

## Purpose

A simplified, navigable help system: a **content format** (hand-authored now,
tool-emittable later), a **parser** into a topic model, a reusable **page
renderer** (`HelpPane`), and the **viewers** that present them. The shipped
deliverable is the *windowing-independent core* — everything except a
free-floating desktop help window.

What it is **not** (deliberately, for now):
- **Not** full TurboVision hypertext — no followable cross-links yet. The
  *format* reserves link syntax; the *renderer* shows a link as its plain label
  text. Following links is a later phase.
- **Not** a non-modal desktop window. `rvision`'s `Desktop`/`Window` can't host a
  dynamically-opened, draggable window yet; building that is its own framework
  concern (the "edit-MDI-vs-framework-windowing" question, spun out as a separate
  grilling). The default `rvision` `HelpWindow` container therefore waits; `edit`
  ships a **modal** viewer via `exec_view`, which no windowing decision can
  invalidate.
- **Not** context-sensitive yet. The open path carries an optional initial topic
  (`Option<&str>`); every caller passes `None` (home) for now. That is the seam.

## The content format

Lightweight, line-oriented, hand-authored, parsed by a simple line scanner (no
serde, no nesting/attribute soup). Borrows `<pre>` for verbatim blocks.

```
# A comment line (ignored). Blank lines separate paragraphs.

@topic keyboard  Keyboard & mouse        <- new topic: id then Title (rest of line)

A prose paragraph. Source line breaks inside a paragraph are
insignificant — the pane reflows it to the current width.

A blank line starts a new paragraph.

<pre>
Ctrl+S        Save            (verbatim: never reflowed, columns preserved)
Ctrl+Shift+V  System paste
</pre>

@topic clipboard  Clipboard
edit keeps two clipboards; see {the Keyboard topic|keyboard} for the keys.
                                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^
                  reserved link syntax {label|target}; v1 renders just "label"
```

Rules:
- A line whose first non-space char is `#` is a **comment** (dropped).
- `@topic <id> <title…>` opens a topic: the first whitespace-delimited token is
  the `id`; the trimmed remainder is the `title`. Content before the first
  `@topic` is ignored (lets a file start with a comment header).
- Between topics, **blank-line-separated** runs of text are `Paragraph`s; within
  a paragraph, newlines and runs of spaces collapse (the pane reflows).
- `<pre>` / `</pre>` on their own lines bound a `Preformatted` block: every line
  between them is kept verbatim (leading spaces, alignment, blank lines).
- `{label|target}` is an inline link; in v1 the parser keeps only `label` in the
  paragraph text (rendered plainly). `target` is a topic `id` for the future.
- Topic **order** is declaration order: that is the contents-list order, and the
  **first** topic is the home/landing topic.

## Public interface

### `rvision::help` — format, parser, model

```rust
/// A parsed help document: an ordered set of topics.
pub struct HelpContents { /* Vec<HelpTopic>, id index */ }

/// One help topic: a stable id, a display title, and a body of blocks.
pub struct HelpTopic { pub id: String, pub title: String, pub body: Vec<Block> }

/// A unit of topic body. Grows additively (Heading/List/inline links later).
pub enum Block {
    /// Reflowed prose. Links already reduced to their label text (v1).
    Paragraph(String),
    /// Verbatim lines, never reflowed (tables, keybinding columns).
    Preformatted(Vec<String>),
}

impl HelpContents {
    /// Parses the markup. Infallible: malformed input degrades gracefully
    /// (an unknown directive is treated as text; an unclosed <pre> runs to the
    /// next @topic or end). Authoring mistakes are caught by a content test,
    /// not by a Result the runtime must thread.
    pub fn parse(source: &str) -> Self;

    pub fn topics(&self) -> &[HelpTopic];
    pub fn topic(&self, id: &str) -> Option<&HelpTopic>;
    pub fn titles(&self) -> Vec<&str>;          // for the contents ListBox
    pub fn home(&self) -> Option<&HelpTopic>;   // the first topic
}
```

### `rvision::widgets::HelpPane` — the reusable page renderer

A focusable read-only view over one topic's body that scrolls in **both** axes
(prose reflows to the width, but a wide `<pre>` block can still overflow).

```rust
pub struct HelpPane { /* bounds, rendered lines, top, focused, style */ }

impl HelpPane {
    pub fn new(bounds: Rect, theme: &Theme) -> Self;
    /// Lay out `topic.body` for the current width: reflow Paragraphs (via
    /// `wrap`), emit Preformatted lines verbatim, a blank line between blocks.
    /// Resets the scroll to the top.
    pub fn show(&mut self, topic: &HelpTopic);
    pub fn content_width(&self) -> i16;   // widest laid-out line (for sizing)
    pub fn content_height(&self) -> i16;  // total lines (for sizing/scroll)
}
impl View for HelpPane { /* draw + scroll keys (↑↓←→/Pg/Home/End) + wheel + bars */ }
```

`HelpPane` re-lays-out when its `bounds` size changes (reflow follows width). It
draws a vertical `ScrollBar` in the last column when the page is too tall and a
horizontal one along the bottom when a line is too wide — each only when needed
(the conditional-scroll-bar predicate, like `ListBox`/the editor). The two bars
interact (each steals a row/column, which can call for the other), so they are
decided together by a short fixed-point in `layout`. The wheel pans vertically;
`←`/`→` and the horizontal bar pan by column; off-screen columns are clipped by
the canvas (the visible slice is drawn at a negative x-offset).

### `rvision::command`

```rust
/// Open the help viewer (TurboVision's cmHelp). A framework-standard id.
pub const CM_HELP: Command = Command(6);
```

### `edit` — the modal viewer + wiring

```rust
// crates/edit/src/help.rs
pub struct HelpViewer { /* HelpContents, ListBox contents, HelpPane, focus, size */ }
impl HelpViewer { pub fn new(contents: &'static HelpContents, initial: Option<&str>, theme: &Theme) -> Self; }
impl View for HelpViewer { /* two-pane draw + Tab + routing + mouse */ }
impl Modal for HelpViewer { /* size; ends_on(CM_OK|CM_CANCEL|CM_HELP-close) */ }

// content, baked in:
static HELP_TEXT: &str = include_str!("help.txt");
// app.rs: fn open_help(app, ed, theme, initial: Option<&str>) -> io::Result<()>
```

## Behaviour & invariants

- **Parser is total.** No input panics; malformed markup degrades (see `parse`).
- **`<pre>` is sacrosanct.** Preformatted lines are byte-for-byte what was
  authored between the fences — never wrapped, trimmed, or space-collapsed.
- **Reflow uses display columns** (`wrap`, ADR 0006/0015), so CJK/wide prose
  fits the pane exactly; a `<pre>` line wider than the pane is reached by
  **horizontal scrolling** (`←`/`→`, the bottom bar, or the wheel is vertical
  only) — it is never wrapped or silently clipped away.
- **Live update.** Moving the contents-list selection re-`show`s the page
  immediately (no Enter needed).
- **Focus model.** Two focus stops: the contents `ListBox` and the `HelpPane`.
  Focus starts on the list (home topic shown). `Tab`/`Shift-Tab` toggle; `Enter`
  on the list jumps focus to the page; `Esc` (and a Close affordance) end the
  modal.
- **Links are inert in v1** but their label text still reads naturally in prose.
- **Empty/degenerate:** empty contents → an empty list and a blank page (no
  panic); a zero-size pane draws nothing.

## Collaborators

- `wrap::wrap` for paragraph reflow; `widgets::ScrollBar`/`ScrollPart` for the
  page scroll bar and wheel/track hit-testing; `widgets::ListBox` for contents;
  `Canvas`/`Theme`/`Role` (`DialogBackground`, `Input`, `WindowFrame`) for draw;
  `view::{Modal, Context}` + `Application::exec_view` for the modal loop (ADR
  0017). `edit` bakes the content with `include_str!` and routes F1 / the Help
  menu item through its bespoke driver (ADR 0018), mirroring `FindDialog`.

## Test plan (write these first)

- **Parser (logic):** one topic; several topics in order; id vs title split;
  blank-line paragraph grouping; `<pre>` kept verbatim (incl. blank/aligned
  lines); `{label|target}` → label text; comments dropped; pre-`@topic` preamble
  ignored; unclosed `<pre>` degrades; `topic(id)`/`home()`/`titles()`.
- **HelpPane (render snapshot):** a topic with a paragraph + a `<pre>` table
  reflows the prose but keeps the table aligned; an overflowing page shows the
  scroll bar; a short page shows none.
- **HelpPane (interaction):** PgDn/PgUp/arrows/Home/End scroll and clamp; wheel
  pans; scroll-bar arrow/track clicks scroll.
- **Viewer (interaction):** arrowing the contents list live-updates the page
  (assert the page's first line changes); Tab moves focus list↔page; Esc ends;
  `initial = Some(id)` opens on that topic, `None` on home; a click in each pane
  focuses it.
- **Content (validation, compile-in safety net):** `HELP_TEXT` parses to the
  expected topics; every topic id is unique; every `{…|target}` resolves to an
  existing id; the Clipboard topic mentions both `Ctrl+V` and `Ctrl+Shift+V`.
- **Manual:** `cargo run -p edit` → F1 / Help ▸ Help Topics; arrow the contents,
  Tab into the page, scroll a long topic, read the keybinding table aligned,
  Esc out. (An `rvision` example for `HelpPane` alone is optional until the
  framework `HelpWindow` lands.)

## Open questions

- **Where `HelpViewer` lives in `edit`:** `src/help.rs` (its own module, next to
  the content `help.txt`) vs `src/dialogs/`. Leaning `src/help.rs` — it is larger
  and content-bearing, not a small prompt like the `dialogs/` modals.
- **Exact viewer width rule** with `<pre>` blocks (default width vs widest-pre,
  capped at screen) — settle while building `HelpPane`/`HelpViewer`.
- **The new ADR** for the format + model (lightweight markup, blocks, reserved
  link syntax, `include_str!` now / authoring-tool blob later) — write alongside
  the parser.
