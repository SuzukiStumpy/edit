//! Persisted user preferences (Phase 10, ADR 0025).
//!
//! A small set of settings survives across runs of `edit` in a hand-rolled
//! `key = value` file under the platform config directory — no `serde`, no
//! `dirs` crate (crate budget, ADR 0001). This module owns the on-disk *format*
//! (`parse`/`render`), the per-OS *location* ([`config_path`]), and the in-memory
//! [`Settings`] value the application reads at startup and writes back when a
//! preference changes.
//!
//! It is deliberately *not* the application state: [`crate::app::EditorApp`] keeps
//! owning the tab width, the Find options, and the recent-files list (ADR 0018);
//! `settings` only loads them in and saves them out. Parsing is **total and
//! lenient** — unknown keys, malformed lines, and bad values are ignored and the
//! field keeps its default — so a hand-edit or an older/newer file never blocks
//! startup.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// The hard cap on the recent-files list: the most entries it can ever hold, and
/// the size of the reserved command-id block for the File-menu entries. The
/// *user-chosen* length is [`Settings::recent_limit`], always `<=` this. Single
/// digit so the File-menu entries stay numbered 1..9.
pub const MAX_RECENT: usize = 9;

/// The default recent-files length (preserving the pre-configurable behaviour).
const DEFAULT_RECENT_LIMIT: usize = 8;

/// The built-in default tab width (also the editor's, see `editor::DEFAULT_TAB_WIDTH`).
const DEFAULT_TAB_WIDTH: usize = 8;
/// Tab width is clamped into this inclusive range on load and on update, so a
/// `0` or absurd value can never reach rendering or cursor motion.
const TAB_WIDTH_MIN: usize = 1;
const TAB_WIDTH_MAX: usize = 16;

/// The application's directory name under the platform config root.
const APP_DIR: &str = "edit";
/// The settings file's name within [`APP_DIR`].
const FILE_NAME: &str = "config";

/// The persisted preferences. [`Default`] is the fallback used when no config
/// file exists or a field is missing/malformed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settings {
    /// Editor tab stop width, in columns (clamped to 1..=16).
    pub tab_width: usize,
    /// The Find dialog's "Case sensitive" default.
    pub find_case_sensitive: bool,
    /// The Find dialog's "Whole word" default.
    pub find_whole_word: bool,
    /// How many recent files to keep and show on the File menu (`0..=MAX_RECENT`;
    /// `0` turns the list off).
    pub recent_limit: usize,
    /// Recently opened/saved files, most-recent first, de-duplicated, capped at
    /// [`recent_limit`](Settings::recent_limit).
    pub recent: Vec<PathBuf>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            tab_width: DEFAULT_TAB_WIDTH,
            find_case_sensitive: false,
            find_whole_word: false,
            recent_limit: DEFAULT_RECENT_LIMIT,
            recent: Vec::new(),
        }
    }
}

impl Settings {
    /// Moves `path` to the front of the recent list, de-duplicating any existing
    /// entry and capping the list at [`recent_limit`](Settings::recent_limit).
    pub fn record_recent(&mut self, path: PathBuf) {
        self.recent.retain(|p| p != &path);
        self.recent.insert(0, path);
        self.recent.truncate(self.recent_limit);
    }

    /// Sets the tab width, clamped into the valid range.
    pub fn set_tab_width(&mut self, width: usize) {
        self.tab_width = clamp_tab(width);
    }

    /// Sets the recent-files length (clamped to `0..=MAX_RECENT`), trimming the
    /// stored list to match immediately.
    pub fn set_recent_limit(&mut self, limit: usize) {
        self.recent_limit = limit.min(MAX_RECENT);
        self.recent.truncate(self.recent_limit);
    }

    /// Resets every *preference* to its default (tab width, recent length, the
    /// Find options) while keeping the recent-files *history*, re-capped to the
    /// restored length. Backs the dialog's "Reset to defaults" (ADR 0025).
    pub fn reset_keeping_recent(&mut self) {
        let recent = std::mem::take(&mut self.recent);
        *self = Settings::default();
        self.recent = recent;
        self.recent.truncate(self.recent_limit);
    }

