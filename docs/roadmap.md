# Roadmap

Phased delivery plan for `rvision` (the framework) and `edit` (the editor).
Each phase is shippable and fully tested before the next begins. Decisions
referenced as `ADR NNNN` live in [`adr/`](adr/).

**Guiding rule:** build the seam now, the feature later. Mouse, MDI, truecolour,
undo, and legacy encodings all have their hooks designed in early and their
behaviour filled in at the phase named below.

Per module, follow the process in
[`module-spec-template.md`](module-spec-template.md): spec → tests → code.

---

## Phase 0 — Scaffolding & docs spine ✅ (this commit)

Workspace (`rvision` + `edit`), pinned toolchain, `.gitignore`, README,
`CLAUDE.md`, the ADR set, this roadmap, the module-spec template, and the first
passing test (`geometry::Point`) that proves the TDD harness.

**Exit:** `cargo test` is green (one trivial unit). *Blocked here only because a
Rust toolchain isn't installed in this environment — see the report.*

---

## Phase 1 — Rendering core + text model

Pure logic, ideal first TDD. Two parallel tracks.

**Framework track**
- `geometry`: `Point` (seeded), `Size`, `Rect` (intersection, contains, clamp).
- `color`: `Color { Default, Named(Color16), Rgb }` (truecolour-ready, ADR 0005),
  `Style` (fg/bg + attributes: bold/underline/reverse).
- `theme`: semantic `Role` enum (e.g. `DesktopBackground`, `WindowFrame`,
  `MenuNormal`, `MenuSelected`, `ButtonFocused`, `EditorText`, `Selection`) →
  `Style`; a default 16-colour CGA `Theme`.
- `cell`: `Cell { grapheme, width, style }`; activate `unicode-width`.
- `buffer`: `Buffer` (grid of cells) + draw primitives — `put_str` (width-aware,
  with wide-char continuation cells), `fill`, `draw_box`, `shadow`.
- `backend`: `Backend` + `EventSource` traits; `TestBackend` (headless).
- diff: front/back double buffer producing a minimal change set (ADR 0002).
- Snapshot harness: activate `insta`; helper to render a `Buffer` to a text grid.

**Text-model track**
- `text::TextBuffer` trait; line-array impl (`Vec<String>`); activate
  `unicode-segmentation` for grapheme navigation (ADR 0008).
- `text::Edit` reversible operation type with `apply`/`invert`
  (property test: `invert(apply(x)) == x`) (ADR 0011).

**Tests first:** rect math; width of CJK/emoji/combining sequences; diff emits
only changed cells; snapshot of a boxed, shadowed region; line split/join;
grapheme cursor steps; `Edit` round-trip.

**Exit:** can compose a styled screen in memory and snapshot it; text model
edits and inverts correctly. No real terminal yet.

---

## Phase 2 — Real terminal & event loop

- `backend::CrosstermBackend`: raw mode, alternate screen, flush the diff via
  crossterm, map crossterm input → our `Event`. Activate `crossterm`.
- `event`: `Event { Key, Mouse, Command, Broadcast, Resize, Idle }`,
  `EventResult { Consumed, Ignored }` (ADR 0004).
- `app::Application` skeleton: init → `poll(timeout)`/`read` loop → draw →
  teardown. Panic-safe RAII restore + panic hook.
- An `examples/` demo that paints a screen and quits on Ctrl-Q.

**Exit (first manual verify on Linux):** `cargo run` shows a drawn screen, resizes
cleanly, and always restores the terminal — even on panic.

---

## Phase 3 — View system

- `view::View` trait: `bounds`, `draw`, `handle_event`, focus state/options.
- `view::Group`: owns children, z-order, focus chain, three-phase dispatch
  (positional → focused → broadcast), redraw orchestration (ADR 0003, 0004).
- `command`: `Command` ids, enable/disable sets, bubbling up the owner chain.
- Basic views: `StaticText` and a minimal focusable test view.

**Tests first:** positional hit-testing; focus traversal (Tab/Shift-Tab);
command bubbling and enable/disable; z-ordered draw.

---

## Phase 4 — Application chrome

- `app`: `Application`/`Program` + `Desktop` (background group) + blue backdrop.
- `widgets::Window` + `Frame` (title, close/zoom glyphs; drag/resize is Phase 9).
- `widgets::MenuBar` + pull-down `Menu` (navigation, accelerators, dispatch).
- `widgets::StatusLine` (status items + context hints).

**Exit:** a recognisably TurboVision screen — menu bar, status line, empty blue
desktop — driven entirely by the keyboard.

---

## Phase 5 — Dialogs & controls

- `widgets::Dialog` (modal) + `app::exec_view` modal loop returning a command.
- Controls: `Button`, `InputLine`, `Label`, `CheckBox`, `RadioButtons`,
  `ListBox`/`ListViewer`, `ScrollBar`.
- `MessageBox` (info/confirm); a file **Open/Save** dialog (`ListBox` +
  `InputLine` + `std::fs` directory listing).

**Tests first:** each control via scripted events + snapshots; `exec_view`
returns the right command for OK/Cancel; file dialog navigation.

---

## Phase 6 — Editor, single document

- `editor::EditorView`: render a `TextBuffer` with viewport/scroll, cursor, and
  selection rendering.
- Cursor movement: by grapheme, word, line, page, home/end, document ends.
- Editing: insert/delete/newline/tab (all via reversible `Edit`).
- File ops: New/Open/Save/Save As through the file dialog; EOL detect-and-preserve;
  UTF-8 via the decode/encode seam (ADR 0010).
- One editor window on the desktop, wired to menu/status commands.

**Tests first:** scripted typing/editing scenarios; viewport scrolls to keep the
cursor visible; round-trip a file preserving its EOL style.

---

## Phase 7 — Editing features

- Selection + clipboard (internal clipboard first; OSC 52 system clipboard in
  Phase 10).
- Undo/redo: wire the reversible-edit journal into undo/redo stacks; coalesce
  consecutive typing into sensible undo units (ADR 0011).
- Find / Replace (dialogs + buffer search + repeat-last).
- Go to line.

---

## Phase 8 — MDI (multi-window)

- Multiple editor windows on the desktop; the Window menu; next/prev, cascade,
  tile, zoom, close; Alt+1…9 to switch (ADR 0009).

---

## Phase 9 — Mouse

- Fill in mouse behaviour across widgets (the positional phase has existed since
  Phase 3): click-to-focus, menu/button/scrollbar interaction, window
  move/resize by drag, drag-select in the editor (ADR 0007).

---

## Phase 10 — Polish & cross-platform

- Verify on Windows and macOS; iron out terminal quirks.
- OSC 52 system clipboard (works over SSH, no crate).
- Settings persistence (hand-rolled key-value format — no serde).
- Help system: a simplified viewer + content; About box.
- Performance pass; rustdoc completeness; rounded-out `examples/`.

---

## Deferred decisions (settled when their phase arrives)

- **System clipboard** — internal → OSC 52 (Phase 7/10).
- **Settings format** — hand-rolled key-value (Phase 10).
- **Help system** — TV's hypertext help is large; ship a simplified viewer
  (Phase 10).
- **Legacy encodings / gap buffer / rope** — behind their seams (decode layer,
  `TextBuffer` trait); add only if a real need appears, each via a new ADR.
