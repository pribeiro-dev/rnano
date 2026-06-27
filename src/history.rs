//! Undo/redo history ring for the editor.
//!
//! Each edit produces a [`HistoryEntry`] that stores both the forward operation
//! (to re-apply on redo) and the inverse operation (to apply on undo), plus
//! before/after cursor positions.
//!
//! Consecutive single-character inserts at adjacent positions are **squashed**
//! into a single entry so that one undo reverses a full word of typing.

use std::collections::VecDeque;

/// A single reversible edit expressed as a rope operation.
#[derive(Debug, Clone)]
pub enum Op {
    /// Insert `text` starting at char position `pos`.
    Insert { pos: usize, text: String },
    /// Delete `char_len` characters starting at char position `pos`.
    Delete { pos: usize, char_len: usize },
}

/// One entry in the undo/redo ring.
#[derive(Debug, Clone)]
pub struct HistoryEntry {
    /// Apply this to **undo** (inverse of what happened).
    pub to_undo: Op,
    /// Apply this to **redo** (re-apply what happened).
    pub to_redo: Op,
    /// Cursor position `(row, col)` before this edit.
    pub before: (usize, usize),
    /// Cursor position `(row, col)` after this edit.
    pub after: (usize, usize),
}

/// Bounded undo/redo ring.
pub struct History {
    pub undo: VecDeque<HistoryEntry>,
    redo: Vec<HistoryEntry>,
    max_depth: usize,
}

impl History {
    pub fn new(max_depth: usize) -> Self {
        History {
            undo: VecDeque::new(),
            redo: Vec::new(),
            max_depth: max_depth.max(1),
        }
    }

    /// Push a new completed edit.  Clears the redo stack.
    pub fn push(&mut self, entry: HistoryEntry) {
        self.redo.clear();
        self.undo.push_back(entry);
        if self.undo.len() > self.max_depth {
            self.undo.pop_front();
        }
    }

    /// Try to squash a single-char insert into the top entry (consecutive
    /// typing coalesces into one undoable unit).  Returns `true` if the entry
    /// was extended in place; `false` if a fresh push is needed.
    pub fn try_squash_insert(&mut self, pos: usize, ch: char, after: (usize, usize)) -> bool {
        let Some(top) = self.undo.back_mut() else {
            return false;
        };
        // The top must be an Insert (i.e., its undo is a Delete).
        let Op::Delete {
            pos: del_pos,
            char_len,
        } = &mut top.to_undo
        else {
            return false;
        };
        // New insert must immediately follow the previous one.
        if *del_pos + *char_len != pos {
            return false;
        }
        // Extend both directions.
        *char_len += 1;
        let Op::Insert { ref mut text, .. } = top.to_redo else {
            return false;
        };
        text.push(ch);
        top.after = after;
        true
    }

    /// Pop the top undo entry and push it to the redo stack.
    pub fn pop_undo(&mut self) -> Option<HistoryEntry> {
        let entry = self.undo.pop_back()?;
        self.redo.push(entry.clone());
        Some(entry)
    }

    /// Pop the top redo entry and push it back to the undo stack.
    pub fn pop_redo(&mut self) -> Option<HistoryEntry> {
        let entry = self.redo.pop()?;
        self.undo.push_back(entry.clone());
        Some(entry)
    }

    #[allow(dead_code)]
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    #[allow(dead_code)]
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ins(pos: usize, text: &str) -> HistoryEntry {
        HistoryEntry {
            to_undo: Op::Delete {
                pos,
                char_len: text.chars().count(),
            },
            to_redo: Op::Insert {
                pos,
                text: text.to_string(),
            },
            before: (0, pos),
            after: (0, pos + text.chars().count()),
        }
    }

    #[test]
    fn push_and_pop_undo() {
        let mut h = History::new(100);
        h.push(ins(0, "a"));
        h.push(ins(1, "b"));
        let e = h.pop_undo().unwrap();
        assert!(matches!(e.to_redo, Op::Insert { pos: 1, .. }));
        assert!(h.can_redo());
    }

    #[test]
    fn redo_after_undo() {
        let mut h = History::new(100);
        h.push(ins(0, "a"));
        h.pop_undo();
        let e = h.pop_redo().unwrap();
        assert!(matches!(e.to_redo, Op::Insert { pos: 0, .. }));
        assert!(!h.can_redo());
        assert!(h.can_undo());
    }

    #[test]
    fn new_edit_clears_redo() {
        let mut h = History::new(100);
        h.push(ins(0, "a"));
        h.pop_undo();
        assert!(h.can_redo());
        h.push(ins(0, "b")); // new edit
        assert!(!h.can_redo());
    }

    #[test]
    fn squash_consecutive_inserts() {
        let mut h = History::new(100);
        h.push(ins(0, "a")); // first char
        // Second char at pos 1 should squash.
        let squashed = h.try_squash_insert(1, 'b', (0, 2));
        assert!(squashed);
        assert_eq!(h.undo.len(), 1);
        let Op::Delete { char_len, .. } = h.undo.back().unwrap().to_undo else {
            panic!()
        };
        assert_eq!(char_len, 2);
    }

    #[test]
    fn no_squash_when_non_adjacent() {
        let mut h = History::new(100);
        h.push(ins(0, "a"));
        let squashed = h.try_squash_insert(5, 'b', (0, 6)); // gap!
        assert!(!squashed);
    }

    #[test]
    fn max_depth_evicts_oldest() {
        let mut h = History::new(3);
        h.push(ins(0, "a"));
        h.push(ins(1, "b"));
        h.push(ins(2, "c"));
        h.push(ins(3, "d")); // evicts "a"
        assert_eq!(h.undo.len(), 3);
        // The oldest remaining should be "b" at pos 1.
        let Op::Insert { pos, .. } = &h.undo.front().unwrap().to_redo else {
            panic!()
        };
        assert_eq!(*pos, 1);
    }
}