    /// Loads the settings from the platform config path. A missing file (or no
    /// resolvable config directory) yields [`Settings::default`] — not an error.
    ///
    /// # Errors
    ///
    /// Propagates a read error other than "not found".
    pub fn load() -> io::Result<Settings> {
        match config_path() {
            Some(path) => load_from(&path),
            None => Ok(Settings::default()),
        }
    }

    /// Renders and writes the settings to the platform config path, creating the
    /// config directory if needed. A no-op when no config directory can be
    /// resolved (the relevant environment variables are unset).
    ///
    /// # Errors
    ///
    /// Propagates any I/O error from creating the directory or writing the file.
    pub fn save(&self) -> io::Result<()> {
        match config_path() {
            Some(path) => save_to(&path, self),
            None => Ok(()),
        }
    }
}

/// Clamps a tab width into the supported range.
fn clamp_tab(width: usize) -> usize {
    width.clamp(TAB_WIDTH_MIN, TAB_WIDTH_MAX)
}

/// Parses a `key = value` settings file. Total and lenient: comment (`#`) and
/// blank lines are skipped, unknown keys and malformed lines are ignored, and a
/// value that fails to parse leaves the field at its default. The `recent` key
/// repeats, newest-first; the result is de-duplicated and capped.
pub fn parse(text: &str) -> Settings {
    let mut settings = Settings::default();
    let mut recent = Vec::new();

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let (key, value) = (key.trim(), value.trim());
        match key {
            "tab_width" => {
                if let Ok(n) = value.parse::<usize>() {
                    settings.tab_width = clamp_tab(n);
                }
            }
            "recent_limit" => {
                if let Ok(n) = value.parse::<usize>() {
                    settings.recent_limit = n.min(MAX_RECENT);
                }
            }
            "find.case_sensitive" => {
                if let Some(b) = parse_bool(value) {
                    settings.find_case_sensitive = b;
                }
            }
            "find.whole_word" => {
                if let Some(b) = parse_bool(value) {
                    settings.find_whole_word = b;
                }
            }
            "recent" => {
                if !value.is_empty() {
                    recent.push(PathBuf::from(value));
                }
            }
            _ => {}
        }
    }

    settings.recent = normalise_recent(recent, settings.recent_limit);
    settings
}

/// Renders the settings to the on-disk format. Deterministic key order, so the
/// round-trip `parse(render(s))` is stable.
pub fn render(settings: &Settings) -> String {
    let mut out = String::new();
    out.push_str("# edit configuration — written automatically; hand-edit if you like.\n");
    out.push_str(&format!("tab_width = {}\n", clamp_tab(settings.tab_width)));
    out.push_str(&format!(
        "recent_limit = {}\n",
        settings.recent_limit.min(MAX_RECENT)
    ));
    out.push_str(&format!(
        "find.case_sensitive = {}\n",
        settings.find_case_sensitive
    ));
    out.push_str(&format!("find.whole_word = {}\n", settings.find_whole_word));
    for path in normalise_recent(settings.recent.clone(), settings.recent_limit) {
        out.push_str(&format!("recent = {}\n", path.display()));
    }
    out
}

/// `"true"`/`"false"` (case-insensitively) to a bool; anything else is `None` so
/// the field keeps its default.
fn parse_bool(value: &str) -> Option<bool> {
    match value.to_ascii_lowercase().as_str() {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

/// De-duplicates a recent list (keeping the first, i.e. newest, occurrence) and
/// caps it at `limit`.
fn normalise_recent(paths: Vec<PathBuf>, limit: usize) -> Vec<PathBuf> {
    let mut seen = Vec::new();
    for path in paths {
        if seen.len() == limit {
            break;
        }
        if !seen.contains(&path) {
            seen.push(path);
        }
    }
    seen
}

/// Reads and parses settings from an explicit path. A non-existent file yields
/// [`Settings::default`].
///
/// # Errors
///
/// Propagates a read error other than "not found".
pub fn load_from(path: &Path) -> io::Result<Settings> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(parse(&text)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Settings::default()),
        Err(e) => Err(e),
    }
}

