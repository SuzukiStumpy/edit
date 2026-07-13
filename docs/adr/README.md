# Architecture Decision Records

Each record captures one significant decision: its context, the choice, and the
consequences. They are append-only history — supersede with a new ADR rather than
rewriting an old one.

Decisions about the framework itself (crossterm seam, view tree, event
dispatch, colour roles, Unicode, mouse, canvas coordinates, the application
shell, modal dialogs, drop shadows, paste, help format) moved to
[`rvision`'s own `docs/adr/`](https://github.com/SuzukiStumpy/rvision/tree/main/docs/adr)
when it was extracted into its own repository. What's left here is specific to
the `edit` binary, plus a handful of decisions that still straddle both (noted
below) and haven't been split out yet.

| #    | Decision |
|------|----------|
| [0008](0008-text-buffer-line-array-first.md) | `TextBuffer` trait, line-array implementation first |
| [0009](0009-document-model-mdi-phased.md) | Multi-window MDI, phased in |
| [0010](0010-file-handling-utf8-eol-preserve.md) | UTF-8 now, detect/preserve EOL, legacy deferred |
| [0011](0011-undo-reversible-edit-journal.md) | Reversible-edit journal for undo/redo |
| [0012](0012-project-layout-workspace.md) | Workspace layout, Rust 2024, pinned MSRV, hand-rolled errors *(straddles; describes the pre-split layout)* |
| [0013](0013-test-strategy-layered-insta.md) | Layered tests + insta snapshots *(straddles; the render/interaction-test infra it describes is rvision's)* |
| [0014](0014-documentation-process.md) | Full documentation process *(straddles; process-only, not code)* |
| [0018](0018-editor-app-bespoke-driver-loop.md) | Editor uses a bespoke driver loop interleaving `exec_view` |
| [0019](0019-clipboard-app-owned-command-routed.md) | Clipboard is app-owned and reached by commands |
| [0021](0021-system-clipboard-osc52-write-only.md) | System clipboard via OSC 52, write-only (`Backend::set_clipboard`) *(straddles; the seam it uses lives in rvision)* |
| [0024](0024-release-versioning-ci.md) | Release-please + lockstep workspace version; hand-rolled cross-platform build on release *(straddles; predates the split)* |
| [0025](0025-settings-key-value-config-dir.md) | Settings: hand-rolled key-value file in the platform config dir |
| [0026](0026-adopt-rvision-help-window-and-resource-loading.md) | Adopt `rvision`'s `HelpWindow` and `resource` module, retiring their `edit`-side equivalents *(help-viewer half superseded by ADR 0027)* |
| [0027](0027-help-window-non-modal-resident-overlay.md) | The help window is a non-modal, resident, standalone overlay — not `exec_view`d |
| [0028](0028-adopt-rvision-arrange.md) | Adopt `rvision::arrange` for chrome hit-testing, drag/resize sessions, and cascade/tile layout |

New decision? Copy [`0000-template.md`](0000-template.md).
