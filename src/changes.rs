//! Changes to cells: applying them as a unit, taking them back, and moving cells with a register.

use std::collections::BTreeMap;
use std::fmt;

use crate::address::{CellRange, CellRef, MAX_COLS, MAX_ROWS};
use crate::formula::translate;
use crate::model::{Cell, CellValue, Sheet};

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

/// Cells kept by a yank or a delete, for pasting.
#[derive(Debug)]
pub struct Register {
    range: CellRange,
    /// Whether whole rows were taken.
    rows: bool,
    cells: Vec<(CellRef, Cell)>,
}

/// What a paste would write, and where the cursor goes if it's allowed.
#[derive(Debug, PartialEq)]
pub struct Paste {
    pub cells: Vec<(CellRef, Cell)>,
    pub cursor: CellRef,
}

impl Register {
    /// Copies a rectangle of a sheet.
    pub fn yank(sheet: &Sheet, range: CellRange, rows: bool) -> Self {
        Self {
            range,
            rows,
            cells: sheet
                .cells_in(range)
                .map(|(at, cell)| (*at, cell.clone()))
                .collect(),
        }
    }

    /// How many cells with contents were taken.
    pub fn len(&self) -> usize {
        self.cells.len()
    }

    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }

    /// Works out a paste at the cursor. Whole rows go below its row, or onto it with `before`.
    pub fn paste(&self, sheet: &Sheet, cursor: CellRef, before: bool) -> Paste {
        let target = match (self.rows, before) {
            (true, true) => CellRef {
                col: 0,
                row: cursor.row,
            },
            (true, false) => CellRef {
                col: 0,
                row: (cursor.row + 1).min(MAX_ROWS - 1),
            },
            (false, _) => cursor,
        };
        let cols = i64::from(target.col) - i64::from(self.range.first.col);
        let rows = i64::from(target.row) - i64::from(self.range.first.row);
        let shifted = |at: CellRef| {
            let col = i64::from(at.col) + cols;
            let row = i64::from(at.row) + rows;
            (col < i64::from(MAX_COLS) && row < i64::from(MAX_ROWS)).then_some(CellRef {
                col: col as u32,
                row: row as u32,
            })
        };
        let bottom_right = CellRef {
            col: (i64::from(self.range.last.col) + cols).min(i64::from(MAX_COLS) - 1) as u32,
            row: (i64::from(self.range.last.row) + rows).min(i64::from(MAX_ROWS) - 1) as u32,
        };
        // Everything the block lands on is replaced, including by its empty cells.
        let landing = CellRange {
            first: target,
            last: bottom_right,
        };
        let mut edits: BTreeMap<CellRef, Cell> = sheet
            .cells_in(landing)
            .map(|(at, _)| (*at, Cell::default()))
            .collect();
        for (at, cell) in &self.cells {
            let Some(to) = shifted(*at) else { continue };
            let is_date = sheet.cell(to).is_date;
            let pasted = match &cell.formula {
                Some(formula) => Cell::formula(translate(formula, cols, rows)),
                None => Cell {
                    is_date: is_date && matches!(cell.value, CellValue::Number(_)),
                    ..cell.clone()
                },
            };
            edits.insert(to, pasted);
        }
        Paste {
            cells: edits.into_iter().collect(),
            cursor: if self.rows {
                CellRef {
                    col: cursor.col,
                    row: target.row,
                }
            } else {
                target
            },
        }
    }
}
