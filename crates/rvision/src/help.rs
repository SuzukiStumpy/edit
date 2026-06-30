//! Help content: a lightweight markup format, a total parser, and the parsed
//! topic model (ADR 0023).
//!
//! The framework owns the *format* and the *model*; an application supplies the
//! *content* (baked in with `include_str!`) and a viewer. This mirrors Turbo
//! Vision's split — `THelpFile` in the framework, the `.hlp` blob from a separate
//! compiler — simplified: a hand-authored, line-oriented markup parsed by a
//! [`HelpContents::parse`] scanner that never fails.
//!
//! # Format
//!
//! ```text
//! # A comment line (ignored). Content before the first @topic is dropped.
//!
//! @topic keyboard  Keyboard & mouse     <- id (first token) then title (the rest)
//!
//! A prose paragraph. Newlines inside it are insignificant — the renderer
//! reflows it to the page width. A blank line starts a new paragraph.
//!
//! <pre>
//! Ctrl+S        Save        (verbatim: never reflowed, columns preserved)
//! Ctrl+Shift+V  System paste
//! </pre>
//!
//! @topic clipboard  Clipboard
//! See {the Keyboard topic|keyboard} for the keys.
//! ```
//!
//! - `#` (first non-space char) → comment, dropped — except inside `<pre>`.
//! - `@topic <id> <title…>` opens a topic; `id` is the contents key and the
//!   future cross-link target, `title` the rest of the line.
//! - Blank-line-separated text runs are [`Block::Paragraph`]s; `<pre>`/`</pre>`
//!   fence a verbatim [`Block::Preformatted`].
//! - `{label|target}` is an inline link; the v1 parser keeps only `label` as
//!   plain text (`target`, a topic id, is reserved for the hypertext phase).
//! - Topic order is declaration order; the first topic is the home topic.

/// One unit of a topic's body. Grows additively (headings, lists, inline link
/// spans) as the help system gains features (ADR 0023).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Block {
    /// Prose to be reflowed to the page width. Source line breaks have already
    /// been collapsed to single spaces and links reduced to their label text.
    Paragraph(String),
    /// Verbatim lines, never reflowed — keybinding tables and other aligned
    /// content. Kept byte-for-byte as authored between the `<pre>` fences.
    Preformatted(Vec<String>),
}

/// One help topic: a stable id, a display title, and a body of [`Block`]s.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HelpTopic {
    /// Stable identifier: the contents-list key and the future link target.
    pub id: String,
    /// Human-readable title, shown in the contents list and the page header.
    pub title: String,
    /// The topic's content, in document order.
    pub body: Vec<Block>,
}

/// A parsed help document: an ordered set of [`HelpTopic`]s.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HelpContents {
    topics: Vec<HelpTopic>,
}

impl HelpContents {
    /// Parses the markup into topics. **Infallible**: malformed input degrades
    /// gracefully (an unknown directive becomes text; an unclosed `<pre>` runs to
    /// the next `@topic` or end-of-input). Authoring mistakes — duplicate ids,
    /// dangling link targets — are caught by a content test, not a runtime error.
    pub fn parse(source: &str) -> Self {
        let mut topics: Vec<HelpTopic> = Vec::new();
        let mut current: Option<HelpTopic> = None;
        let mut paragraph: Vec<String> = Vec::new();
        let mut pre: Option<Vec<String>> = None;

        for raw in source.split('\n') {
            let line = raw.strip_suffix('\r').unwrap_or(raw);
            let trimmed = line.trim_start();
            let is_topic = topic_header(trimmed).is_some();

            // Inside a <pre> block, everything is verbatim until </pre>; an
            // unclosed block also ends at the next @topic or end-of-input.
            if pre.is_some() {
                if line.trim() == "</pre>" {
                    flush_pre(&mut current, &mut pre);
                    continue;
                } else if is_topic {
                    flush_pre(&mut current, &mut pre);
                    // fall through to handle the @topic line below
                } else {
                    pre.as_mut().expect("pre is Some").push(line.to_string());
                    continue;
                }
            }

            if trimmed.starts_with('#') {
                continue; // comment
            }

            if let Some((id, title)) = topic_header(trimmed) {
                flush_paragraph(&mut current, &mut paragraph);
                if let Some(t) = current.take() {
                    topics.push(t);
                }
                current = Some(HelpTopic {
                    id: id.to_string(),
                    title: title.to_string(),
                    body: Vec::new(),
                });
                continue;
            }

            if line.trim() == "<pre>" {
                flush_paragraph(&mut current, &mut paragraph);
                pre = Some(Vec::new());
                continue;
            }

            if trimmed.is_empty() {
                flush_paragraph(&mut current, &mut paragraph);
                continue;
            }

            // Ordinary prose — only meaningful inside a topic.
            if current.is_some() {
                paragraph.push(line.trim().to_string());
            }
        }

        flush_pre(&mut current, &mut pre);
        flush_paragraph(&mut current, &mut paragraph);
        if let Some(t) = current.take() {
            topics.push(t);
        }
        Self { topics }
    }

