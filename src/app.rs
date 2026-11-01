//! Application state: everything the keyboard can change.

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::address::{CellRef, MAX_COLS, MAX_ROWS};
use crate::model::Sheet;
use crate::text::{edit_text, parse_input};
use crate::xlsx::Workbook;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Navigate,
    Edit,
}

/// Plain characters, plus AltGr combinations, which Windows reports as Ctrl+Alt.
fn is_text_input(key: KeyEvent) -> bool {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    ctrl == alt
}

pub struct App {
    pub workbook: Workbook,
    sheet: usize,
    cursor: CellRef,
    /// The visible grid size, in columns and rows.
    viewport: (u32, u32),
    mode: Mode,
    buffer: Vec<char>,
    caret: usize,
}

impl App {
    pub fn new(workbook: Workbook) -> Self {
        Self {
            workbook,
            sheet: 0,
            cursor: CellRef { col: 0, row: 0 }, viewport: (10, 20),
            mode: Mode::Navigate,
            buffer: Vec::new(),
            caret: 0,
        }
    }

    pub fn cursor(&self) -> CellRef {
        self.cursor
    }

    pub fn current_sheet(&self) -> &Sheet {
        &self.workbook.sheets[self.sheet]
    }

    pub fn set_viewport(&mut self, cols: u32, rows: u32) {
        self.viewport = (cols.max(1), rows.max(1));
    }

    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// The caret position within the edited text, in characters.
    pub fn caret(&self) -> usize {
        self.caret
    }

    /// The text being edited.
    pub fn buffer(&self) -> String {
        self.buffer.iter().collect()
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        // Windows reports key releases as separate events.
        if key.kind == KeyEventKind::Release {
            return;
        }
        match self.mode {
            Mode::Navigate => self.navigate_key(key),
            Mode::Edit => self.edit_key(key),
        }
    }

    fn navigate_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Char(c) if is_text_input(key) => {
                self.begin_edit(vec![c]);
            }
            KeyCode::F(2) => {
                let cell = self.current_sheet().cell(self.cursor);
                let text = edit_text(&cell, self.workbook.date1904);
                self.begin_edit(text.chars().collect());
            }
            KeyCode::Left => self.move_by(-1, 0),
            KeyCode::Right => self.move_by(1, 0),
            KeyCode::Up => self.move_by(0, -1),
            KeyCode::Down => self.move_by(0, 1),
            KeyCode::Tab => self.move_by(1, 0),
            KeyCode::BackTab => self.move_by(-1, 0),
            KeyCode::Enter => self.move_by(0, 1),
            KeyCode::PageDown => self.move_by(0, i64::from(self.viewport.1)),
            KeyCode::PageUp => self.move_by(0, -i64::from(self.viewport.1)),
            KeyCode::Home if ctrl => self.cursor = CellRef { col: 0, row: 0 },
            KeyCode::Home => self.cursor.col = 0,
            KeyCode::End if ctrl => {
                if let Some(extent) = self.current_sheet().extent() {
                    self.cursor = extent;
                }
            }
            KeyCode::End => {
                if let Some(extent) = self.current_sheet().extent() {
                    self.cursor.col = extent.col;
                }
            }
            _ => {}
        }
    }

    fn begin_edit(&mut self, initial: Vec<char>) {
        self.caret = initial.len();
        self.buffer = initial;
        self.mode = Mode::Edit;
    }

    fn edit_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Enter => {
                self.commit_edit();
                self.move_by(0, 1);
            }
            KeyCode::Char(c) if is_text_input(key) => {
                self.buffer.insert(self.caret, c);
                self.caret += 1;
            }
            KeyCode::Left => self.caret = self.caret.saturating_sub(1),
            KeyCode::Right => self.caret = (self.caret + 1).min(self.buffer.len()),
            KeyCode::Home => self.caret = 0,
            KeyCode::End => self.caret = self.buffer.len(),
            KeyCode::Backspace if self.caret > 0 => {
                self.caret -= 1;
                self.buffer.remove(self.caret);
            }
            KeyCode::Delete if self.caret < self.buffer.len() => {
                self.buffer.remove(self.caret);
            }
            _ => {}
        }
    }

    fn commit_edit(&mut self) {
        self.mode = Mode::Navigate;
        let at = self.cursor;
        let text = self.buffer();
        let was_date = self.current_sheet().cell(at).is_date;
        let cell = parse_input(&text, was_date, self.workbook.date1904);
        self.workbook.sheets[self.sheet].set_cell(at, cell);
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
