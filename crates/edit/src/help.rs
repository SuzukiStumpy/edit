//! The editor's help content.
//!
//! The viewer itself is `rvision`'s own [`rvision::widgets::HelpWindow`]
//! (ADR 0026) — `edit` no longer hand-rolls a `ListBox`+`HelpPane` composite.
//!
//! Content lives in `help.txt`, compiled into the binary with `include_str!`
//! (ADR 0023). A future authoring app would emit the same format.

/// The editor's help content, baked into the binary (ADR 0023).
pub const HELP_TEXT: &str = include_str!("help.txt");

#[cfg(test)]
mod tests {
    use super::*;
    use rvision::help::HelpContents;

    // --- the shipped content (a compile-in safety net, ADR 0023) ---

    fn topic_text(t: &rvision::help::HelpTopic) -> String {
        use rvision::help::{Block, Span};
        let mut s = String::new();
        for block in &t.body {
            match block {
                Block::Paragraph(spans) => {
                    for span in spans {
                        match span {
                            Span::Text(text) => s.push_str(text),
                            Span::Link { label, .. } => s.push_str(label),
                        }
                    }
                    s.push('\n');
                }
                Block::Preformatted(lines) => {
                    for l in lines {
                        s.push_str(l);
                        s.push('\n');
                    }
                }
            }
        }
        s
    }

    /// Extracts every `{label|target}` target from raw markup (links are reduced
    /// to label text at parse time, so this scans the source).
    fn link_targets(src: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut rest = src;
        while let Some(o) = rest.find('{') {
            let after = &rest[o + 1..];
            if let Some(bar) = after.find('|') {
                let ab = &after[bar + 1..];
                if let Some(close) = ab.find('}') {
                    out.push(ab[..close].to_string());
                    rest = &ab[close + 1..];
                    continue;
                }
            }
            rest = after;
        }
        out
    }

    #[test]
    fn shipped_content_parses_with_the_expected_topics() {
        let c = HelpContents::parse(HELP_TEXT);
        let ids: Vec<&str> = c.topics().iter().map(|t| t.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "overview",
                "keyboard",
                "clipboard",
                "files",
                "find",
                "settings"
            ]
        );
    }

    #[test]
    fn shipped_topic_ids_are_unique() {
        let c = HelpContents::parse(HELP_TEXT);
        let mut seen = std::collections::BTreeSet::new();
        for t in c.topics() {
            assert!(seen.insert(t.id.clone()), "duplicate topic id {:?}", t.id);
        }
    }

    #[test]
    fn the_clipboard_topic_documents_both_pastes() {
        let c = HelpContents::parse(HELP_TEXT);
        let text = topic_text(c.topic("clipboard").expect("a clipboard topic"));
        assert!(text.contains("Ctrl+V"), "internal paste key documented");
        assert!(text.contains("Ctrl+Shift+V"), "system paste key documented");
    }

    #[test]
    fn every_link_target_resolves() {
        let c = HelpContents::parse(HELP_TEXT);
        for target in link_targets(HELP_TEXT) {
            assert!(
                c.topic(&target).is_some(),
                "dangling help link target {target:?}"
            );
        }
    }
}
