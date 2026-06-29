# Architecture Decision Records

Each record captures one significant decision: its context, the choice, and the
consequences. They are append-only history — supersede with a new ADR rather than
rewriting an old one.

| #    | Decision |
|------|----------|
| [0001](0001-terminal-backend-crossterm.md) | Use crossterm at the OS/terminal boundary |
| [0002](0002-render-seam-backend-double-buffer.md) | Backend/EventSource traits + double-buffer cell diff |
| [0003](0003-view-model-trait-objects-messages.md) | Retained-mode view tree: trait objects + message passing |
| [0004](0004-event-engine-three-phase.md) | Three-phase event dispatch, `EventResult`, modal `exec_view` |
| [0005](0005-colour-roles-truecolour-ready.md) | Semantic colour roles over a truecolour-ready type |
| [0006](0006-unicode-full-now.md) | Full Unicode now (width + segmentation data crates) |
| [0007](0007-mouse-architected-keyboard-first.md) | Architect for mouse, build keyboard-first |
| [0008](0008-text-buffer-line-array-first.md) | `TextBuffer` trait, line-array implementation first |
| [0009](0009-document-model-mdi-phased.md) | Multi-window MDI, phased in |
| [0010](0010-file-handling-utf8-eol-preserve.md) | UTF-8 now, detect/preserve EOL, legacy deferred |
| [0011](0011-undo-reversible-edit-journal.md) | Reversible-edit journal for undo/redo |
| [0012](0012-project-layout-workspace.md) | Workspace layout, Rust 2024, pinned MSRV, hand-rolled errors |
| [0013](0013-test-strategy-layered-insta.md) | Layered tests + insta snapshots |
| [0014](0014-documentation-process.md) | Full documentation process |
| [0015](0015-view-coordinates-canvas.md) | Owner-relative view coordinates via a translating `Canvas` |
| [0016](0016-application-shell-menu-overlay.md) | `TProgram`-style application shell + drawn menu overlay |
| [0017](0017-modal-dialogs-and-focus-aware-controls.md) | Modal dialogs via `exec_view` + focus-aware controls (`set_focused`) |
| [0018](0018-editor-app-bespoke-driver-loop.md) | Editor uses a bespoke driver loop interleaving `exec_view` |
| [0019](0019-clipboard-app-owned-command-routed.md) | Clipboard is app-owned and reached by commands |
| [0020](0020-drop-shadows-per-view-protocol.md) | Drop shadows are a per-view protocol (`View::drop_shadow`) |

New decision? Copy [`0000-template.md`](0000-template.md).