    /// The topics in declaration order.
    pub fn topics(&self) -> &[HelpTopic] {
        &self.topics
    }

    /// The topic with the given `id`, if any.
    pub fn topic(&self, id: &str) -> Option<&HelpTopic> {
        self.topics.iter().find(|t| t.id == id)
    }

    /// The topic titles, in order — the labels for a contents list.
    pub fn titles(&self) -> Vec<&str> {
        self.topics.iter().map(|t| t.title.as_str()).collect()
    }

    /// The home (first) topic, if any.
    pub fn home(&self) -> Option<&HelpTopic> {
        self.topics.first()
    }
}

/// If `trimmed` is an `@topic <id> <title…>` header, returns `(id, title)`.
/// The directive must be followed by whitespace (or be the whole line).
fn topic_header(trimmed: &str) -> Option<(&str, &str)> {
    let rest = trimmed.strip_prefix("@topic")?;
    if !(rest.is_empty() || rest.starts_with(char::is_whitespace)) {
        return None; // e.g. "@topical" is not the directive
    }
    let rest = rest.trim_start();
    let mut parts = rest.splitn(2, char::is_whitespace);
    let id = parts.next().unwrap_or("");
    let title = parts.next().unwrap_or("").trim();
    Some((id, title))
}

/// Joins the accumulated paragraph lines, strips link markup to label text, and
/// pushes a [`Block::Paragraph`] onto the current topic. A no-op if empty.
fn flush_paragraph(current: &mut Option<HelpTopic>, paragraph: &mut Vec<String>) {
    if paragraph.is_empty() {
        return;
    }
    let joined = paragraph.join(" ");
    paragraph.clear();
    if let Some(t) = current.as_mut() {
        t.body.push(Block::Paragraph(strip_links(&joined)));
    }
}

/// Emits the in-progress `<pre>` block (if any) as a [`Block::Preformatted`].
fn flush_pre(current: &mut Option<HelpTopic>, pre: &mut Option<Vec<String>>) {
    if let Some(buf) = pre.take() {
        if let Some(t) = current.as_mut() {
            t.body.push(Block::Preformatted(buf));
        }
    }
}