/// Renders and writes settings to an explicit path, creating parent directories.
///
/// # Errors
///
/// Propagates any I/O error from creating the directory or writing the file.
pub fn save_to(path: &Path, settings: &Settings) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }
    fs::write(path, render(settings))
}

/// The platform-appropriate settings file path, or `None` when the environment
/// gives us nowhere to put it. See ADR 0025 for the per-OS rules.
///
/// `$EDIT_CONFIG_PATH`, if set, overrides everything with an explicit file path —
/// a testability seam (so a test redirects persistence away from the real
/// per-user config) that doubles as a power-user override.
pub fn config_path() -> Option<PathBuf> {
    let get = |key: &str| std::env::var(key).ok();
    if let Some(explicit) = config_override(&get) {
        return Some(explicit);
    }
    // `cfg!` keeps all three branches in the AST (unlike `#[cfg]`), so each
    // resolver stays referenced and unit-tested on every host.
    if cfg!(target_os = "windows") {
        windows_config_path(get)
    } else if cfg!(target_os = "macos") {
        macos_config_path(get)
    } else {
        unix_config_path(get)
    }
}

/// The explicit `$EDIT_CONFIG_PATH` override (a whole file path), if set.
fn config_override(get: &impl Fn(&str) -> Option<String>) -> Option<PathBuf> {
    nonempty(get, "EDIT_CONFIG_PATH").map(PathBuf::from)
}

/// An environment value, treating an unset *or empty* variable as absent.
fn nonempty(get: &impl Fn(&str) -> Option<String>, key: &str) -> Option<String> {
    get(key).filter(|v| !v.is_empty())
}

/// Linux/BSD: `$XDG_CONFIG_HOME/edit/config`, else `$HOME/.config/edit/config`.
fn unix_config_path(get: impl Fn(&str) -> Option<String>) -> Option<PathBuf> {
    let base = nonempty(&get, "XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| nonempty(&get, "HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join(APP_DIR).join(FILE_NAME))
}

/// macOS: `$HOME/Library/Application Support/edit/config`.
fn macos_config_path(get: impl Fn(&str) -> Option<String>) -> Option<PathBuf> {
    let base = nonempty(&get, "HOME")
        .map(|h| PathBuf::from(h).join("Library").join("Application Support"))?;
    Some(base.join(APP_DIR).join(FILE_NAME))
}

