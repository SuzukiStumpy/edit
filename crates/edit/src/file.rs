//! The file decode/encode seam (ADR 0010): bytes on disk ↔ the editor's
//! `'\n'`-normalised document text, plus the per-file [`Encoding`] (line-ending
//! style and UTF-8 BOM) needed to write the file back the way it came.
//!
//! The document model only ever sees `'\n'` line breaks; this module is the one
//! place that knows real files use CRLF/CR and may carry a BOM. Decoding is
//! infallible — invalid UTF-8 loads lossily and sets [`Loaded::lossy`] — so the
//! editor opens *something* for any file (ADR 0010 "lossy load"). Legacy
//! codepages would slot in behind this same seam later.

use std::io;
use std::path::Path;

/// The UTF-8 byte-order mark.
const BOM: [u8; 3] = [0xEF, 0xBB, 0xBF];

/// A file's line-ending convention.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Eol {
    /// Unix `\n`.
    Lf,
    /// DOS/Windows `\r\n`.
    Crlf,
    /// Classic Mac `\r`.
    Cr,
}

impl Eol {
    /// The byte sequence this convention writes for a line break.
    pub const fn as_str(self) -> &'static str {
        match self {
            Eol::Lf => "\n",
            Eol::Crlf => "\r\n",
            Eol::Cr => "\r",
        }
    }

    /// Detects the convention of `raw` text (before normalisation): CRLF if any
    /// `\r\n` is present, else CR if any lone `\r` is, else LF.
    pub fn detect(raw: &str) -> Eol {
        if raw.contains("\r\n") {
            Eol::Crlf
        } else if raw.contains('\r') {
            Eol::Cr
        } else {
            Eol::Lf
        }
    }

    /// The convention to give a brand-new file on this platform (CRLF on Windows,
    /// LF elsewhere).
    pub const fn platform_default() -> Eol {
        #[cfg(windows)]
        {
            Eol::Crlf
        }
        #[cfg(not(windows))]
        {
            Eol::Lf
        }
    }
}

/// How a file is encoded on disk: its line-ending style and whether it carries a
/// UTF-8 BOM. Preserved across a load/save so the editor never silently rewrites
/// either (ADR 0010).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Encoding {
    /// The line-ending convention.
    pub eol: Eol,
    /// Whether the file begins with a UTF-8 BOM.
    pub bom: bool,
}

impl Encoding {
    /// The encoding for a new file: the platform default EOL and no BOM.
    pub fn new_file() -> Self {
        Self {
            eol: Eol::platform_default(),
            bom: false,
        }
    }
}

impl Default for Encoding {
    fn default() -> Self {
        Self::new_file()
    }
}

/// The result of decoding a file's bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Loaded {
    /// The document text, BOM-stripped and `'\n'`-normalised.
    pub text: String,
    /// The encoding to preserve when saving.
    pub encoding: Encoding,
    /// Whether the bytes were not valid UTF-8 and were decoded lossily.
    pub lossy: bool,
}

/// Replaces every CRLF and lone CR with a single `'\n'`.
fn normalise(raw: &str) -> String {
    raw.replace("\r\n", "\n").replace('\r', "\n")
}

/// Decodes raw file `bytes` into normalised document text plus the [`Encoding`]
/// to preserve. Never fails: non-UTF-8 input is decoded lossily with
/// [`Loaded::lossy`] set.
pub fn decode(bytes: &[u8]) -> Loaded {
    let (bom, rest) = match bytes.strip_prefix(&BOM) {
        Some(rest) => (true, rest),
        None => (false, bytes),
    };
    let (raw, lossy) = match std::str::from_utf8(rest) {
        Ok(s) => (s.to_string(), false),
        Err(_) => (String::from_utf8_lossy(rest).into_owned(), true),
    };
    let eol = Eol::detect(&raw);
    Loaded {
        text: normalise(&raw),
        encoding: Encoding { eol, bom },
        lossy,
    }
}

