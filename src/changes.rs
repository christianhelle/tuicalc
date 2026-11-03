//! Changes to cells: applying them as a unit, taking them back, and moving cells with a register.

use std::fmt;

use crate::address::CellRef;
use crate::model::{Cell, Sheet};

/// A change refused because it touches a cell that can't be edited on its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Locked {
    pub at: CellRef,
}

impl fmt::Display for Locked {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} is part of an array formula and is read-only",
            self.at
        )
    }
}

/// One cell's part in a change.
#[derive(Debug)]
struct CellEdit {
    at: CellRef,
    before: Cell,
    after: Cell,
}

/// What one command changed, undone and redone as a unit.
#[derive(Debug)]
struct Change {
    sheet: usize,
    edits: Vec<CellEdit>,
}

/// Where an undo or redo happened, for showing it to the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Restored {
    pub sheet: usize,
    pub first: Option<CellRef>,
}

/// The changes that can be undone and redone.
#[derive(Debug, Default)]
pub struct History {
    undo: Vec<Change>,
    redo: Vec<Change>,
}

impl History {
    /// Applies edits to a sheet as one undoable change, or none of them if any cell is locked.
    pub fn apply(
        &mut self,
        sheets: &mut [Sheet],
        sheet: usize,
        cells: Vec<(CellRef, Cell)>,
    ) -> Result<(), Locked> {
        let target = &mut sheets[sheet];
        if let Some((at, _)) = cells.iter().find(|(at, _)| target.is_locked(*at)) {
            return Err(Locked { at: *at });
        }
        let mut edits = Vec::new();
        for (at, after) in cells {
            let before = target.cell(at);
            if before != after {
                target.set_cell(at, after.clone());
                edits.push(CellEdit { at, before, after });
            }
        }
        if !edits.is_empty() {
            self.undo.push(Change { sheet, edits });
            self.redo.clear();
        }
        Ok(())
    }

    /// Takes back the latest change, if there is one.
    pub fn undo(&mut self, sheets: &mut [Sheet]) -> Option<Restored> {
        let change = self.undo.pop()?;
        let restored = restore(sheets, &change, |edit| &edit.before);
        self.redo.push(change);
        Some(restored)
    }

    /// Makes the latest undone change again, if there is one.
    pub fn redo(&mut self, sheets: &mut [Sheet]) -> Option<Restored> {
        let change = self.redo.pop()?;
        let restored = restore(sheets, &change, |edit| &edit.after);
        self.undo.push(change);
        Some(restored)
    }
}

fn restore(sheets: &mut [Sheet], change: &Change, side: fn(&CellEdit) -> &Cell) -> Restored {
    for edit in &change.edits {
        sheets[change.sheet].set_cell(edit.at, side(edit).clone());
    }
    Restored {
        sheet: change.sheet,
        first: change.edits.first().map(|edit| edit.at),
    }
}
