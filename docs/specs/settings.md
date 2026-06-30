# Module spec: `edit::settings`

- **Status:** Done
- **Phase:** 10 (Settings persistence)
- **Related ADRs:** 0025 (format + location), 0001/0013 (crate budget, test seams),
  0018 (the editor app owns its state concretely)

## Purpose

Persist a small set of user preferences across runs of `edit`, in a hand-rolled
key-value file under the platform config directory. It owns: the on-disk
**format** (parse/render), the **location** (per-OS path resolution with no extra
crate), and the in-memory `Settings` value the app reads at startup and writes
back when a setting changes.

It is **not** the application state itself — `EditorApp` keeps owning tab width,
the Find options, and the recent-files list; `settings` only loads them in and
saves them out. It knows nothing about widgets or the view tree.

## Public interface

```rust
/// The persisted preferences. `Default` is the built-in fallback used when no
/// config file exists or a field is missing/malformed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settings {
    pub tab_width: usize,            // clamped to 1..=16 on load/set
    pub find_case_sensitive: bool,
    pub find_whole_word: bool,
    pub recent_limit: usize,         // 0..=MAX_RECENT; the user-chosen MRU length
    pub recent: Vec<PathBuf>,        // most-recent first, deduped, capped to recent_limit
}

impl Default for Settings { /* tab_width 8, options off, recent_limit 8, no recent */ }

impl Settings {
    /// Push `path` to the front of the MRU, de-duplicating and capping to recent_limit.
    pub fn record_recent(&mut self, path: PathBuf);
    pub fn set_tab_width(&mut self, width: usize);       // clamps
    pub fn set_recent_limit(&mut self, limit: usize);    // clamps to MAX_RECENT, trims list
    /// Reset every *preference* to default, keeping the recent *history* (re-capped).
    pub fn reset_keeping_recent(&mut self);

    /// Load from the platform config path; missing file -> Default (not an error).
    pub fn load() -> io::Result<Settings>;
    /// Render and write to the platform config path, creating parent dirs.
    pub fn save(&self) -> io::Result<()>;
}

// --- the testable seams underneath the two convenience methods ---

/// Lenient parse: unknown keys and malformed lines are ignored; bad values fall
/// back to the field default. Never fails.
pub fn parse(text: &str) -> Settings;
/// Deterministic render (stable key order) so the round-trip is stable.
pub fn render(s: &Settings) -> String;

pub fn load_from(path: &Path) -> io::Result<Settings>;   // missing -> Default
pub fn save_to(path: &Path, s: &Settings) -> io::Result<()>;

/// Resolves the file path: `$EDIT_CONFIG_PATH` override, else per-OS. The per-OS
/// resolvers take an injected env lookup so each is unit-tested on every host.
pub fn config_path() -> Option<PathBuf>;
fn unix_config_path(get: impl Fn(&str) -> Option<String>) -> Option<PathBuf>;
fn macos_config_path(get: impl Fn(&str) -> Option<String>) -> Option<PathBuf>;
fn windows_config_path(get: impl Fn(&str) -> Option<String>) -> Option<PathBuf>;
```

## File format

`key = value`, one per line. `#` starts a comment; blank lines ignored. Whitespace
around the key and value is trimmed. The list field repeats its key, newest first.

```
# edit configuration — written automatically; hand-edit if you like.
tab_width = 4
find.case_sensitive = true
find.whole_word = false
recent = /home/me/notes.txt
recent = /home/me/main.rs
```

Keys: `tab_width` (usize), `find.case_sensitive` / `find.whole_word` (bool:
`true`/`false`), `recent` (one path per line, in MRU order).

## Location (ADR 0025)

File name `config`, under a per-OS directory, resolved by hand from env vars (no
`dirs` crate — crate budget, ADR 0001):

| OS | Directory |
|----|-----------|
| Linux/BSD | `$XDG_CONFIG_HOME/edit`, else `$HOME/.config/edit` |
| macOS | `$HOME/Library/Application Support/edit` |
| Windows | `%APPDATA%\edit` |

If neither the primary nor the fallback env var is set, `config_path()` returns
`None`: load yields `Default`, save is a silent no-op (we never invent a path).
`$EDIT_CONFIG_PATH`, if set, overrides everything with an explicit file path — a
testability seam (a test or CI run redirects persistence away from the real
per-user config) that doubles as a power-user override.

## Behaviour & invariants

- **Lenient load is total.** A corrupt or partial file never panics or errors —
  unknown keys, bad ints/bools, and junk lines are dropped; present, valid fields
  win. This keeps a hand-edit or a future-version file from bricking startup.
- **`tab_width` is clamped** to `TAB_WIDTH_RANGE` (1..=16) on parse and on
  `record`/set, so a `0` or absurd value can't break rendering or motion.
- **`recent` is newest-first, de-duplicated, capped** at `recent_limit`.
  `record_recent` moves an existing path to the front rather than duplicating it.
  `recent_limit` is itself clamped to `0..=MAX_RECENT` (9 — the reserved
  command-id block); `set_recent_limit` trims the list to match.
- **Round-trip is stable:** `parse(render(s)) == s` for any *normalised* `s`
  (clamped tab width, capped/deduped recent). Property-tested.
- **Missing file is normal**, not an error; a missing config dir on save is
  created (`create_dir_all`).

## Collaborators

- `EditorApp` (in `app.rs`) owns a `Settings`, loads it in `main`, seeds the
  Find dialog and each new `EditorView`'s tab width from it, rebuilds the File
  menu's recent-files items from `recent`, and calls `save()` when a setting
  changes (tab width, MRU length, a used Find option, an opened/saved file).
  Wiring lives in `app.rs`, not here (ADR 0018 — the app owns its state concretely).
- `dialogs::SettingsDialog` (in `app.rs`'s `open_settings`) edits `tab_width` and
  `recent_limit`; its "Reset to defaults" posts a bespoke `CM_DEFAULTS` that
  drives `Settings::reset_keeping_recent`.
- `std::fs`/`std::env` only; no widget or `rvision` dependency.

## Test plan (write these first)

- **Logic:** parse a full file; parse with unknown keys / blank lines / comments;
  malformed `tab_width` and bad bool fall back to default; `record_recent` dedups
  and caps and re-orders; `tab_width` clamp.
- **Property:** `parse(render(s)) == s` for normalised `Settings` (generated
  inputs, then normalise before compare).
- **I/O:** `save_to` then `load_from` round-trips through a temp dir; `load_from`
  a non-existent path yields `Default`.
- **Path:** the per-OS resolvers return the right path given env, honour the
  XDG/HOME fallback, return `None` when the env is empty, and `$EDIT_CONFIG_PATH`
  overrides; `recent_limit` parses/clamps and trims the list; `reset_keeping_recent`
  resets prefs but keeps history.
- **Dialog (scripted events):** fields seed from current settings; editing reads
  back; Tab cycles all five controls; Reset posts `CM_DEFAULTS`; Esc cancels.
- **Manual:** Edit ▸ Settings — change tab width / MRU length, OK, confirm it
  takes effect and persists across a restart; Reset to defaults; delete the file —
  `edit` starts on defaults.

## Open questions

- "Search backwards" is treated as a per-search choice, not persisted (only
  `case` and `whole_word` are preferences). Revisit if it feels wrong in use.
- Window layout / zoom persistence was scoped out of v1 (multi-doc geometry is
  ambiguous); the format can gain keys later without breaking older files.
- The Settings dialog covers tab width and MRU length; other preferences stay
  transparent (auto-persisted, reachable only via Reset to defaults or a hand-edit).
  More knobs can join the dialog as the need appears.
