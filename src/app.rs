//! Application state: everything the keyboard can change.

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::address::{CellRef, MAX_COLS, MAX_ROWS};
use crate::model::{Cell, Sheet};
use crate::text::{edit_text, parse_input};
use crate::xlsx::Workbook;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Navigate,
    Edit,
    /// Waiting for the user to decide what happens to unsaved edits.
    ConfirmQuit,
}

/// Plain characters, plus AltGr combinations, which Windows reports as Ctrl+Alt.
fn is_text_input(key: KeyEvent) -> bool {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    ctrl == alt
}

/// One reversible cell edit.
struct Change {
    sheet: usize,
    at: CellRef,
    before: Cell,
    after: Cell,
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
    /// The cell as it was when editing began.
    original: Cell,
    status: String,
    undo: Vec<Change>,
    redo: Vec<Change>,
    should_quit: bool,
    /// The cursor position left behind on each sheet.
    cursors: Vec<CellRef>,
}

impl App {
    pub fn new(workbook: Workbook) -> Self {
        let origin = CellRef { col: 0, row: 0 };
        Self {
            cursors: vec![origin; workbook.sheets.len()],
            workbook,
            sheet: 0,
            cursor: CellRef { col: 0, row: 0 }, viewport: (10, 20),
            mode: Mode::Navigate,
            buffer: Vec::new(),
            caret: 0,
            original: Cell::default(),
            status: String::new(),
            undo: Vec::new(),
            redo: Vec::new(),
            should_quit: false,
        }
    }

    pub fn cursor(&self) -> CellRef {
        self.cursor
    }

    pub fn sheet_index(&self) -> usize {
        self.sheet
    }

    pub fn current_sheet(&self) -> &Sheet {
        &self.workbook.sheets[self.sheet]
    }

    pub fn set_viewport(&mut self, cols: u32, rows: u32) {
        self.viewport = (cols.max(1), rows.max(1));
    }

    /// The latest message for the user.
    pub fn status(&self) -> &str {
        &self.status
    }

    pub fn should_quit(&self) -> bool {
        self.should_quit
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
        self.status.clear();
        match self.mode {
            Mode::Navigate => self.navigate_key(key),
            Mode::Edit => self.edit_key(key),
            Mode::ConfirmQuit => self.confirm_quit_key(key),
        }
    }

    fn navigate_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        match key.code {
            KeyCode::Char(c) if is_text_input(key) => {
                self.begin_edit(vec![c]);
            }
            KeyCode::PageDown if ctrl => self.switch_sheet(1),
            KeyCode::PageUp if ctrl => self.switch_sheet(-1),
            KeyCode::F(6) if shift => self.switch_sheet(-1),
            KeyCode::F(6) => self.switch_sheet(1),
            KeyCode::Char('q' | 'c') if ctrl => {
                if self.workbook.is_dirty() {
                    self.mode = Mode::ConfirmQuit;
                } else {
                    self.should_quit = true;
                }
            }
            KeyCode::Char('s') if ctrl => {
                self.save();
            }
            KeyCode::Char('z') if ctrl => self.undo(),
            KeyCode::Char('y') if ctrl => self.redo(),
            KeyCode::Delete | KeyCode::Backspace => {
                self.apply_edit(self.cursor, Cell::default());
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

    /// Saves and reports the outcome; returns whether the file is now up to date.
    fn save(&mut self) -> bool {
        match self.workbook.save() {
            Ok(()) => {
                self.status = format!("Saved {}", self.file_name());
                true
            }
            Err(error) => {
                self.status = format!("Save failed: {error}");
                false
            }
        }
    }

    pub fn file_name(&self) -> String {
        let path = self.workbook.path();
        path.file_name().unwrap_or(path.as_os_str()).to_string_lossy().into_owned()
    }

    fn confirm_quit_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('s' | 'S') => {
                self.mode = Mode::Navigate;
                self.should_quit = self.save();
            }
            KeyCode::Char('d' | 'D') => self.should_quit = true,
            KeyCode::Esc => self.mode = Mode::Navigate,
            _ => {}
        }
    }

    /// Moves between sheets, each of which remembers its own cursor.
    fn switch_sheet(&mut self, offset: isize) {
        let count = self.workbook.sheets.len() as isize;
        let next = (self.sheet as isize + offset).rem_euclid(count) as usize;
        self.cursors[self.sheet] = self.cursor;
        self.sheet = next;
        self.cursor = self.cursors[next];
    }

    fn undo(&mut self) {
        let Some(change) = self.undo.pop() else {
            self.status = "Nothing to undo".into();
            return;
        };
        self.restore(change.sheet, change.at, change.before.clone());
        self.redo.push(change);
    }

    fn redo(&mut self) {
        let Some(change) = self.redo.pop() else {
            self.status = "Nothing to redo".into();
            return;
        };
        self.restore(change.sheet, change.at, change.after.clone());
        self.undo.push(change);
    }

    /// Writes a cell back and shows it to the user.
    fn restore(&mut self, sheet: usize, at: CellRef, cell: Cell) {
        self.workbook.sheets[sheet].set_cell(at, cell);
        self.cursors[self.sheet] = self.cursor;
        self.sheet = sheet;
        self.cursor = at;
    }

    fn begin_edit(&mut self, initial: Vec<char>) {
        if self.refuse_if_locked() {
            return;
        }
        self.original = self.current_sheet().cell(self.cursor);
        self.caret = initial.len();
        self.buffer = initial;
        self.mode = Mode::Edit;
    }

    fn edit_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => self.mode = Mode::Navigate,
            KeyCode::Enter | KeyCode::Down => self.commit_and_move(0, 1),
            KeyCode::Up => self.commit_and_move(0, -1),
            KeyCode::Tab => self.commit_and_move(1, 0),
            KeyCode::BackTab => self.commit_and_move(-1, 0),
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

    fn refuse_if_locked(&mut self) -> bool {
        let locked = self.current_sheet().is_locked(self.cursor);
        if locked {
            self.status = format!("{} is part of an array formula and is read-only", self.cursor);
        }
        locked
    }

    fn apply_edit(&mut self, at: CellRef, cell: Cell) {
        if self.refuse_if_locked() {
            return;
        }
        let before = self.current_sheet().cell(at);
        if before != cell {
            self.workbook.sheets[self.sheet].set_cell(at, cell.clone());
            self.undo.push(Change { sheet: self.sheet, at, before, after: cell });
            self.redo.clear();
        }
    }

    fn commit_and_move(&mut self, cols: i64, rows: i64) {
        self.commit_edit();
        self.move_by(cols, rows);
    }

    fn commit_edit(&mut self) {
        self.mode = Mode::Navigate;
        let at = self.cursor;
        let text = self.buffer();
        let was_date = self.current_sheet().cell(at).is_date;
        let date1904 = self.workbook.date1904;
        // Confirming unchanged text must not disturb the cell, cached formula results included.
        if text == edit_text(&self.original, date1904) {
            return;
        }
        let cell = parse_input(&text, was_date, date1904);
        self.apply_edit(at, cell);
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
