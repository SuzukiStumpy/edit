# Module spec: `edit::file`

- **Status:** Done
- **Phase:** 6 (editor, single document) — sub-phase 6b
- **Related ADRs:** 0010 (UTF-8 now, detect/preserve EOL, legacy deferred)

## Purpose

The decode/encode seam between file bytes on disk and the editor's
`'\n'`-normalised document text. It is the *one* place that knows real files use
CRLF/CR line endings and may carry a UTF-8 BOM, so the document model never has
to. It is not responsible for the document model, the editor, or any UI.

## Public interface

```rust
pub enum Eol { Lf, Crlf, Cr }
impl Eol {
    pub const fn as_str(self) -> &'static str;
    pub fn detect(raw: &str) -> Eol;
    pub const fn platform_default() -> Eol;
}

pub struct Encoding { pub eol: Eol, pub bom: bool }
impl Encoding { pub fn new_file() -> Self; }          // platform EOL, no BOM

pub struct Loaded { pub text: String, pub encoding: Encoding, pub lossy: bool }

pub fn decode(bytes: &[u8]) -> Loaded;                 // infallible (lossy on bad UTF-8)
pub fn encode(text: &str, encoding: &Encoding) -> Vec<u8>;
pub fn load(path: &Path) -> io::Result<Loaded>;
pub fn save(path: &Path, text: &str, encoding: &Encoding) -> io::Result<()>;
```

## Behaviour & invariants

- **Decode never fails.** Non-UTF-8 input is decoded with `from_utf8_lossy`
  (replacement chars) and `lossy = true`, so the editor always opens *something*.
- **EOL detected before normalisation:** CRLF if any `\r\n`, else CR if any lone
  `\r`, else LF. `decode` then collapses all of them to `'\n'`.
- **Round-trip fidelity:** for clean UTF-8 with one EOL style,
  `encode(decode(bytes))` reproduces the original bytes — EOL style, BOM, and the
  presence/absence of a final newline all survive (the final-newline state is
  carried implicitly by the normalised text).
- A BOM is stripped on decode and re-prepended on encode iff `encoding.bom`.

## Collaborators

- `std::fs`/`std::str` only (the OS boundary). Used by `edit::app` for Open/Save;
  feeds `edit::editor::EditorView::set_text` / `text`.

## Test plan (done)

- **Logic:** EOL detection per style (and CRLF-wins on a mixed file); decode strips
  BOM + normalises CRLF; invalid UTF-8 is lossy, not an error; encode re-applies
  EOL + BOM.
- **Property-ish:** byte-exact round-trip across LF/CRLF/CR and no-final-newline.
- **Manual:** a real load/save through `std::env::temp_dir()`.

## Open questions

- Legacy codepages (CP437, Windows-1252) slot in behind this same seam later, each
  via a new ADR (ADR 0010) — not needed until a real file demands it.
