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

## Phase 1 — Rendering core + text model ✅

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
- `backend`: `Backend` trait + `TestBackend` (headless). The input half,
  `EventSource`, lands in Phase 2 with the `Event` type it carries.
- diff: front/back double buffer producing a minimal change set (ADR 0002).
- Snapshot harness: activate `insta`; helper to render a `Buffer` to a text grid.

**Text-model track** — lives in the `edit` crate, not `rvision`: the document
model is editor-specific, so it stays out of the framework (CLAUDE.md, ADR 0008).
- `edit::text::TextBuffer` trait; line-array impl (`Vec<String>`); activate
  `unicode-segmentation` for grapheme navigation (ADR 0008).
- `edit::text::Edit` reversible operation type with `apply`/`invert`
  (property test: `invert(apply(x)) == x`) (ADR 0011).

**Tests first:** rect math; width of CJK/emoji/combining sequences; diff emits
only changed cells; snapshot of a boxed, shadowed region; line split/join;
grapheme cursor steps; `Edit` round-trip.

**Exit (met):** a styled screen composes in memory, snapshots, and presents
through a `Backend` emitting only changed cells; the text model edits and inverts
correctly. No real terminal yet.

---

## Phase 2 — Real terminal & event loop ✅

- `event`: `Event { Key, Mouse, Command, Broadcast, Resize, Idle }`,
  `EventResult { Consumed, Ignored }` (ADR 0004). ✅
- `backend::EventSource` trait (input half of the ADR 0002 seam) + the
  `CrosstermBackend`/crossterm event source; raw mode, alternate screen, flush the
  diff via crossterm, map crossterm input → our `Event`. Activate `crossterm`. ✅
- `app::Application` skeleton: init → `poll(timeout)`/`read` loop → draw →
  teardown. Panic-safe RAII restore + panic hook. ✅
- An `examples/` demo that paints a screen and quits on Ctrl-Q. ✅
  (`cargo run -p rvision --example hello`)

**Decisions made while building (see `docs/specs/`):**
- `Backend::present` is now `-> io::Result<()>`: a real flush is fallible, and the
  loop must surface I/O errors. `TestBackend` returns `Ok(())`.
- `CrosstermBackend` implements *both* `Backend` and `EventSource` (it owns one
  terminal); `Application` is generic over `T: Backend + EventSource`. It lives in
  its own `crossterm_backend` module so crossterm stays confined (ADR 0001).
- The loop drives a `Program` trait (`draw`/`handle_event`/`is_finished`). Quit is
  a flag the program sets (no command bubbling yet); Phase 3 replaces this with a
  `cmQuit` command up the owner chain. `Program` is a stepping stone to the Phase 3
  view-tree root.
- The pure `map_event` (crossterm → our `Event`) is the unit-tested core; live
  terminal I/O is checked by the demo.

**Exit (met):** manually verified on Linux — `cargo run -p rvision --example hello`
draws a screen, resizes cleanly, and always restores the terminal, even on panic
(the `panic!` path in the demo). The headless loop is covered by unit tests against
the scripted backend.

---

## Phase 3 — View system ✅

- `canvas::Canvas`: translating, clipping draw surface over a `Buffer` — the draw
  half of the view seam (ADR 0015). Views draw in local coords; `child()` carves a
  sub-surface per child. ✅
- `view::View` trait: `bounds`, `draw`, `handle_event`, `focusable`. ✅
- `view::Group`: owns children (`Vec<Box<dyn View>>`), z-order draw, focus chain,
  three-phase dispatch (positional → focused → broadcast) (ADR 0003, 0004). ✅
- `view::Context`: a handler's outbound channel — posts commands (enabled-gated)
  and broadcasts without view-to-view references. ✅
- `command`: `Command` ids, `CommandSet` enable/disable, the `CM_USER`
  framework/app boundary; bubbling is the recursive dispatch unwinding. ✅
