//! The in-memory workbook.

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Bound;
use std::sync::Arc;

use crate::address::{CellRange, CellRef};

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

/// A sparse worksheet that remembers which cells were edited since the last save. Copies share
/// their cells until one of them changes, so a snapshot of millions of cells costs next to nothing.
#[derive(Debug, Clone, Default)]
pub struct Sheet {
    pub name: String,
    cells: Arc<BTreeMap<CellRef, Cell>>,
    /// How many cells each column holds, so the extent needn't look at every cell.
    column_counts: BTreeMap<u32, u32>,
    dirty: BTreeSet<CellRef>,
    locked: Vec<CellRange>,
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

    /// The populated cells ordered by position.
    pub fn cells(&self) -> impl Iterator<Item = (&CellRef, &Cell)> {
        self.cells.iter()
    }

    pub fn has_cell(&self, at: CellRef) -> bool {
        self.cells.contains_key(&at)
    }

    /// The populated cells of a rectangle, in order, without looking at the rows outside it.
    pub fn cells_in(&self, range: CellRange) -> impl Iterator<Item = (&CellRef, &Cell)> {
        self.cells
            .range(range.first..=range.last)
            .filter(move |(at, _)| range.contains(**at))
    }

    /// Stores a cell read from a file, without marking it as edited.
    pub fn load_cell(&mut self, at: CellRef, cell: Cell) {
        if cell.is_blank() {
            if Arc::make_mut(&mut self.cells).remove(&at).is_some() {
                self.forget_column_cell(at.col);
            }
        } else if Arc::make_mut(&mut self.cells).insert(at, cell).is_none() {
            *self.column_counts.entry(at.col).or_default() += 1;
        }
    }

    fn forget_column_cell(&mut self, col: u32) {
        if let Some(count) = self.column_counts.get_mut(&col) {
            *count -= 1;
            if *count == 0 {
                self.column_counts.remove(&col);
            }
        }
    }

    /// Stores a user edit.
    pub fn set_cell(&mut self, at: CellRef, cell: Cell) {
        self.load_cell(at, cell);
        self.dirty.insert(at);
    }

    /// Marks a region that Excel only allows editing as a whole, such as an array formula.
    pub fn lock_range(&mut self, first: CellRef, last: CellRef) {
        self.locked.push(CellRange::between(first, last));
    }

    pub fn is_locked(&self, at: CellRef) -> bool {
        self.locked.iter().any(|range| range.contains(at))
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
        // Cells are ordered by row first, so the last one is on the last row.
        let row = self.cells.last_key_value()?.0.row;
        let col = *self.column_counts.last_key_value()?.0;
        Some(CellRef { col, row })
    }

    /// Every populated cell in reading order starting just past `from`, or backwards starting just
    /// before it, wrapping around the sheet and ending with `from` itself if it's populated.
    pub fn cells_around(
        &self,
        from: CellRef,
        forward: bool,
    ) -> Box<dyn Iterator<Item = (&CellRef, &Cell)> + '_> {
        if forward {
            let after = self.cells.range((Bound::Excluded(from), Bound::Unbounded));
            Box::new(after.chain(self.cells.range(..=from)))
        } else {
            let before = self.cells.range(..from).rev();
            Box::new(before.chain(self.cells.range(from..).rev()))
        }
    }
}
