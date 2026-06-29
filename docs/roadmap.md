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

## Phase 5 — Dialogs & controls ✅

- `widgets::Dialog` (modal) + `app::exec_view` modal loop returning a command. ✅
- Controls: `Button`, `InputLine`, `Label`, `CheckBox`, `RadioButtons`,
  `ListBox`/`ListViewer`, `ScrollBar`. ✅
- `MessageBox` (info/confirm); a file **Open/Save** dialog (`ListBox` +
  `InputLine` + `std::fs` directory listing). ✅

**Decisions made while building (see `docs/specs/controls.md`, `dialog.md`, ADR 0017):**
- **The modal loop lives on `Application`.** `exec_view(background, modal)` reuses
  the owned terminal and the same draw→present→poll→drain shape as `run`: it draws
  the background (no events to it), centres and draws the modal on top, and returns
  the first *ending* command the modal posts. No terminal handle escapes into the
  view tree. It runs any `view::Modal` (a `View` that knows its size + ending
  commands), so `Dialog` and `FileDialog` both drop in.
- **Focus-in-draw is a push, not a draw-time argument.** `View::set_focused` (a
  defaulted no-op) lets a control store its focus and draw itself focused; `Group`
  pushes it as focus moves. The `draw(&self, Canvas)` signature is unchanged — this
  resolved the Phase 3/4 focus-in-draw deferral (ADR 0017). A richer `DrawContext`
  (theme/cursor too) is left for later if needed.
- **Controls keep `Enter` for the dialog.** Buttons activate on `Enter`/`Space`;
  the input line, check box, radio group, and list deliberately *ignore* `Enter`
  so it bubbles to the dialog's default button. `Esc` always cancels.
- **The file dialog reads through an injected closure** (real `std::fs` by
  default), so directory navigation is unit-tested without touching the
  filesystem.

**Tests (done):** each control via scripted events + snapshots; `exec_view`
returns OK on the default button and Cancel on `Esc` (scripted terminal); file
dialog lists dirs-first, navigates in/out, and builds the chosen path.

**Exit (met):** `cargo test` green (179 rvision unit tests); `cargo run -p rvision
--example dialogs` is the on-a-real-terminal check — a welcome box, a settings
dialog exercising every control, a file picker, and a closing summary, all
keyboard-driven with the terminal always restored.

---

## Phase 6 — Editor, single document ✅

- `editor::EditorView` (in `edit`): renders the owned document with
  viewport/scroll, a reverse-video caret, and a highlighted selection; display
  geometry (tab expansion, wide graphemes) lives in one `line_columns` helper so
  rendering and vertical motion can never disagree. ✅
- Cursor movement: by grapheme, word (Ctrl-←/→), line, page, home/end, document
  ends (Ctrl-Home/End); vertical motion keeps a sticky goal column. ✅
- Editing: insert/newline/tab/backspace/delete, all through reversible `Edit`s; a
  selection is replaced by typing. ✅
- `edit::file`: the decode/encode seam — UTF-8 (BOM preserved), EOL
  detect-and-preserve, lossy load for non-UTF-8 (ADR 0010). ✅
- `edit::app`: one framed editor window between a menu bar and status line, wired
  to New/Open/Save/Save As/Exit; the binary `edit [FILE]` runs it. ✅

**Decisions made while building (see `docs/specs/editor.md`, `file.md`, `app.md`,
ADR 0018):**
- **The editor uses a bespoke driver loop, not `Application::run` + `Root`**
  (ADR 0018). Modal file dialogs go through `exec_view`, which owns the terminal
  and so can't be called from inside the view tree; the generic `Root` offers no
  hook to interleave one. `edit::app::EditorApp` owns the `EditorView`
  **concretely** (no downcast, no shared `Rc<RefCell>`) and its `dispatch` returns
  the posted commands, so the driver can run the dialog and load/save directly.
- **The document is `'\n'`-only inside; the file seam is the one place that knows
  about CRLF/CR/BOM** — so EOL style and a final-newline survive a load/save
  untouched (ADR 0010), proven by a byte-exact round-trip test.
- **Selection rendering lands now; clipboard and the undo *stack* are Phase 7** —
  editing already flows through reversible `Edit`s, and `selected_text()` sets up
  the clipboard.

**Tests (done):** scripted typing/editing scenarios; goal-column motion across
short/long lines; viewport scroll keeps the cursor visible; selection replace;
byte-exact file round-trip per EOL style; open-then-save preserves CRLF; command
routing (menu/editor/status) without a terminal. `cargo run -p edit` is the
on-a-real-terminal check (open/edit/save, resize, always-restored terminal).

**Exit (met):** `cargo test` green (42 `edit` + 180 `rvision`); a working
single-document editor with menus, a status line, and modal Open/Save dialogs.

---

## Phase 7 — Editing features ✅

- **7a ✅ Selection + clipboard** — selection rendered in Phase 6; Cut/Copy/Paste
  now wired through an app-owned internal clipboard the editor reaches by posting
  commands (ADR 0019). OSC 52 system clipboard is still Phase 10.
