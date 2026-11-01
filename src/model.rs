//! The in-memory workbook.

use std::collections::{BTreeMap, BTreeSet};

use crate::address::CellRef;

#[derive(Debug, Clone, PartialEq, Default)]
pub enum CellValue {
    #[default]
    Empty,
    Number(f64),
    Text(String),
    Bool(bool),
    /// An error literal such as `#DIV/0!`.
    Error(String),
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Cell {
    /// The literal value, or the cached result when `formula` is set.
    pub value: CellValue,
    /// Formula text without the leading `=`.
    pub formula: Option<String>,
    /// Whether the cell's number format shows its number as a date.
    pub is_date: bool,
}

impl Cell {
    pub fn value(value: CellValue) -> Self {
        Self {
            value,
            ..Self::default()
        }
    }

    pub fn formula(text: impl Into<String>) -> Self {
        Self {
            formula: Some(text.into()),
            ..Self::default()
        }
    }

    fn is_blank(&self) -> bool {
        self.value == CellValue::Empty && self.formula.is_none()
    }
}

/// A sparse worksheet that remembers which cells were edited since the last save.
#[derive(Debug, Clone, Default)]
pub struct Sheet {
    pub name: String,
    cells: BTreeMap<CellRef, Cell>,
    dirty: BTreeSet<CellRef>,
    locked: Vec<(CellRef, CellRef)>,
}

impl Sheet {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ..Self::default()
        }
    }

    pub fn cell(&self, at: CellRef) -> Cell {
        self.cells.get(&at).cloned().unwrap_or_default()
    }

    /// Stores a cell read from a file, without marking it as edited.
    pub fn load_cell(&mut self, at: CellRef, cell: Cell) {
        if cell.is_blank() {
            self.cells.remove(&at);
        } else {
            self.cells.insert(at, cell);
        }
    }

    /// Stores a user edit.
    pub fn set_cell(&mut self, at: CellRef, cell: Cell) {
        self.load_cell(at, cell);
        self.dirty.insert(at);
    }

    /// Marks a region that Excel only allows editing as a whole, such as an array formula.
    pub fn lock_range(&mut self, first: CellRef, last: CellRef) {
        self.locked.push((first, last));
    }

    pub fn is_locked(&self, at: CellRef) -> bool {
        self.locked.iter().any(|(first, last)| {
            (first.col..=last.col).contains(&at.col) && (first.row..=last.row).contains(&at.row)
        })
    }

    pub fn has_formulas(&self) -> bool {
        self.cells.values().any(|cell| cell.formula.is_some())
    }

    pub fn is_dirty(&self) -> bool {
        !self.dirty.is_empty()
    }

    pub fn dirty_cells(&self) -> impl Iterator<Item = &CellRef> {
        self.dirty.iter()
    }

    pub fn mark_saved(&mut self) {
        self.dirty.clear();
    }

    /// The bottom-right corner of the populated area.
    pub fn extent(&self) -> Option<CellRef> {
        let row = self.cells.keys().map(|at| at.row).max()?;
        let col = self.cells.keys().map(|at| at.col).max()?;
        Some(CellRef { col, row })
    }
}