/// Replaces every well-formed `{label|target}` with `label`. A `{` without a
/// following `|…}` is left literal, so the function is total on any input.
fn strip_links(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        if let Some(bar) = after.find('|') {
            let label = &after[..bar];
            let after_bar = &after[bar + 1..];
            if let Some(close) = after_bar.find('}') {
                out.push_str(label);
                rest = &after_bar[close + 1..];
                continue;
            }
        }
        out.push('{'); // not a link: keep the brace literal
        rest = after;
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn para(s: &str) -> Block {
        Block::Paragraph(s.to_string())
    }
    fn pre(lines: &[&str]) -> Block {
        Block::Preformatted(lines.iter().map(|s| s.to_string()).collect())
    }

    #[test]
    fn a_single_topic_parses_id_title_and_body() {
        let c = HelpContents::parse("@topic kb  Keyboard & mouse\nPress keys.");
        assert_eq!(c.topics().len(), 1);
        let t = &c.topics()[0];
        assert_eq!(t.id, "kb");
        assert_eq!(t.title, "Keyboard & mouse");
        assert_eq!(t.body, vec![para("Press keys.")]);
    }

    #[test]
    fn topics_keep_declaration_order_and_home_is_first() {
        let c = HelpContents::parse("@topic a  Alpha\n@topic b  Beta\n@topic c  Gamma");
        assert_eq!(c.titles(), vec!["Alpha", "Beta", "Gamma"]);
        assert_eq!(c.home().unwrap().id, "a");
        assert_eq!(c.topic("b").unwrap().title, "Beta");
        assert!(c.topic("missing").is_none());
    }

    #[test]
    fn blank_lines_separate_paragraphs_and_collapse_internal_breaks() {
        let c = HelpContents::parse("@topic t  T\nline one\nstill one\n\nparagraph two");
        assert_eq!(
            c.topics()[0].body,
            vec![para("line one still one"), para("paragraph two")]
        );
    }

    #[test]
    fn pre_blocks_are_kept_verbatim() {
        let src = "@topic t  T\nintro\n\n<pre>\nCtrl+S        Save\n  indented  row\n\nblank above\n</pre>\nafter";
        let c = HelpContents::parse(src);
        assert_eq!(
            c.topics()[0].body,
            vec![
                para("intro"),
                pre(&["Ctrl+S        Save", "  indented  row", "", "blank above"]),
                para("after"),
            ]
        );
    }

    #[test]
    fn a_hash_inside_pre_is_content_not_a_comment() {
        let c = HelpContents::parse("@topic t  T\n<pre>\n# not a comment\n</pre>");
        assert_eq!(c.topics()[0].body, vec![pre(&["# not a comment"])]);
    }

    #[test]
    fn links_reduce_to_their_label_text() {
        let c = HelpContents::parse("@topic t  T\nSee {the keys|keyboard} and {paste|clipboard}.");
        assert_eq!(c.topics()[0].body, vec![para("See the keys and paste.")]);
    }

    #[test]
    fn a_lone_brace_is_left_literal() {
        let c = HelpContents::parse("@topic t  T\nUse {braces} and { alone.");
        assert_eq!(c.topics()[0].body, vec![para("Use {braces} and { alone.")]);
    }

    #[test]
    fn comments_and_preamble_before_the_first_topic_are_dropped() {
        let c =
            HelpContents::parse("# header comment\nstray prose\n@topic t  T\n# mid comment\nbody");
        assert_eq!(c.topics().len(), 1);
        assert_eq!(c.topics()[0].body, vec![para("body")]);
    }

    #[test]
    fn an_unclosed_pre_runs_to_the_next_topic() {
        let c = HelpContents::parse("@topic a  A\n<pre>\nverbatim\n@topic b  B\nprose");
        assert_eq!(c.topics().len(), 2);
        assert_eq!(c.topics()[0].body, vec![pre(&["verbatim"])]);
        assert_eq!(c.topics()[1].body, vec![para("prose")]);
    }

    #[test]
    fn an_unclosed_pre_runs_to_end_of_input() {
        let c = HelpContents::parse("@topic a  A\n<pre>\none\ntwo");
        assert_eq!(c.topics()[0].body, vec![pre(&["one", "two"])]);
    }

    #[test]
    fn crlf_line_endings_are_normalised() {
        let c = HelpContents::parse("@topic t  T\r\nprose\r\n<pre>\r\nrow\r\n</pre>\r\n");
        assert_eq!(c.topics()[0].body, vec![para("prose"), pre(&["row"])]);
    }

    #[test]
    fn topical_is_not_the_topic_directive() {
        // A word that merely starts with "@topic" is ordinary prose.
        let c = HelpContents::parse("@topic t  T\n@topical not a directive");
        assert_eq!(c.topics().len(), 1);
        assert_eq!(c.topics()[0].body, vec![para("@topical not a directive")]);
    }

    #[test]
    fn empty_input_yields_no_topics() {
        assert!(HelpContents::parse("").topics().is_empty());
        assert!(HelpContents::parse("").home().is_none());
    }
}
