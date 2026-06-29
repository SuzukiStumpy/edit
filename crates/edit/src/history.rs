//! The editor's undo/redo journal (ADR 0011): two stacks of reversible-edit
//! [`Record`]s, with consecutive same-kind keystrokes coalesced into one record
//! and an identity-based "saved" marker driving the dirty flag.
//!
//! `History` is pure: it never holds a document and never applies an [`Edit`]. The
//! editor pops a record, applies its edits to its own buffer (forward to redo,
//! inverted to undo), and hands the record to the opposite stack — so this module
//! is just bookkeeping and is unit-tested without a buffer (see
//! `docs/specs/history.md`).

use crate::text::{Edit, Position, position_after};

/// How (and whether) the *next* record may merge into this one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Coalesce {
    /// A single-grapheme insertion (ordinary typing).
    Typing,
    /// A single-grapheme deletion (Backspace / Delete).
    Deleting,
    /// An action that is its own undo unit and closes the run (Enter, paste, cut,
    /// replace-selection, line-join).
    Standalone,
}

/// One undoable action: the edits to replay plus the caret before and after.
#[derive(Debug, Clone)]
pub(crate) struct Record {
    /// Unique id, used only for the saved-marker comparison.
    id: u64,
    /// Caret to restore on undo (where it sat before the action).
    before: Position,
    /// Caret to restore on redo (where the action left it).
    after: Position,
    /// The forward edits, applied in order (undo inverts them in reverse).
    edits: Vec<Edit>,
    /// How the next record may coalesce into this one.
    coalesce: Coalesce,
}

impl Record {
    /// The caret to restore when this record is undone.
    pub(crate) fn before(&self) -> Position {
        self.before
    }

    /// The caret to restore when this record is redone.
    pub(crate) fn after(&self) -> Position {
        self.after
    }

    /// The forward edits (apply in order to redo; invert in reverse to undo).
    pub(crate) fn edits(&self) -> &[Edit] {
        &self.edits
    }
}

/// The undo/redo journal: an undo stack, a redo stack, and the saved marker.
#[derive(Debug)]
pub(crate) struct History {
    undo: Vec<Record>,
    redo: Vec<Record>,
    /// The id to assign the next freshly-pushed record.
    next_id: u64,
    /// Whether the top undo record is still open to coalescing.
    open: bool,
    /// The id on top of the undo stack at the last save (`None` = saved at empty).
    saved_id: Option<u64>,
}

impl History {
    /// An empty journal: nothing to undo or redo, document clean.
    pub(crate) fn new() -> Self {
        Self {
            undo: Vec::new(),
            redo: Vec::new(),
            next_id: 1,
            open: false,
            saved_id: None,
        }
    }

    /// Records a new action, clearing the redo stack and coalescing into the open
    /// top record when the kinds match and abut.
    pub(crate) fn record(
        &mut self,
        before: Position,
        edits: Vec<Edit>,
        after: Position,
        coalesce: Coalesce,
    ) {
        self.redo.clear();
        let candidate = Record {
            id: 0,
            before,
            after,
            edits,
            coalesce,
        };
        if self.try_coalesce(&candidate) {
            return; // merged into the open top record; the run stays open
        }
        let id = self.next_id;
        self.next_id += 1;
        self.undo.push(Record { id, ..candidate });
        self.open = coalesce != Coalesce::Standalone;
    }

    /// Merges `candidate` into the open top undo record if it may; returns whether
    /// it did. Only single-edit records of the same kind that abut merge.
    fn try_coalesce(&mut self, candidate: &Record) -> bool {
        if !self.open || candidate.edits.len() != 1 {
            return false;
        }
        let Some(top) = self.undo.last_mut() else {
            return false;
        };
        if top.coalesce != candidate.coalesce || top.edits.len() != 1 {
            return false;
        }
        match top.coalesce {
            Coalesce::Typing => {
                let (
                    Edit::Insert { at, text },
                    Edit::Insert {
                        at: c_at,
                        text: c_text,
                    },
                ) = (&mut top.edits[0], &candidate.edits[0])
                else {
                    return false;
                };
                if *c_at != position_after(*at, text) {
                    return false;
                }
                text.push_str(c_text);
            }
            Coalesce::Deleting => {
                let (
                    Edit::Delete { at, text },
                    Edit::Delete {
                        at: c_at,
                        text: c_text,
                    },
                ) = (&mut top.edits[0], &candidate.edits[0])
                else {
                    return false;
                };
                if position_after(*c_at, c_text) == *at {
                    // Backspace: the new deletion abuts on the left.
                    *text = format!("{c_text}{text}");
                    *at = *c_at;
                } else if *c_at == *at {
                    // Forward delete: each removes the grapheme at the same spot.
                    text.push_str(c_text);
                } else {
                    return false;
                }
            }
            Coalesce::Standalone => return false,
        }
        top.after = candidate.after;
        true
    }

