# ADR 0023 — Help content format and topic model

- **Status:** Accepted
- **Date:** 2026-06-30
- **Phase:** 10 (polish — help system)

## Context

The roadmap deferred a help system as "TV's hypertext help is large; ship a
simplified viewer." Turbo Vision's help was a *binary* `.hlp` file produced by a
separate compiler (TVHC) and shown by `THelpFile`/`THelpWindow` in the framework.
We want the same lineage — framework owns the format and viewer, content is an
app concern, an authoring tool is downstream — but simplified.

Decisions taken in the design grilling shape the format:

- Ship a **topic-list + scrollable-page** viewer now; leave **full hypertext**
  (followable cross-links) as its own later phase. But the *source format* must be
  designed so hypertext layers on without a rewrite.
- Content is **hand-authored markup now**; eventually a **separate authoring app**
  (not shipped) emits the help blob that is compiled into the target binary —
  exactly TVHC's role. So a clean `parse(&str) -> HelpContents` boundary must
  exist, with the source coming from `include_str!` today and a tool later.
- The page **reflows prose** to the pane width but must keep **preformatted
  blocks** (keybinding tables) verbatim — so the format needs a `<pre>`-like
  distinction, and the parsed body cannot be a flat string.
- The format, parser, model, and a reusable page renderer live in **`rvision`**
  (editor-agnostic, like `THelpFile`); `edit` supplies only content + wiring.

This ADR records the **format and the parsed model**. The *viewer surface* (a
deferred `rvision` desktop `HelpWindow` vs. `edit`'s modal) and the
**windowing** question it exposed are recorded in the roadmap, not here.

## Decision

**A lightweight, line-oriented markup, parsed by a total scanner into a block
model.**

Format:
- `#`-prefixed line → comment (dropped). Content before the first topic is ignored.
- `@topic <id> <title…>` opens a topic: first token is the `id`, the rest the
  `title`. `id` is the contents-list key **and** the future cross-link target.
- Blank-line-separated runs of text are `Paragraph`s (reflowed; intra-paragraph
  newlines and space-runs collapse).
- `<pre>` / `</pre>` fence a `Preformatted` block kept byte-for-byte verbatim.
- `{label|target}` is an inline link; the **v1 parser keeps only `label`** as
  plain text. `target` (a topic id) is reserved for the hypertext phase.

Model:
```rust
struct HelpContents { /* ordered topics + id index */ }
struct HelpTopic { id: String, title: String, body: Vec<Block> }
enum Block { Paragraph(String), Preformatted(Vec<String>) }
```
`HelpContents::parse` is **infallible**: unknown directives become text, an
unclosed `<pre>` runs to the next `@topic`/end. Authoring errors (duplicate ids,
dangling link targets) are caught by a **content test**, not a runtime `Result`.

## Consequences

- **Hand-authoring is pleasant and the parser is small** — a line scanner, no
  serde, within the crate budget (ADR 0001). The same format is trivial for a
  future authoring tool to emit; the runtime can't tell hand-written from
  tool-written source, because it only sees the `parse` boundary.
- **The format is fully hypertext-ready; the model is minimal but grows
  additively.** Links already have syntax (rendered as label text today); when
  the hypertext phase lands, `Paragraph(String)` becomes a span sequence and the
  parser keeps `target`s — a contained change behind one block variant, plus new
  `Block` variants (headings, lists) as needed. "Build the seam now, fill the
  behaviour later," as elsewhere on the roadmap.
- **`Paragraph` vs `Preformatted` is the reflow seam.** The renderer reflows the
  former via `wrap` (ADR 0006/0015 display-column width) and emits the latter
  verbatim, so keybinding tables stay aligned while prose adapts to width.
- **A total parser keeps help robust** — malformed content degrades to readable
  text rather than crashing the editor — at the cost that authoring mistakes are
  only visible via the content test (acceptable: content ships compiled-in, so
  the test gates every build).

## Alternatives considered

- **In-code `Vec<HelpTopic>` Rust literals (no format).** Simplest mechanically,
  but there is no "format" to future-proof, and authoring prose as string
  literals is unpleasant — and it can't be what an authoring tool targets.
  Rejected: the explicit goal was an extensible authored format.
- **Fully tag-based (HTML/XML-ish) markup.** Internally uniform and natural for a
  generated artifact, but heavier to hand-author and hand-parse now (attributes,
  nesting) for no v1 benefit. The lightweight format borrows just `<pre>`.
- **Markdown subset.** Familiar, but "a subset" invites scope creep and ambiguity
  about what is supported; full Markdown is a large spec we don't want to imply.
- **Flat `Vec<Line { text, preformatted }>` model.** No real structure —
  paragraph grouping stays implicit and headings/lists/links are awkward to add.
  Weakest fit for the extensible-format goal. Rejected for the block model.
- **Fallible `parse -> Result`.** Pushes authoring-error handling into the
  runtime for content that is fixed at compile time. A compile-in content test is
  the right place to catch it; the runtime stays total.