- Basic views: `StaticText` and a minimal focusable test view (`Probe`, in tests). ✅
- `app::Root`: bridges the view tree to the Phase 2 `Program` loop — dispatches an
  event, drains posted commands/broadcasts and re-dispatches them, quits on
  `CM_QUIT`. Replaces Phase 2's quit-flag stepping stone. ✅

**Decisions made while building (see `docs/specs/`, ADR 0015):**
- **Coordinates are owner-relative**, drawn through a translating/clipping
  `Canvas` (ADR 0015), not absolute screen coords — keeps Phase 8/9 (MDI, window
  drag) from becoming a coordinate rewrite. The grapheme→cell iteration is shared
  with `Buffer::put_str` via one `cell::cells_of` helper.
- **Commands bubble for free**: routing a focused event down the focus chain and
  letting the result unwind *is* the up-the-owner-chain bubble — no back-refs, no
  IDs needed yet.
- **The command id space is open and partitioned**: framework standard ids live
  below `CM_USER`, apps number from there up; `Event` stays a closed enum, so app
  extensibility is *adding command ids*, never new event variants.
- **Deferred (noted in `docs/specs/view.md`):** focus-aware drawing (Phase 5),
  integer view IDs, cross-group Tab hand-off, and `Theme`-threaded draw (chrome).

**Tests first (done):** positional hit-testing + point translation; focus
traversal (Tab/Shift-Tab, skipping non-focusable); command posting, enable/disable
gating, and down-then-up routing; z-ordered draw (snapshot); the `Root` loop bridge
end-to-end through the scripted backend.

---

## Phase 4 — Application chrome ✅

- `widgets::Background` + `widgets::Desktop` (blue backdrop + a stack of windows,
  the top one active). ✅
- `widgets::Frame` + `widgets::Window` (title, close/zoom glyphs, doubled border
  when active; drag/resize is Phase 9). ✅
- `widgets::MenuBar` + pull-down `Menu` (navigation, accelerators, dispatch). ✅
- `widgets::StatusLine` (labelled global hot-keys). ✅
- `app::Shell` — the `TProgram`-style application root that lays the three out,
  draws the menu overlay, and routes keys. ✅

**Decisions made while building (see `docs/specs/widgets.md`, `shell.md`, ADR 0016):**
- **A purpose-built `app::Shell`, not a generic `Group`.** TurboVision's `TProgram`
  needs live layout (regions carved from the terminal size each frame, so resize
  relays out), a menu pull-down drawn as a full-frame **overlay** on top of
  everything (a one-row menu bar can't draw below itself through a clipped child
  canvas), and three local key passes — menu bar (pre-process: `Alt`-hot-keys and,
  while open, modal) → active window (focused) → status line (post-process). The
  generic event engine and `Group` stayed untouched (ADR 0016).
- **The `Desktop` owns concrete `Window`s** (not `Box<dyn View>`) so it can mark
  the active one; a `Window` wraps a `Box<dyn View>` interior (an editor in Phase
  6) and paints its own background so a non-filling interior shows solid.
- **Menu modality without `exec_view`.** The open/highlight state lives on the
  `MenuBar` as a small state machine; Phase 5's modal `exec_view` will be able to
  re-express the pull-down as a modal view without disturbing the data or layout.
- **Deferred (noted in the specs):** greying disabled menu items / active-frame
  styling at draw time (needs `CommandSet`/focus in `draw` — same family as the
  Phase 5 focus-in-draw item); all menu/window **mouse** behaviour (Phase 9).

**Exit (met):** a recognisably TurboVision screen — menu bar, status line, empty
blue desktop with a framed window — driven entirely by the keyboard. The headless
`Shell` snapshot composes the whole screen; `cargo run -p rvision --example chrome`
is the on-a-real-terminal check (menus open on `Alt`/`F10`, `Alt-X` exits, resize
relays out, the terminal is always restored).

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