/// Windows: `%APPDATA%\edit\config`.
fn windows_config_path(get: impl Fn(&str) -> Option<String>) -> Option<PathBuf> {
    let base = nonempty(&get, "APPDATA").map(PathBuf::from)?;
    Some(base.join(APP_DIR).join(FILE_NAME))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    /// An env lookup backed by a fixed table, for the path resolvers.
    fn env<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |key: &str| {
            pairs
                .iter()
                .find(|(k, _)| *k == key)
                .map(|(_, v)| v.to_string())
        }
    }

    /// A unique temp directory for an I/O test (no tempfile crate).
    fn unique_temp_dir() -> PathBuf {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!("edit-settings-test-{}-{}", std::process::id(), n))
    }

    // --- parse ---

    #[test]
    fn parses_a_full_file() {
        let s = parse(
            "# a comment\n\
             tab_width = 4\n\
             find.case_sensitive = true\n\
             find.whole_word = false\n\
             recent = /a/one.txt\n\
             recent = /b/two.rs\n",
        );
        assert_eq!(s.tab_width, 4);
        assert!(s.find_case_sensitive);
        assert!(!s.find_whole_word);
        assert_eq!(
            s.recent,
            vec![PathBuf::from("/a/one.txt"), PathBuf::from("/b/two.rs")]
        );
    }

    #[test]
    fn ignores_comments_blank_lines_and_unknown_keys() {
        let s = parse(
            "\n   \n# header\nfuture_key = 99\ntab_width = 2\n  # indented comment\nnonsense line\n",
        );
        assert_eq!(s.tab_width, 2);
        assert_eq!(
            s,
            Settings {
                tab_width: 2,
                ..Settings::default()
            }
        );
    }

    #[test]
    fn malformed_values_fall_back_to_default() {
        let s = parse("tab_width = wide\nfind.case_sensitive = yes\n");
        assert_eq!(s.tab_width, DEFAULT_TAB_WIDTH);
        assert!(!s.find_case_sensitive); // "yes" is not a recognised bool
    }

    #[test]
    fn tab_width_is_clamped_on_parse() {
        assert_eq!(parse("tab_width = 0\n").tab_width, TAB_WIDTH_MIN);
        assert_eq!(parse("tab_width = 999\n").tab_width, TAB_WIDTH_MAX);
    }

    #[test]
    fn recent_is_deduped_and_capped_on_parse() {
        let mut text = String::from("recent = /dup.txt\n");
        for i in 0..MAX_RECENT + 5 {
            text.push_str(&format!("recent = /f{i}.txt\n"));
        }
        text.push_str("recent = /dup.txt\n"); // second mention dropped
        let s = parse(&text);
        assert_eq!(s.recent.len(), DEFAULT_RECENT_LIMIT); // capped to the default length
        assert_eq!(s.recent[0], PathBuf::from("/dup.txt"));
        // No duplicates survive.
        let mut sorted = s.recent.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), s.recent.len());
    }

    #[test]
    fn bools_are_case_insensitive() {
        let s = parse("find.case_sensitive = TRUE\nfind.whole_word = False\n");
        assert!(s.find_case_sensitive);
        assert!(!s.find_whole_word);
    }

    // --- record_recent ---

    #[test]
    fn record_recent_moves_existing_to_front_without_duplicating() {
        let mut s = Settings::default();
        s.record_recent(PathBuf::from("/a"));
        s.record_recent(PathBuf::from("/b"));
        s.record_recent(PathBuf::from("/a")); // re-open /a
        assert_eq!(s.recent, vec![PathBuf::from("/a"), PathBuf::from("/b")]);
    }

    #[test]
    fn record_recent_caps_the_list() {
        let mut s = Settings::default();
        let n = s.recent_limit + 3;
        for i in 0..n {
            s.record_recent(PathBuf::from(format!("/f{i}")));
        }
        assert_eq!(s.recent.len(), DEFAULT_RECENT_LIMIT);
        // Newest first.
        assert_eq!(s.recent[0], PathBuf::from(format!("/f{}", n - 1)));
    }

    #[test]
    fn recent_limit_parses_clamps_and_caps_the_list() {
        // Honoured, and caps the parsed recent list.
        let s = parse("recent_limit = 2\nrecent = /a\nrecent = /b\nrecent = /c\n");
        assert_eq!(s.recent_limit, 2);
        assert_eq!(s.recent, vec![PathBuf::from("/a"), PathBuf::from("/b")]);
        // Clamped to the hard cap.
        assert_eq!(parse("recent_limit = 99\n").recent_limit, MAX_RECENT);
        // 0 turns the list off.
        assert_eq!(
            parse("recent_limit = 0\nrecent = /a\n").recent,
            Vec::<PathBuf>::new()
        );
    }

    #[test]
    fn set_recent_limit_clamps_and_trims_immediately() {
        let mut s = Settings::default();
        for i in 0..5 {
            s.record_recent(PathBuf::from(format!("/f{i}")));
        }
        s.set_recent_limit(2);
        assert_eq!(s.recent_limit, 2);
        assert_eq!(s.recent.len(), 2);
        s.set_recent_limit(99);
        assert_eq!(s.recent_limit, MAX_RECENT); // clamped, list unchanged (already shorter)
        assert_eq!(s.recent.len(), 2);
    }

    #[test]
    fn reset_keeping_recent_restores_prefs_but_keeps_history() {
        let mut s = Settings {
            tab_width: 2,
            find_case_sensitive: true,
            find_whole_word: true,
            recent_limit: 3,
            recent: vec![PathBuf::from("/a"), PathBuf::from("/b")],
        };
        s.reset_keeping_recent();
        assert_eq!(s.tab_width, DEFAULT_TAB_WIDTH);
        assert!(!s.find_case_sensitive && !s.find_whole_word);
        assert_eq!(s.recent_limit, DEFAULT_RECENT_LIMIT);
        // History survives the reset.
        assert_eq!(s.recent, vec![PathBuf::from("/a"), PathBuf::from("/b")]);
    }

    // --- round-trip property (hand-rolled; no proptest in the crate budget) ---

    #[test]
    fn parse_of_render_is_identity_for_normalised_settings() {
        let widths = [1usize, 4, 8, 16];
        let bools = [false, true];
        let recents: [&[&str]; 3] = [&[], &["/only.txt"], &["/a.txt", "/b.txt", "/c.txt"]];
        for &tab_width in &widths {
            for &case in &bools {
                for &word in &bools {
                    for paths in &recents {
                        let s = Settings {
                            tab_width,
                            find_case_sensitive: case,
                            find_whole_word: word,
                            recent_limit: DEFAULT_RECENT_LIMIT,
                            recent: paths.iter().map(PathBuf::from).collect(),
                        };
                        // `s` is already normalised (clamped width, recent within limit).
                        assert_eq!(parse(&render(&s)), s);
                    }
                }
            }
        }
    }

    // --- I/O ---

    #[test]
    fn save_then_load_round_trips_through_a_temp_dir() {
        let dir = unique_temp_dir();
        let path = dir.join(APP_DIR).join(FILE_NAME); // exercises create_dir_all
        let mut s = Settings::default();
        s.set_tab_width(3);
        s.find_whole_word = true;
        s.record_recent(PathBuf::from("/x.txt"));

        save_to(&path, &s).unwrap();
        let loaded = load_from(&path).unwrap();
        assert_eq!(loaded, s);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_from_a_missing_file_is_default_not_error() {
        let path = unique_temp_dir().join("never-written");
        assert_eq!(load_from(&path).unwrap(), Settings::default());
    }

    // --- path resolution ---

    /// Builds an expected path with the host's separator (so these assertions
    /// hold on the Windows CI runner too, where `join` uses `\`).
    fn expect(base: &str, rest: &[&str]) -> Option<PathBuf> {
        let mut p = PathBuf::from(base);
        for seg in rest {
            p.push(seg);
        }
        p.push(APP_DIR);
        p.push(FILE_NAME);
        Some(p)
    }

    #[test]
    fn unix_prefers_xdg_then_falls_back_to_home() {
        let with_xdg = unix_config_path(env(&[("XDG_CONFIG_HOME", "/cfg"), ("HOME", "/home/me")]));
        assert_eq!(with_xdg, expect("/cfg", &[]));

        let home_only = unix_config_path(env(&[("HOME", "/home/me")]));
        assert_eq!(home_only, expect("/home/me", &[".config"]));
    }

    #[test]
    fn empty_env_values_count_as_unset() {
        // Empty XDG_CONFIG_HOME falls through to HOME.
        let s = unix_config_path(env(&[("XDG_CONFIG_HOME", ""), ("HOME", "/h")]));
        assert_eq!(s, expect("/h", &[".config"]));
        // Nothing set at all -> no path.
        assert_eq!(unix_config_path(env(&[])), None);
    }

    #[test]
    fn macos_uses_application_support() {
        let s = macos_config_path(env(&[("HOME", "/Users/me")]));
        assert_eq!(s, expect("/Users/me", &["Library", "Application Support"]));
        assert_eq!(macos_config_path(env(&[])), None);
    }

    #[test]
    fn explicit_override_wins_and_empty_is_ignored() {
        assert_eq!(
            config_override(&env(&[("EDIT_CONFIG_PATH", "/custom/edit.cfg")])),
            Some(PathBuf::from("/custom/edit.cfg"))
        );
        assert_eq!(config_override(&env(&[("EDIT_CONFIG_PATH", "")])), None);
        assert_eq!(config_override(&env(&[])), None);
    }

    #[test]
    fn windows_uses_appdata() {
        let s = windows_config_path(env(&[("APPDATA", r"C:\Users\me\AppData\Roaming")]));
        assert_eq!(s, expect(r"C:\Users\me\AppData\Roaming", &[]));
        assert_eq!(windows_config_path(env(&[])), None);
    }
}