    /// Pops the top undo record (the editor applies its inverses), or `None`.
    pub(crate) fn take_undo(&mut self) -> Option<Record> {
        self.open = false;
        self.undo.pop()
    }

    /// Pushes a just-undone record onto the redo stack.
    pub(crate) fn push_redo(&mut self, rec: Record) {
        self.redo.push(rec);
    }

    /// Pops the top redo record (the editor re-applies its edits), or `None`.
    pub(crate) fn take_redo(&mut self) -> Option<Record> {
        self.open = false;
        self.redo.pop()
    }

    /// Pushes a just-redone record back onto the undo stack (sealed: the run is
    /// closed, so later typing starts a fresh record).
    pub(crate) fn push_undo(&mut self, rec: Record) {
        self.undo.push(rec);
    }

    /// Closes the open coalescing run (called when the caret moves), so the next
    /// keystroke starts a new undo unit.
    pub(crate) fn break_run(&mut self) {
        self.open = false;
    }

    /// Whether there is anything to undo.
    pub(crate) fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    /// Whether there is anything to redo.
    pub(crate) fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Whether the document differs from its last saved state — the id on top of
    /// the undo stack no longer matches the one pinned at save (ADR 0011).
    pub(crate) fn is_modified(&self) -> bool {
        self.undo.last().map(|r| r.id) != self.saved_id
    }

