//! The workbook open in the app, and the rules for changing it.

use std::fmt;

use crate::address::CellRef;
use crate::changes::{History, Locked};
use crate::model::{Cell, Sheet};
use crate::xlsx::Workbook;

/// Why something that was asked of the document isn't happening.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    Locked(CellRef),
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Locked(at) => Locked { at: *at }.fmt(f),
        }
    }
}

/// Where the user is in a document.
#[derive(Debug)]
pub struct Place {
    pub cursor: CellRef,
    /// The cell shown in the grid's top-left corner.
    pub scroll: CellRef,
    /// Where the visual selection started.
    pub anchor: CellRef,
    sheet: usize,
    /// The cursor position left behind on each sheet.
    cursors: Vec<CellRef>,
}

impl Place {
    fn start(sheets: usize) -> Self {
        let origin = CellRef { col: 0, row: 0 };
        Self {
            cursor: origin,
            scroll: origin,
            anchor: origin,
            sheet: 0,
            cursors: vec![origin; sheets],
        }
    }
}

/// A workbook, the changes made to it, and where the user is in it.
#[derive(Debug)]
pub struct Document {
    pub place: Place,
    workbook: Workbook,
    history: History,
}

impl Document {
    pub fn new(workbook: Workbook) -> Self {
        Self {
            place: Place::start(workbook.sheets.len()),
            workbook,
            history: History::default(),
        }
    }

    pub fn workbook(&self) -> &Workbook {
        &self.workbook
    }

    pub fn sheet_index(&self) -> usize {
        self.place.sheet
    }

    pub fn current_sheet(&self) -> &Sheet {
        &self.workbook.sheets[self.place.sheet]
    }

    /// Moves between sheets, wrapping around. Each one remembers its own cursor.
    pub fn switch_sheet(&mut self, offset: isize) {
        let count = self.workbook.sheets.len() as isize;
        let next = (self.place.sheet as isize + offset).rem_euclid(count) as usize;
        self.show_sheet(next);
    }

    fn show_sheet(&mut self, sheet: usize) {
        self.place.cursors[self.place.sheet] = self.place.cursor;
        self.place.sheet = sheet;
        self.place.cursor = self.place.cursors[sheet];
    }

    /// Changes cells of the sheet on show as one undoable step, or none of them if it's refused.
    pub fn edit(&mut self, cells: Vec<(CellRef, Cell)>) -> Result<(), Refusal> {
        self.history
            .apply(&mut self.workbook.sheets, self.place.sheet, cells)
            .map_err(|locked| Refusal::Locked(locked.at))
    }

    /// Takes back the latest change and shows where it was. Says whether there was one.
    pub fn undo(&mut self) -> Result<bool, Refusal> {
        let restored = self.history.undo(&mut self.workbook.sheets);
        Ok(self.show_restored(restored))
    }

    /// Makes the latest undone change again and shows where it was. Says whether there was one.
    pub fn redo(&mut self) -> Result<bool, Refusal> {
        let restored = self.history.redo(&mut self.workbook.sheets);
        Ok(self.show_restored(restored))
    }

    fn show_restored(&mut self, restored: Option<crate::changes::Restored>) -> bool {
        let Some(restored) = restored else {
            return false;
        };
        self.place.cursors[self.place.sheet] = self.place.cursor;
        self.place.sheet = restored.sheet;
        if let Some(first) = restored.first {
            self.place.cursor = first;
        }
        true
    }
}