- **7b ✅ Undo/redo** — the reversible-edit journal (`edit::history`) wired into
  undo/redo stacks; runs of typing and of in-line deletes coalesce into single
  undo units, cursor moves break the run, and an identity-based saved marker drives
  the dirty flag across undo/redo (ADR 0011).
- **7c Find / Replace** (dialogs + buffer search + repeat-last) **and Go to line.**
  - **7c.1 ✅ Go to Line** — establishes the editor's own modal-dialog pattern
    (`edit::dialogs`, a bespoke `Modal` owning its controls so the driver reads the
    value back; ADR 0017/0018) and `EditorView::go_to_line`. Search menu added.
  - **7c.2 ✅ Find + Find Next** — `edit::search` (a pure line-oriented search
    engine: case/whole-word/direction/wrap) + the Find dialog; `find`/`find_next`
    select and reveal matches. Ctrl+F opens Find, F3 repeats (so F3 is no longer
    Open — that stays on the File menu, matching MS-DOS `EDIT`).
  - **7c.3 ✅ Replace** — the Replace dialog + `EditorView::replace_all` (every
    match rewritten as one undo unit), reporting a count. Interactive
    one-at-a-time replace is a possible later refinement.

---

## Phase 8 — MDI (multi-window) ✅

Multiple editor windows on the desktop; the Window menu; next/prev, cascade,
tile, zoom, close; Alt+1…9 to switch (ADR 0009). `edit::app` owns its documents
**concretely** (ADR 0018), so it does *not* reuse `rvision`'s
`Desktop`/`Window` (which wrap `Box<dyn View>` and would force a downcast): a
`Document` bundles an `EditorView` + path + `Encoding`, and `EditorApp` holds a
`Vec<Document>` with an active index, drawing each as its own framed window.

- **8a.1 ✅** Multi-document model — `EditorApp` owns `Vec<Document>` + an active
  index instead of a single editor; a `Document` carries the editor, path, and
  encoding. Pure refactor: one document, still maximised, behaviour unchanged.
- **8a.2 ✅** Switching + Window menu — New/Open spawn a new window (not replace);
  Alt+1…9, Next (F6) / Previous (Shift-F6) cycle the active document; a Window
  menu (Next/Previous/Close); Close removes the active document (discard guard;
  closing the last leaves a fresh Untitled); Exit confirms *every* dirty document.
  Windows are still drawn maximised — only the active one shows.
- **8b ✅** Overlapping windows — each `Document` carries a `normal` rect
  (desktop-local); an app-level `zoomed` flag maximises the active window (a fresh
  app starts zoomed, so the single-window look is unchanged). Windows draw stacked
  (inactive first, active last with the doubled frame), each with its own scroll
  bars; Cascade / Tile recompute the slots, Zoom (F5) toggles maximise. Drag/resize
  is Phase 9 (mouse).

---

## Phase 9 — Mouse

Fill in mouse *behaviour* across widgets (the positional dispatch phase has
existed since Phase 3, ADR 0004/0007): click-to-focus, menu/button/scrollbar
interaction, window move/resize by drag, drag-select in the editor.

- **9a ✅** Enable capture + the editor-app routing seam — `CrosstermBackend`
  now sends `EnableMouseCapture`/`DisableMouseCapture` around the alternate
  screen; `EditorApp::dispatch` routes `Event::Mouse` instead of dropping it. A
  left-click focuses the window under the pointer (`window_at` hit-tests
  front-to-back in `draw_order`); clicks over bare desktop are ignored.
- **9b ✅** Menu mouse — `MenuBar::handle_event` now answers `Event::Mouse`:
  clicking a title opens it (toggling shut on a second click, switching on a
  click to another title), clicking a pull-down item chooses it (Context-gated,
  so disabled items still post nothing), clicking anywhere off an open menu
  dismisses it, and moving over an item tracks the highlight. A shared
  `pulldown_area` keeps the drawn box and the hit-test from drifting. `EditorApp`
  gives the bar first refusal whenever it is open or the pointer is on its row.
- **9c** Editor mouse.
  - **9c.1 ✅** Interior + wheel — `EditorView::handle_event` now answers
    `Event::Mouse`: a left-press drops the caret under the pointer (`position_at`
    inverts the draw mapping — screen → viewport-local → document, clamping past a
    line's end / below the text) and anchors a selection; a left-drag extends it
    (even past the edge); the wheel pans the view `WHEEL_STEP` lines without
    moving the caret. `EditorApp` focuses the window on a press, routes an
    interior press / any drag to the editor, and sends the wheel to the window
    under the pointer.
  - **9c.2 ✅** Scroll bars — `ScrollBar::hit` classifies a click into a
    `ScrollPart` (arrow / track page / thumb); `EditorApp` hit-tests each
    window's bars (geometry shared with the drawing via `vscroll_rect`/
    `hscroll_rect`) and applies it through `EditorView::scroll_lines`/
    `scroll_cols` — arrows step a line/column, the track pages by a viewport.
    Thumb *dragging* rides on the window drag work (9d).
- **9d** Window chrome drag — title-bar drag to move, border/corner to resize,
  click the close/zoom glyphs.
- **9e** Dialog controls — `Button`, `InputLine`, `CheckBox`, `RadioButtons`,
  `ListViewer`, `ScrollBar` clicks.

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