    /// Pins the current top-of-undo id as "saved" and seals the run, so a later
    /// edit starts a distinct record (and reads as modified).
    pub(crate) fn mark_saved(&mut self) {
        self.saved_id = self.undo.last().map(|r| r.id);
        self.open = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pos(line: usize, col: usize) -> Position {
        Position::new(line, col)
    }

    /// Records a single-char insert at `(0, col)` as a typing keystroke.
    fn type_char(h: &mut History, col: usize, ch: &str) {
        let at = pos(0, col);
        h.record(
            at,
            vec![Edit::insert(at, ch)],
            pos(0, col + 1),
            Coalesce::Typing,
        );
    }

    #[test]
    fn record_then_take_undo_returns_it_for_inversion() {
        let mut h = History::new();
        type_char(&mut h, 0, "a");
        assert!(h.can_undo());
        let rec = h.take_undo().expect("one record");
        assert_eq!(rec.before(), pos(0, 0));
        assert_eq!(rec.after(), pos(0, 1));
        assert_eq!(rec.edits(), &[Edit::insert(pos(0, 0), "a")]);
        assert!(!h.can_undo());
    }

    #[test]
    fn consecutive_typing_coalesces_into_one_record() {
        let mut h = History::new();
        type_char(&mut h, 0, "a");
        type_char(&mut h, 1, "b");
        type_char(&mut h, 2, "c");
        let rec = h.take_undo().expect("coalesced record");
        // One record holding the whole run, from the first `before` to the last.
        assert_eq!(rec.edits(), &[Edit::insert(pos(0, 0), "abc")]);
        assert_eq!(rec.before(), pos(0, 0));
        assert_eq!(rec.after(), pos(0, 3));
        assert!(!h.can_undo(), "the three keystrokes are one undo unit");
    }

    #[test]
    fn a_break_splits_typing_into_two_records() {
        let mut h = History::new();
        type_char(&mut h, 0, "a");
        h.break_run(); // e.g. the user pressed an arrow key
        type_char(&mut h, 1, "b");
        assert_eq!(
            h.take_undo().unwrap().edits(),
            &[Edit::insert(pos(0, 1), "b")]
        );
        assert_eq!(
            h.take_undo().unwrap().edits(),
            &[Edit::insert(pos(0, 0), "a")]
        );
    }

    #[test]
    fn non_adjacent_typing_does_not_merge() {
        let mut h = History::new();
        type_char(&mut h, 0, "a");
        type_char(&mut h, 5, "b"); // a gap: not where the last insert ended
        assert!(h.take_undo().is_some());
        assert!(h.take_undo().is_some(), "two separate records");
    }

    #[test]
    fn backspace_runs_coalesce() {
        // Two backspaces: delete "b" at (0,1) then "a" at (0,0), abutting leftward.
        let mut h = History::new();
        h.record(
            pos(0, 2),
            vec![Edit::delete(pos(0, 1), "b")],
            pos(0, 1),
            Coalesce::Deleting,
        );
        h.record(
            pos(0, 1),
            vec![Edit::delete(pos(0, 0), "a")],
            pos(0, 0),
            Coalesce::Deleting,
        );
        let rec = h.take_undo().expect("coalesced deletes");
        assert_eq!(rec.edits(), &[Edit::delete(pos(0, 0), "ab")]);
        assert_eq!(rec.before(), pos(0, 2), "the run's original caret-before");
        assert_eq!(rec.after(), pos(0, 0));
        assert!(!h.can_undo());
    }

    #[test]
    fn forward_delete_runs_coalesce() {
        // Two forward-deletes at the same spot remove "a" then "b".
        let mut h = History::new();
        h.record(
            pos(0, 0),
            vec![Edit::delete(pos(0, 0), "a")],
            pos(0, 0),
            Coalesce::Deleting,
        );
        h.record(
            pos(0, 0),
            vec![Edit::delete(pos(0, 0), "b")],
            pos(0, 0),
            Coalesce::Deleting,
        );
        let rec = h.take_undo().expect("coalesced forward deletes");
        assert_eq!(rec.edits(), &[Edit::delete(pos(0, 0), "ab")]);
    }

    #[test]
    fn typing_and_deleting_do_not_merge() {
        let mut h = History::new();
        type_char(&mut h, 0, "a");
        h.record(
            pos(0, 1),
            vec![Edit::delete(pos(0, 0), "a")],
            pos(0, 0),
            Coalesce::Deleting,
        );
        assert!(h.take_undo().is_some());
        assert!(h.take_undo().is_some(), "different kinds stay separate");
    }

    #[test]
    fn standalone_never_coalesces() {
        let mut h = History::new();
        h.record(
            pos(0, 0),
            vec![Edit::insert(pos(0, 0), "\n")],
            pos(1, 0),
            Coalesce::Standalone,
        );
        type_char(&mut h, 0, "a"); // a normal keystroke after Enter is its own unit
        assert!(h.take_undo().is_some());
        assert!(h.take_undo().is_some());
    }

    #[test]
    fn redo_replays_and_a_new_edit_clears_it() {
        let mut h = History::new();
        type_char(&mut h, 0, "a");
        let rec = h.take_undo().unwrap();
        h.push_redo(rec);
        assert!(h.can_redo());
        // Redo the record back onto the undo stack.
        let rec = h.take_redo().unwrap();
        h.push_undo(rec);
        assert!(h.can_undo() && !h.can_redo());
        // A fresh edit while there is redo history would clear it.
        if let Some(r) = h.take_undo() {
            h.push_redo(r);
        }
        assert!(h.can_redo());
        type_char(&mut h, 0, "z");
        assert!(!h.can_redo(), "recording invalidates redo");
    }

    #[test]
    fn dirty_flag_follows_the_saved_marker() {
        let mut h = History::new();
        assert!(!h.is_modified(), "a fresh journal is clean");
        type_char(&mut h, 0, "a");
        assert!(h.is_modified());
        h.mark_saved();
        assert!(!h.is_modified(), "clean right after saving");
        // Undo past the save: modified again.
        let rec = h.take_undo().unwrap();
        h.push_redo(rec);
        assert!(h.is_modified());
        // Redo back to the saved state: clean once more.
        let rec = h.take_redo().unwrap();
        h.push_undo(rec);
        assert!(!h.is_modified());
    }

    #[test]
    fn dirty_flag_is_identity_based_not_depth_based() {
        // Save at depth 1, undo to depth 0, then a *different* edit lands back at
        // depth 1 — it must still read modified (its record carries a fresh id).
        let mut h = History::new();
        type_char(&mut h, 0, "a");
        h.mark_saved();
        let rec = h.take_undo().unwrap();
        h.push_redo(rec);
        type_char(&mut h, 0, "x"); // clears redo, pushes a new id at depth 1
        assert!(h.is_modified(), "same depth, different content => modified");
    }
}