/// Encodes normalised document `text` back to file bytes: re-applies `encoding`'s
/// line endings and prepends the BOM if it had one.
pub fn encode(text: &str, encoding: &Encoding) -> Vec<u8> {
    let body = match encoding.eol {
        Eol::Lf => text.to_string(),
        other => text.replace('\n', other.as_str()),
    };
    let mut out = Vec::with_capacity(body.len() + if encoding.bom { BOM.len() } else { 0 });
    if encoding.bom {
        out.extend_from_slice(&BOM);
    }
    out.extend_from_slice(body.as_bytes());
    out
}

/// Reads and decodes the file at `path`.
///
/// # Errors
///
/// Propagates any I/O error from reading the file.
pub fn load(path: &Path) -> io::Result<Loaded> {
    Ok(decode(&std::fs::read(path)?))
}

/// Encodes `text` with `encoding` and writes it to `path`.
///
/// # Errors
///
/// Propagates any I/O error from writing the file.
pub fn save(path: &Path, text: &str, encoding: &Encoding) -> io::Result<()> {
    std::fs::write(path, encode(text, encoding))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_each_eol_style() {
        assert_eq!(Eol::detect("a\nb"), Eol::Lf);
        assert_eq!(Eol::detect("a\r\nb"), Eol::Crlf);
        assert_eq!(Eol::detect("a\rb"), Eol::Cr);
        // CRLF wins when a file mixes the two (the \n is part of a \r\n).
        assert_eq!(Eol::detect("a\r\nb\nc"), Eol::Crlf);
    }

    #[test]
    fn decode_strips_bom_and_normalises_crlf() {
        let mut bytes = BOM.to_vec();
        bytes.extend_from_slice(b"line1\r\nline2\r\n");
        let loaded = decode(&bytes);
        assert_eq!(loaded.text, "line1\nline2\n");
        assert_eq!(
            loaded.encoding,
            Encoding {
                eol: Eol::Crlf,
                bom: true
            }
        );
        assert!(!loaded.lossy);
    }

    #[test]
    fn decode_invalid_utf8_is_lossy_not_an_error() {
        let loaded = decode(&[b'a', 0xFF, b'b']);
        assert!(loaded.lossy);
        assert!(
            loaded.text.contains('\u{FFFD}'),
            "replacement char inserted"
        );
    }

    #[test]
    fn encode_reapplies_eol_and_bom() {
        let enc = Encoding {
            eol: Eol::Crlf,
            bom: true,
        };
        let bytes = encode("a\nb\n", &enc);
        let mut expected = BOM.to_vec();
        expected.extend_from_slice(b"a\r\nb\r\n");
        assert_eq!(bytes, expected);
    }

    #[test]
    fn round_trip_preserves_bytes_for_each_style() {
        // A clean UTF-8 file decoded then re-encoded reproduces the original bytes,
        // so EOL style (and a final-newline, or lack of one) survives untouched.
        for original in [
            &b"a\nb\n"[..],
            &b"a\r\nb\r\n"[..],
            &b"a\rb\rc"[..], // CR, no trailing newline
            &b"plain, no newline"[..],
        ] {
            let loaded = decode(original);
            let again = encode(&loaded.text, &loaded.encoding);
            assert_eq!(again, original, "round-trip changed the bytes");
        }
    }

    #[test]
    fn load_and_save_round_trip_through_the_filesystem() {
        let path = std::env::temp_dir().join(format!("edit-file-test-{}.txt", std::process::id()));
        std::fs::write(&path, b"hello\r\nworld\r\n").unwrap();

        let loaded = load(&path).unwrap();
        assert_eq!(loaded.text, "hello\nworld\n");
        assert_eq!(loaded.encoding.eol, Eol::Crlf);

        save(&path, &loaded.text, &loaded.encoding).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"hello\r\nworld\r\n");

        std::fs::remove_file(&path).ok();
    }
}
