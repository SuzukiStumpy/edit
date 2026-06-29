//! OSC 52 terminal clipboard escapes (ADR 0021).
//!
//! OSC 52 lets a program set the host's clipboard by writing an escape sequence
//! `ESC ] 52 ; c ; <base64> BEL`, where `<base64>` is the standard-alphabet
//! Base64 of the clipboard text's UTF-8 bytes and `c` selects the clipboard (as
//! opposed to the primary selection). We build it ourselves — the encoder is
//! small and pure, so the byte format is unit-tested without a terminal, and it
//! keeps us inside the crate budget (ADR 0001, 0012).
//!
//! Only the *set* direction is implemented. Reading the clipboard back (the `?`
//! query form) is deliberately omitted: it is asynchronous, widely disabled for
//! security, and prompts the user on many terminals (ADR 0021).

/// The OSC 52 escape that sets the system clipboard (`c`) to `text`.
///
/// Write the returned bytes straight to the terminal. The text is Base64-encoded
/// per the spec; an empty string yields a well-formed escape with an empty
/// payload (which clears the clipboard on terminals that honour it).
pub(crate) fn set_clipboard(text: &str) -> String {
    format!("\x1b]52;c;{}\x07", base64_encode(text.as_bytes()))
}

/// Standard Base64 (RFC 4648 — `+/` alphabet, `=` padding) of `bytes`.
fn base64_encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        // Pack up to three bytes into a 24-bit group, missing bytes as zero.
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        let group = (b0 << 16) | (b1 << 8) | b2;
        out.push(ALPHABET[((group >> 18) & 0x3f) as usize] as char);
        out.push(ALPHABET[((group >> 12) & 0x3f) as usize] as char);
        // The last two sextets become '=' when their source bytes are absent.
        out.push(if chunk.len() > 1 {
            ALPHABET[((group >> 6) & 0x3f) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[(group & 0x3f) as usize] as char
        } else {
            '='
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_the_rfc_4648_test_vectors() {
        // The canonical vectors exercise every padding case (0/1/2 trailing '=').
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foob"), "Zm9vYg==");
        assert_eq!(base64_encode(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn base64_handles_non_ascii_bytes() {
        // "é" is U+00E9 -> 0xC3 0xA9 in UTF-8.
        assert_eq!(base64_encode("é".as_bytes()), "w6k=");
    }

    #[test]
    fn set_clipboard_wraps_base64_in_the_osc_52_escape() {
        assert_eq!(set_clipboard("hi"), "\x1b]52;c;aGk=\x07");
        // Empty text is still a well-formed (empty-payload) escape.
        assert_eq!(set_clipboard(""), "\x1b]52;c;\x07");
    }
}
