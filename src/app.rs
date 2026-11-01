//! Application state: everything the keyboard can change.

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind};

use crate::address::{CellRef, MAX_COLS, MAX_ROWS};
use crate::model::Sheet;
use crate::xlsx::Workbook;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Navigate,
}

pub struct App {
    pub workbook: Workbook,
    sheet: usize,
    cursor: CellRef,
}

impl App {
    pub fn new(workbook: Workbook) -> Self {
        Self { workbook, sheet: 0, cursor: CellRef { col: 0, row: 0 } }
    }

    pub fn cursor(&self) -> CellRef {
        self.cursor
    }

    pub fn current_sheet(&self) -> &Sheet {
        &self.workbook.sheets[self.sheet]
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        // Windows reports key releases as separate events.
        if key.kind == KeyEventKind::Release {
            return;
        }
        self.navigate_key(key);
    }

    fn navigate_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Left => self.move_by(-1, 0),
            KeyCode::Right => self.move_by(1, 0),
            KeyCode::Up => self.move_by(0, -1),
            KeyCode::Down => self.move_by(0, 1),
            _ => {}
        }
    }

    fn move_by(&mut self, cols: i64, rows: i64) {
        let col = i64::from(self.cursor.col) + cols;
        let row = i64::from(self.cursor.row) + rows;
        self.cursor = CellRef {
            col: col.clamp(0, i64::from(MAX_COLS) - 1) as u32,
            row: row.clamp(0, i64::from(MAX_ROWS) - 1) as u32,
        };
    }
}
