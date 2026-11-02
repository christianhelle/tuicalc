//! Application state: everything the keyboard can change.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::address::{CellRef, MAX_COLS, MAX_ROWS};
use crate::file_dialog::{DialogAction, DialogKind, FileDialog, with_default_extension};
use crate::formula::translate;
use crate::model::{Cell, CellValue, Sheet};
use crate::text::{edit_text, parse_input};
use crate::xlsx::{Workbook, XlsxError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Insert,
    /// Selecting a rectangle of cells.
    Visual,
    /// Selecting whole rows.
    VisualRow,
    /// Typing a `:` command.
    Command,
    /// Waiting for the user to decide what happens to unsaved edits.
    ConfirmDiscard,
    /// Choosing where to save a workbook that has no file yet.
    FileDialog,
}

/// Plain characters, plus AltGr combinations, which Windows reports as Ctrl+Alt.
pub(crate) fn is_text_input(key: KeyEvent) -> bool {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    ctrl == alt
}

/// Ctrl alone, not the Ctrl+Alt that Windows reports for AltGr.
pub(crate) fn is_ctrl(key: KeyEvent) -> bool {
    key.modifiers.contains(KeyModifiers::CONTROL) && !key.modifiers.contains(KeyModifiers::ALT)
}

/// What to do once unsaved edits have been saved or discarded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NextStep {
    Quit,
    OpenFile,
}

/// One cell's part in a change.
struct CellEdit {
    at: CellRef,
    before: Cell,
    after: Cell,
}

/// What one command changed, undone and redone as a unit.
struct Change {
    sheet: usize,
    edits: Vec<CellEdit>,
}

/// Cells kept by a yank or a delete, for pasting.
struct Register {
    first: CellRef,
    last: CellRef,
    /// Whether whole rows were taken.
    rows: bool,
    cells: Vec<(CellRef, Cell)>,
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
    status_is_error: bool,
    undo: Vec<Change>,
    redo: Vec<Change>,
    should_quit: bool,
    /// The cursor position left behind on each sheet.
    cursors: Vec<CellRef>,
    scroll: CellRef,
    /// The count typed so far, as in `5j`.
    count: Option<u32>,
    /// The first key of a two-key command, as in `gg`.
    pending: Option<char>,
    /// Where the visual selection started.
    anchor: CellRef,
    register: Option<Register>,
    /// The operator waiting for its motion, and where it started.
    operator: Option<(char, CellRef)>,
    operator_count: Option<u32>,
    file_dialog: Option<FileDialog>,
    /// Where the file dialog starts.
    dialog_directory: PathBuf,
    /// What's waiting on the unsaved edits being saved or discarded.
    next_step: Option<NextStep>,
}

impl App {
    pub fn new(workbook: Workbook) -> Self {
        let origin = CellRef { col: 0, row: 0 };
        Self {
            cursors: vec![origin; workbook.sheets.len()],
            scroll: origin,
            count: None,
            pending: None,
            anchor: origin,
            register: None,
            operator: None,
            operator_count: None,
            file_dialog: None,
            next_step: None,
            dialog_directory: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            workbook,
            sheet: 0,
            cursor: CellRef { col: 0, row: 0 },
            viewport: (10, 20),
            mode: Mode::Normal,
            buffer: Vec::new(),
            caret: 0,
            original: Cell::default(),
            status: String::new(),
            status_is_error: false,
            undo: Vec::new(),
            redo: Vec::new(),
            should_quit: false,
        }
    }

    pub fn set_dialog_directory(&mut self, directory: &Path) {
        self.dialog_directory = directory.to_path_buf();
    }

    /// The file dialog, while it's open.
    pub fn file_dialog(&self) -> Option<&FileDialog> {
        self.file_dialog.as_ref()
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
        self.scroll_to_cursor();
    }

    /// The keys of a command still being typed, as vim's `showcmd` shows them.
    pub fn pending_keys(&self) -> String {
        let count = self
            .operator_count
            .or(self.count)
            .map_or(String::new(), |n| n.to_string());
        let operator = self
            .operator
            .map_or(String::new(), |(operator, _)| operator.to_string());
        let inner = match (self.operator_count, self.count) {
            (Some(_), Some(n)) => n.to_string(),
            _ => String::new(),
        };
        let prefix = self
            .pending
            .map_or(String::new(), |prefix| prefix.to_string());
        format!("{count}{operator}{inner}{prefix}")
    }

    /// The latest message for the user.
    pub fn status(&self) -> &str {
        &self.status
    }

    /// Whether the latest message reports something going wrong.
    pub fn status_is_error(&self) -> bool {
        self.status_is_error
    }

    fn report_error(&mut self, message: String) {
        self.status = message;
        self.status_is_error = true;
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

    /// The cell shown in the grid's top-left corner.
    pub fn scroll(&self) -> CellRef {
        self.scroll
    }

    /// Adjusts the scroll position just enough to show the cursor.
    fn scroll_to_cursor(&mut self) {
        let (cols, rows) = self.viewport;
        if self.cursor.col < self.scroll.col {
            self.scroll.col = self.cursor.col;
        } else if self.cursor.col >= self.scroll.col + cols {
            self.scroll.col = self.cursor.col + 1 - cols;
        }
        if self.cursor.row < self.scroll.row {
            self.scroll.row = self.cursor.row;
        } else if self.cursor.row >= self.scroll.row + rows {
            self.scroll.row = self.cursor.row + 1 - rows;
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        // Windows reports key releases as separate events.
        if key.kind == KeyEventKind::Release {
            return;
        }
        self.status.clear();
        self.status_is_error = false;
        match self.mode {
            Mode::Normal => self.normal_key(key),
            Mode::Insert => self.insert_key(key),
            Mode::Visual | Mode::VisualRow => self.visual_key(key),
            Mode::Command => self.command_key(key),
            Mode::ConfirmDiscard => self.confirm_discard_key(key),
            Mode::FileDialog => self.file_dialog_key(key),
        }
        self.scroll_to_cursor();
    }

    /// The populated columns of the cursor's row, from left to right.
    fn filled_columns(&self) -> impl Iterator<Item = u32> + '_ {
        let row = self.cursor.row;
        let last = self.current_sheet().extent().map_or(0, |extent| extent.col);
        (0..=last).filter(move |&col| self.is_filled(CellRef { col, row }))
    }

    fn first_filled_column(&self) -> Option<u32> {
        self.filled_columns().next()
    }

    fn last_filled_column(&self) -> Option<u32> {
        self.filled_columns().last()
    }

    fn is_filled(&self, at: CellRef) -> bool {
        self.current_sheet().has_cell(at)
    }

    fn repeat_motion(&mut self, times: i64, step: fn(&Self) -> Option<CellRef>) {
        for _ in 0..times {
            match step(self) {
                Some(next) => self.cursor = next,
                None => break,
            }
        }
    }

    /// The last column worth searching on this sheet.
    fn last_column(&self) -> u32 {
        self.current_sheet().extent().map_or(0, |extent| extent.col)
    }

    fn filled_in_row(&self, col: u32) -> bool {
        self.is_filled(CellRef {
            col,
            row: self.cursor.row,
        })
    }

    /// `w`: the first cell of the next run of filled cells in this row.
    fn next_run_start(&self) -> Option<CellRef> {
        let last = self.last_column();
        let mut col = self.cursor.col;
        while col <= last && self.filled_in_row(col) {
            col += 1;
        }
        while col <= last && !self.filled_in_row(col) {
            col += 1;
        }
        (col <= last).then_some(CellRef {
            col,
            row: self.cursor.row,
        })
    }

    /// `e`: the last cell of this run, or of the next one when already at the end.
    fn next_run_end(&self) -> Option<CellRef> {
        let last = self.last_column();
        let mut col = self.cursor.col + 1;
        while col <= last && !self.filled_in_row(col) {
            col += 1;
        }
        if col > last {
            return None;
        }
        while col < last && self.filled_in_row(col + 1) {
            col += 1;
        }
        Some(CellRef {
            col,
            row: self.cursor.row,
        })
    }

    /// `b`: the first cell of this run, or of the previous one when already at the start.
    fn previous_run_start(&self) -> Option<CellRef> {
        let mut col = self.cursor.col;
        while col > 0 && !self.filled_in_row(col - 1) {
            col -= 1;
        }
        if col == 0 {
            return None;
        }
        col -= 1;
        while col > 0 && self.filled_in_row(col - 1) {
            col -= 1;
        }
        Some(CellRef {
            col,
            row: self.cursor.row,
        })
    }

    fn filled_in_column(&self, row: u32) -> bool {
        self.is_filled(CellRef {
            col: self.cursor.col,
            row,
        })
    }

    /// `}`: the empty cell below the next run of filled cells in this column.
    fn next_gap(&self) -> Option<CellRef> {
        let last = self.current_sheet().extent()?.row;
        let mut row = self.cursor.row;
        while row <= last && !self.filled_in_column(row) {
            row += 1;
        }
        if row > last {
            return None;
        }
        while row <= last && self.filled_in_column(row) {
            row += 1;
        }
        Some(CellRef {
            col: self.cursor.col,
            row,
        })
    }

    /// `{`: the empty cell above the previous run of filled cells, or the top of the sheet.
    fn previous_gap(&self) -> Option<CellRef> {
        let mut row = self.cursor.row;
        if row == 0 {
            return None;
        }
        while row > 0 && !self.filled_in_column(row) {
            row -= 1;
        }
        while row > 0 && self.filled_in_column(row) {
            row -= 1;
        }
        Some(CellRef {
            col: self.cursor.col,
            row,
        })
    }

    /// Moves the view and the cursor by the same number of rows.
    fn scroll_with_cursor(&mut self, rows: i64) {
        self.move_by(0, rows);
        let top = i64::from(self.scroll.row) + rows;
        self.scroll.row = top.clamp(0, i64::from(MAX_ROWS) - 1) as u32;
    }

    /// Moves the cursor if the key is a motion, and says whether it was.
    fn motion(&mut self, key: KeyEvent, count: Option<u32>) -> bool {
        let ctrl = is_ctrl(key);
        let n = i64::from(count.unwrap_or(1));
        match key.code {
            KeyCode::Char('0') if !ctrl => self.cursor.col = 0,
            KeyCode::Char('|') if !ctrl => {
                self.cursor.col = count.unwrap_or(1).saturating_sub(1).min(MAX_COLS - 1);
            }
            KeyCode::Char('^') if !ctrl => {
                self.cursor.col = self.first_filled_column().unwrap_or(0);
            }
            KeyCode::Char('$') if !ctrl => {
                if let Some(col) = self.last_filled_column() {
                    self.cursor.col = col;
                }
            }
            KeyCode::Char('G') if !ctrl => {
                let last = self.current_sheet().extent().map_or(0, |extent| extent.row);
                self.cursor.row = count.map_or(last, |row| row.saturating_sub(1));
            }
            KeyCode::Char('w') if !ctrl => self.repeat_motion(n, Self::next_run_start),
            KeyCode::Char('e') if !ctrl => self.repeat_motion(n, Self::next_run_end),
            KeyCode::Char('b') if !ctrl => self.repeat_motion(n, Self::previous_run_start),
            KeyCode::Char('}') if !ctrl => self.repeat_motion(n, Self::next_gap),
            KeyCode::Char('{') if !ctrl => self.repeat_motion(n, Self::previous_gap),
            KeyCode::Char('H') if !ctrl => {
                let offset = count
                    .unwrap_or(1)
                    .saturating_sub(1)
                    .min(self.viewport.1 - 1);
                self.cursor.row = self.scroll.row + offset;
            }
            KeyCode::Char('M') if !ctrl => {
                self.cursor.row = self.scroll.row + (self.viewport.1 - 1) / 2
            }
            KeyCode::Char('L') if !ctrl => {
                let offset = count
                    .unwrap_or(1)
                    .saturating_sub(1)
                    .min(self.viewport.1 - 1);
                self.cursor.row = self.scroll.row + self.viewport.1 - 1 - offset;
            }
            KeyCode::Char('d') if ctrl => {
                self.scroll_with_cursor(n * i64::from(self.viewport.1 / 2))
            }
            KeyCode::Char('u') if ctrl => {
                self.scroll_with_cursor(-n * i64::from(self.viewport.1 / 2))
            }
            KeyCode::Char('f') if ctrl => self.scroll_with_cursor(n * i64::from(self.viewport.1)),
            KeyCode::Char('b') if ctrl => self.scroll_with_cursor(-n * i64::from(self.viewport.1)),
            KeyCode::Char('h') if !ctrl => self.move_by(-n, 0),
            KeyCode::Char('l') if !ctrl => self.move_by(n, 0),
            KeyCode::Char('k') if !ctrl => self.move_by(0, -n),
            KeyCode::Char('j') if !ctrl => self.move_by(0, n),
            KeyCode::Left => self.move_by(-n, 0),
            KeyCode::Right => self.move_by(n, 0),
            KeyCode::Up => self.move_by(0, -n),
            KeyCode::Down => self.move_by(0, n),
            KeyCode::Tab => self.move_by(n, 0),
            KeyCode::BackTab => self.move_by(-n, 0),
            KeyCode::Enter => self.move_by(0, n),
            KeyCode::PageDown if !ctrl => self.move_by(0, n * i64::from(self.viewport.1)),
            KeyCode::PageUp if !ctrl => self.move_by(0, -n * i64::from(self.viewport.1)),
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
            _ => return false,
        }
        true
    }

    /// Collects count digits, and says whether the key was one.
    fn count_digit(&mut self, key: KeyEvent) -> bool {
        let KeyCode::Char(c @ '0'..='9') = key.code else {
            return false;
        };
        // A leading zero is the motion to the first column, not a count.
        if c == '0' && self.count.is_none() {
            return false;
        }
        let digit = c.to_digit(10).unwrap_or(0);
        let count = self
            .count
            .unwrap_or(0)
            .saturating_mul(10)
            .saturating_add(digit);
        self.count = Some(count.min(MAX_ROWS));
        true
    }

    fn begin_visual(&mut self, mode: Mode) {
        self.anchor = self.cursor;
        self.mode = mode;
    }

    /// The selected rectangle, as its top-left and bottom-right cells.
    pub fn selection(&self) -> Option<(CellRef, CellRef)> {
        let (a, b) = (self.anchor, self.cursor);
        let rows = (a.row.min(b.row), a.row.max(b.row));
        match self.mode {
            Mode::Visual => Some((
                CellRef {
                    col: a.col.min(b.col),
                    row: rows.0,
                },
                CellRef {
                    col: a.col.max(b.col),
                    row: rows.1,
                },
            )),
            Mode::VisualRow => Some((
                CellRef {
                    col: 0,
                    row: rows.0,
                },
                CellRef {
                    col: MAX_COLS - 1,
                    row: rows.1,
                },
            )),
            _ => None,
        }
    }

    /// Completes `d`, `y` or `c` with the motion that just moved the cursor. Exclusive motions
    /// stop short of where they landed, so `dw` keeps the next run and `dl` takes one cell.
    fn finish_operator(&mut self, exclusive: bool) {
        let Some((operator, origin)) = self.operator.take() else {
            return;
        };
        let target = self.cursor;
        let (first, mut last) = (
            CellRef {
                col: origin.col.min(target.col),
                row: origin.row.min(target.row),
            },
            CellRef {
                col: origin.col.max(target.col),
                row: origin.row.max(target.row),
            },
        );
        if exclusive && target.col != origin.col {
            last.col -= 1;
        }
        self.cursor = origin;
        self.operate(operator, first, last, false, first);
    }

    /// `dd`, `yy` and `cc`: the operator on whole rows, starting at the cursor's.
    fn operate_on_rows(&mut self, operator: char, count: u32) {
        let row = self.cursor.row;
        let bottom = row.saturating_add(count - 1).min(MAX_ROWS - 1);
        let first = CellRef { col: 0, row };
        let last = CellRef {
            col: MAX_COLS - 1,
            row: bottom,
        };
        self.operate(operator, first, last, true, self.cursor);
    }

    fn operate_on_selection(&mut self, operator: char) {
        let Some((first, last)) = self.selection() else {
            return;
        };
        let rows = self.mode == Mode::VisualRow;
        self.mode = Mode::Normal;
        let at = if rows {
            CellRef {
                col: self.cursor.col,
                row: first.row,
            }
        } else {
            first
        };
        self.operate(operator, first, last, rows, at);
    }

    /// Runs `d`, `y` or `c` on a rectangle, leaving the cursor at `at`.
    fn operate(&mut self, operator: char, first: CellRef, last: CellRef, rows: bool, at: CellRef) {
        let yanked = self.yank(first, last, rows);
        if operator != 'y' && !self.clear_cells(first, last) {
            return;
        }
        self.cursor = at;
        match operator {
            'y' => self.status = format!("{yanked} cells yanked"),
            'c' => self.begin_edit(Vec::new()),
            _ => {}
        }
    }

    /// Copies a rectangle into the register, and says how many cells had contents.
    fn yank(&mut self, first: CellRef, last: CellRef, rows: bool) -> usize {
        let cells: Vec<(CellRef, Cell)> = self
            .filled_cells(first, last)
            .into_iter()
            .map(|at| (at, self.current_sheet().cell(at)))
            .collect();
        let count = cells.len();
        self.register = Some(Register {
            first,
            last,
            rows,
            cells,
        });
        count
    }

    /// `p`: writes the register at the cursor, or below it for whole rows. `P` puts rows here.
    fn paste(&mut self, before: bool) {
        let Some(register) = self.register.take() else {
            self.status = "Nothing to paste".into();
            return;
        };
        let target = match (register.rows, before) {
            (true, true) => CellRef {
                col: 0,
                row: self.cursor.row,
            },
            (true, false) => CellRef {
                col: 0,
                row: (self.cursor.row + 1).min(MAX_ROWS - 1),
            },
            (false, _) => self.cursor,
        };
        let cols = i64::from(target.col) - i64::from(register.first.col);
        let rows = i64::from(target.row) - i64::from(register.first.row);
        let shifted = |at: CellRef| {
            let col = i64::from(at.col) + cols;
            let row = i64::from(at.row) + rows;
            (col < i64::from(MAX_COLS) && row < i64::from(MAX_ROWS)).then_some(CellRef {
                col: col as u32,
                row: row as u32,
            })
        };
        let bottom_right = CellRef {
            col: (i64::from(register.last.col) + cols).min(i64::from(MAX_COLS) - 1) as u32,
            row: (i64::from(register.last.row) + rows).min(i64::from(MAX_ROWS) - 1) as u32,
        };
        // Everything the block lands on is replaced, including by its empty cells.
        let mut edits: BTreeMap<CellRef, Cell> = self
            .filled_cells(target, bottom_right)
            .into_iter()
            .map(|at| (at, Cell::default()))
            .collect();
        for (at, cell) in &register.cells {
            let Some(to) = shifted(*at) else { continue };
            let is_date = self.current_sheet().cell(to).is_date;
            let pasted = match &cell.formula {
                Some(formula) => Cell::formula(translate(formula, cols, rows)),
                None => Cell {
                    is_date: is_date && matches!(cell.value, CellValue::Number(_)),
                    ..cell.clone()
                },
            };
            edits.insert(to, pasted);
        }
        if self.apply_edits(edits.into_iter().collect()) {
            self.cursor = CellRef {
                col: self.cursor.col,
                row: target.row,
            };
            if !register.rows {
                self.cursor = target;
            }
        }
        self.register = Some(register);
    }

    fn visual_key(&mut self, key: KeyEvent) {
        if self.count_digit(key) {
            return;
        }
        let count = self.count.take();
        if let Some(prefix) = self.pending.take() {
            self.prefixed_key(prefix, key, count);
            return;
        }
        if let (KeyCode::Char(prefix @ ('g' | 'z')), false) = (key.code, is_ctrl(key)) {
            self.pending = Some(prefix);
            self.count = count;
            return;
        }
        if self.motion(key, count) {
            return;
        }
        match (key.code, self.mode) {
            (KeyCode::Esc, _)
            | (KeyCode::Char('v'), Mode::Visual)
            | (KeyCode::Char('V'), Mode::VisualRow) => {
                self.mode = Mode::Normal;
            }
            (KeyCode::Char('c'), _) if is_ctrl(key) => self.mode = Mode::Normal,
            (KeyCode::Char('v'), _) => self.mode = Mode::Visual,
            (KeyCode::Char('V'), _) => self.mode = Mode::VisualRow,
            (KeyCode::Char('o' | 'O'), _) => std::mem::swap(&mut self.anchor, &mut self.cursor),
            (KeyCode::Char('d' | 'x') | KeyCode::Delete, _) => self.operate_on_selection('d'),
            (KeyCode::Char('y'), _) => self.operate_on_selection('y'),
            (KeyCode::Char('c' | 's'), _) => self.operate_on_selection('c'),
            _ => {}
        }
    }

    /// Finishes a two-key command.
    fn prefixed_key(&mut self, prefix: char, key: KeyEvent, count: Option<u32>) -> bool {
        let is_motion = prefix == 'g' && key.code == KeyCode::Char('g');
        let rows = self.viewport.1;
        match (prefix, key.code) {
            ('g', KeyCode::Char('g')) => {
                self.cursor.row = count.unwrap_or(1).saturating_sub(1).min(MAX_ROWS - 1);
            }
            ('g', KeyCode::Char('t' | 'T')) if self.selection().is_some() => {}
            ('g', KeyCode::Char('t')) => match count {
                Some(number) => self.go_to_sheet(number),
                None => self.switch_sheet(1),
            },
            ('g', KeyCode::Char('T')) => self.switch_sheet(-1),
            ('Z', KeyCode::Char('Z')) => self.save_if_edited_and_quit(),
            ('Z', KeyCode::Char('Q')) => self.should_quit = true,
            ('z', KeyCode::Char('z')) => self.scroll.row = self.cursor.row.saturating_sub(rows / 2),
            ('z', KeyCode::Char('t')) => self.scroll.row = self.cursor.row,
            ('z', KeyCode::Char('b')) => {
                self.scroll.row = (self.cursor.row + 1).saturating_sub(rows)
            }
            _ => {}
        }
        is_motion
    }

    fn normal_key(&mut self, key: KeyEvent) {
        if self.count_digit(key) {
            return;
        }
        let count = self.count.take();
        let count = match (count, self.operator_count.take()) {
            (Some(a), Some(b)) => Some(a.saturating_mul(b).min(MAX_ROWS)),
            (a, b) => a.or(b),
        };
        if let Some(prefix) = self.pending.take() {
            if self.prefixed_key(prefix, key, count) {
                self.finish_operator(false);
            } else {
                self.operator = None;
            }
            return;
        }
        if let (KeyCode::Char(prefix @ ('g' | 'z' | 'Z')), false) = (key.code, is_ctrl(key)) {
            self.pending = Some(prefix);
            self.count = count;
            return;
        }
        if self.motion(key, count) {
            let exclusive = matches!(
                key.code,
                KeyCode::Char('h' | 'l' | 'w' | 'b') | KeyCode::Left | KeyCode::Right
            );
            let backward = matches!(key.code, KeyCode::Char('h' | 'b') | KeyCode::Left);
            if backward
                && self
                    .operator
                    .is_some_and(|(_, origin)| origin == self.cursor)
            {
                self.operator = None;
                return;
            }
            self.finish_operator(exclusive);
            return;
        }
        if let Some((operator, _)) = self.operator.take() {
            if key.code == KeyCode::Char(operator) {
                self.operate_on_rows(operator, count.unwrap_or(1));
            }
            return;
        }
        if let (KeyCode::Char(operator @ ('d' | 'y' | 'c')), false) = (key.code, is_ctrl(key)) {
            self.operator = Some((operator, self.cursor));
            self.operator_count = count;
            return;
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        match key.code {
            KeyCode::PageDown if ctrl => self.switch_sheet(1),
            KeyCode::PageUp if ctrl => self.switch_sheet(-1),
            KeyCode::F(6) if shift => self.switch_sheet(-1),
            KeyCode::F(6) => self.switch_sheet(1),
            KeyCode::Char('q' | 'c') if ctrl => {
                self.ask_before_losing_edits(NextStep::Quit);
            }
            KeyCode::Char('q') => self.ask_before_losing_edits(NextStep::Quit),
            KeyCode::Char('s') if ctrl => {
                self.save();
            }
            KeyCode::Char('o') if ctrl => self.choose_file_to_open(),
            KeyCode::Char('z') if ctrl => self.undo(),
            KeyCode::Char('y') if ctrl => self.redo(),
            KeyCode::Delete | KeyCode::Backspace => {
                self.apply_edit(self.cursor, Cell::default());
            }
            KeyCode::Char('p') if !ctrl => self.paste(false),
            KeyCode::Char('P') if !ctrl => self.paste(true),
            KeyCode::Char('u') if !ctrl => self.undo(),
            KeyCode::Char('r') if ctrl => self.redo(),
            KeyCode::Char('x') if !ctrl => {
                let n = count.unwrap_or(1) - 1;
                let last = CellRef {
                    col: (self.cursor.col + n).min(MAX_COLS - 1),
                    ..self.cursor
                };
                self.clear_cells(self.cursor, last);
            }
            KeyCode::Char('X') if !ctrl => {
                let n = count.unwrap_or(1);
                if self.cursor.col > 0 {
                    let first = CellRef {
                        col: self.cursor.col.saturating_sub(n),
                        ..self.cursor
                    };
                    let last = CellRef {
                        col: self.cursor.col - 1,
                        ..self.cursor
                    };
                    if self.clear_cells(first, last) {
                        self.cursor = first;
                    }
                }
            }
            KeyCode::Char('D') if !ctrl => {
                if let Some(col) = self
                    .last_filled_column()
                    .filter(|col| *col >= self.cursor.col)
                {
                    self.clear_cells(self.cursor, CellRef { col, ..self.cursor });
                }
            }
            KeyCode::Char('v') if is_ctrl(key) => self.begin_visual(Mode::Visual),
            KeyCode::Char('v') if !ctrl => self.begin_visual(Mode::Visual),
            KeyCode::Char('V') if !ctrl => self.begin_visual(Mode::VisualRow),
            KeyCode::Char(':') if !ctrl => {
                self.buffer.clear();
                self.caret = 0;
                self.mode = Mode::Command;
            }
            KeyCode::F(2) => self.edit_existing(),
            KeyCode::Char('a' | 'A') if !ctrl => self.edit_existing(),
            KeyCode::Char('i' | 'I') if !ctrl => {
                self.edit_existing();
                self.caret = 0;
            }
            KeyCode::Char('s') if !ctrl => self.begin_edit(Vec::new()),
            KeyCode::Char('S') if !ctrl => self.operate_on_rows('c', count.unwrap_or(1)),
            KeyCode::Char('C') if !ctrl => {
                match self
                    .last_filled_column()
                    .filter(|col| *col >= self.cursor.col)
                {
                    Some(col) => {
                        let last = CellRef { col, ..self.cursor };
                        self.operate('c', self.cursor, last, false, self.cursor);
                    }
                    None => self.begin_edit(Vec::new()),
                }
            }
            KeyCode::Char('o') if !ctrl => {
                self.move_by(0, 1);
                self.edit_existing();
            }
            KeyCode::Char('O') if !ctrl => {
                self.move_by(0, -1);
                self.edit_existing();
            }
            KeyCode::Char('=') if !ctrl => self.begin_edit(vec!['=']),
            _ => {}
        }
    }

    /// `ZZ` and `:x`: as `:wq`, except that a blank workbook with no edits has nothing to save.
    fn save_if_edited_and_quit(&mut self) {
        if self.workbook.path().is_none() && !self.workbook.is_dirty() {
            self.should_quit = true;
        } else {
            self.save_and_quit();
        }
    }

    /// Saves and quits, leaving the quit to the file dialog if the file needs a name.
    fn save_and_quit(&mut self) {
        self.next_step = self.workbook.path().is_none().then_some(NextStep::Quit);
        self.should_quit = self.save();
    }

    /// Saves and reports the outcome; returns whether the file is now up to date.
    fn save(&mut self) -> bool {
        if self.workbook.path().is_none() {
            self.open_file_dialog(DialogKind::Save);
            return false;
        }
        match self.workbook.save() {
            Ok(()) => {
                self.status = format!("Saved {}", self.file_name());
                true
            }
            Err(error) => {
                self.report_error(format!("Save failed: {error}"));
                false
            }
        }
    }

    fn open_file_dialog(&mut self, kind: DialogKind) {
        self.file_dialog = Some(FileDialog::new(kind, &self.dialog_directory));
        self.mode = Mode::FileDialog;
    }

    fn file_dialog_key(&mut self, key: KeyEvent) {
        let Some(dialog) = &mut self.file_dialog else {
            return;
        };
        match dialog.handle_key(key) {
            DialogAction::Stay => {}
            DialogAction::Cancel => {
                self.next_step = None;
                self.close_file_dialog();
            }
            DialogAction::Save(path) => {
                let result = self.workbook.save_as(path);
                self.finish_dialog_save(result);
            }
            DialogAction::Overwrite(path) => {
                let result = self.workbook.save_over(path);
                self.finish_dialog_save(result);
            }
            DialogAction::Open(path) => match Workbook::open(path) {
                Ok(workbook) => {
                    self.close_file_dialog();
                    self.load_workbook(workbook);
                }
                Err(error) => {
                    if let Some(dialog) = &mut self.file_dialog {
                        dialog.set_message(format!("Open failed: {error}"));
                    }
                }
            },
        }
    }

    /// Closes the dialog after a save, or reports why it failed and leaves the dialog open.
    fn finish_dialog_save(&mut self, result: Result<(), XlsxError>) {
        match result {
            Ok(()) => {
                self.close_file_dialog();
                self.status = format!("Saved {}", self.file_name());
                if let Some(step) = self.next_step.take() {
                    self.take_step(step);
                }
            }
            Err(error) => {
                if let Some(dialog) = &mut self.file_dialog {
                    dialog.set_message(format!("Save failed: {error}"));
                }
            }
        }
    }

    fn close_file_dialog(&mut self) {
        self.file_dialog = None;
        self.mode = Mode::Normal;
    }

    /// Says why a command that would lose unsaved edits isn't running; returns whether it isn't.
    fn refuse_if_dirty(&mut self) -> bool {
        let dirty = self.workbook.is_dirty();
        if dirty {
            self.status = "E37: No write since last change (add ! to override)".into();
        }
        dirty
    }

    /// `:e` and `Ctrl+O`.
    fn choose_file_to_open(&mut self) {
        if !self.refuse_if_dirty() {
            self.open_file_dialog(DialogKind::Open);
        }
    }

    fn open_named(&mut self, name: &str) {
        match Workbook::open(with_default_extension(name)) {
            Ok(workbook) => self.load_workbook(workbook),
            Err(error) => self.report_error(format!("Open failed: {error}")),
        }
    }

    /// Replaces the workbook, keeping only the register, so cells can be pasted across files.
    fn load_workbook(&mut self, workbook: Workbook) {
        let origin = CellRef { col: 0, row: 0 };
        self.cursors = vec![origin; workbook.sheets.len()];
        self.workbook = workbook;
        self.sheet = 0;
        self.cursor = origin;
        self.scroll = origin;
        self.anchor = origin;
        self.undo.clear();
        self.redo.clear();
        self.status = format!("Opened {}", self.file_name());
    }

    /// Saves to a new file and reports the outcome; returns whether it worked.
    fn save_named(&mut self, name: &str) -> bool {
        if self.workbook.path().is_some() {
            self.status = "Save As isn't supported yet".into();
            return false;
        }
        match self.workbook.save_as(with_default_extension(name)) {
            Ok(()) => {
                self.status = format!("Saved {}", self.file_name());
                true
            }
            Err(error) => {
                self.report_error(format!("Save failed: {error}"));
                false
            }
        }
    }

    pub fn file_name(&self) -> String {
        let Some(path) = self.workbook.path() else {
            return "[No Name]".into();
        };
        path.file_name()
            .unwrap_or(path.as_os_str())
            .to_string_lossy()
            .into_owned()
    }

    /// What's waiting on the unsaved edits being saved or discarded.
    pub fn next_step(&self) -> Option<NextStep> {
        self.next_step
    }

    /// Takes the step now, or asks first if it would lose unsaved edits.
    fn ask_before_losing_edits(&mut self, step: NextStep) {
        if self.workbook.is_dirty() {
            self.next_step = Some(step);
            self.mode = Mode::ConfirmDiscard;
        } else {
            self.take_step(step);
        }
    }

    fn take_step(&mut self, step: NextStep) {
        match step {
            NextStep::Quit => self.should_quit = true,
            NextStep::OpenFile => self.open_file_dialog(DialogKind::Open),
        }
    }

    fn confirm_discard_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('s' | 'S') => {
                self.mode = Mode::Normal;
                // Without a file name the save dialog takes over, and takes the step afterwards.
                if self.save() {
                    if let Some(step) = self.next_step.take() {
                        self.take_step(step);
                    }
                } else if self.file_dialog.is_none() {
                    self.next_step = None;
                }
            }
            KeyCode::Char('d' | 'D') => {
                self.mode = Mode::Normal;
                if let Some(step) = self.next_step.take() {
                    self.take_step(step);
                }
            }
            _ => {
                self.mode = Mode::Normal;
                self.next_step = None;
            }
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

    /// `{n}gt`: shows a sheet by its 1-based position.
    fn go_to_sheet(&mut self, number: u32) {
        let index = number.saturating_sub(1) as isize;
        if (index as usize) < self.workbook.sheets.len() {
            self.switch_sheet(index - self.sheet as isize);
        }
    }

    fn undo(&mut self) {
        let Some(change) = self.undo.pop() else {
            self.status = "Nothing to undo".into();
            return;
        };
        let cells = change
            .edits
            .iter()
            .map(|edit| (edit.at, edit.before.clone()))
            .collect();
        self.restore(change.sheet, cells);
        self.redo.push(change);
    }

    fn redo(&mut self) {
        let Some(change) = self.redo.pop() else {
            self.status = "Nothing to redo".into();
            return;
        };
        let cells = change
            .edits
            .iter()
            .map(|edit| (edit.at, edit.after.clone()))
            .collect();
        self.restore(change.sheet, cells);
        self.undo.push(change);
    }

    /// Writes cells back and shows the first of them to the user.
    fn restore(&mut self, sheet: usize, cells: Vec<(CellRef, Cell)>) {
        let first = cells.first().map(|(at, _)| *at);
        for (at, cell) in cells {
            self.workbook.sheets[sheet].set_cell(at, cell);
        }
        self.cursors[self.sheet] = self.cursor;
        self.sheet = sheet;
        if let Some(first) = first {
            self.cursor = first;
        }
    }

    fn edit_existing(&mut self) {
        let cell = self.current_sheet().cell(self.cursor);
        let text = edit_text(&cell, self.workbook.date1904);
        self.begin_edit(text.chars().collect());
    }

    fn command_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => self.mode = Mode::Normal,
            KeyCode::Char('c') if is_ctrl(key) => self.mode = Mode::Normal,
            KeyCode::Enter => {
                self.mode = Mode::Normal;
                let command = self.buffer();
                self.run_command(command.trim());
            }
            KeyCode::Backspace if self.caret == 0 => self.mode = Mode::Normal,
            KeyCode::Backspace => {
                self.caret -= 1;
                self.buffer.remove(self.caret);
            }
            KeyCode::Char(c) if is_text_input(key) => {
                self.buffer.insert(self.caret, c);
                self.caret += 1;
            }
            _ => {}
        }
    }

    fn run_command(&mut self, command: &str) {
        match command.split_once(' ') {
            Some(("w", name)) => {
                self.save_named(name.trim());
                return;
            }
            Some(("wq" | "x", name)) => {
                self.should_quit = self.save_named(name.trim());
                return;
            }
            Some(("e", name)) => {
                if !self.refuse_if_dirty() {
                    self.open_named(name.trim());
                }
                return;
            }
            Some(("e!", name)) => {
                self.open_named(name.trim());
                return;
            }
            _ => {}
        }
        match command {
            "" => {}
            "w" => {
                self.save();
            }
            "q" if self.refuse_if_dirty() => {}
            "e" => self.choose_file_to_open(),
            "e!" => self.open_file_dialog(DialogKind::Open),
            "q" | "q!" => self.should_quit = true,
            "wq" => self.save_and_quit(),
            "x" => self.save_if_edited_and_quit(),
            _ => {
                if let Ok(row) = command.parse::<u32>() {
                    self.cursor.row = row.saturating_sub(1).min(MAX_ROWS - 1);
                } else if let Some(at) = CellRef::parse(command) {
                    self.cursor = at;
                } else {
                    self.status = format!("E492: Not an editor command: {command}");
                }
            }
        }
    }

    fn begin_edit(&mut self, initial: Vec<char>) {
        if self.refuse_if_locked() {
            return;
        }
        self.original = self.current_sheet().cell(self.cursor);
        self.caret = initial.len();
        self.buffer = initial;
        self.mode = Mode::Insert;
    }

    fn insert_key(&mut self, key: KeyEvent) {
        match key.code {
            // As in vim, Esc keeps what was typed and Ctrl-C is the way out without it.
            KeyCode::Esc => self.commit_edit(),
            KeyCode::Char('c') if is_ctrl(key) => self.mode = Mode::Normal,
            KeyCode::Char('u') if is_ctrl(key) => {
                self.buffer.drain(..self.caret);
                self.caret = 0;
            }
            KeyCode::Char('w') if is_ctrl(key) => {
                let mut start = self.caret;
                while start > 0 && self.buffer[start - 1].is_whitespace() {
                    start -= 1;
                }
                while start > 0 && !self.buffer[start - 1].is_whitespace() {
                    start -= 1;
                }
                self.buffer.drain(start..self.caret);
                self.caret = start;
            }
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
            self.report_error(format!(
                "{} is part of an array formula and is read-only",
                self.cursor
            ));
        }
        locked
    }

    fn apply_edit(&mut self, at: CellRef, cell: Cell) {
        self.apply_edits(vec![(at, cell)]);
    }

    /// Applies edits as one undoable change, or none of them if any cell is read-only.
    fn apply_edits(&mut self, cells: Vec<(CellRef, Cell)>) -> bool {
        if let Some((at, _)) = cells
            .iter()
            .find(|(at, _)| self.current_sheet().is_locked(*at))
        {
            self.report_error(format!("{at} is part of an array formula and is read-only"));
            return false;
        }
        let mut edits = Vec::new();
        for (at, after) in cells {
            let before = self.current_sheet().cell(at);
            if before != after {
                self.workbook.sheets[self.sheet].set_cell(at, after.clone());
                edits.push(CellEdit { at, before, after });
            }
        }
        if !edits.is_empty() {
            self.undo.push(Change {
                sheet: self.sheet,
                edits,
            });
            self.redo.clear();
        }
        true
    }

    /// The populated cells of a rectangle.
    fn filled_cells(&self, first: CellRef, last: CellRef) -> Vec<CellRef> {
        self.current_sheet()
            .cells()
            .map(|(at, _)| *at)
            .filter(|at| {
                (first.col..=last.col).contains(&at.col) && (first.row..=last.row).contains(&at.row)
            })
            .collect()
    }

    fn clear_cells(&mut self, first: CellRef, last: CellRef) -> bool {
        let cleared = self.filled_cells(first, last).into_iter();
        self.apply_edits(cleared.map(|at| (at, Cell::default())).collect())
    }

    fn commit_and_move(&mut self, cols: i64, rows: i64) {
        self.commit_edit();
        self.move_by(cols, rows);
    }

    fn commit_edit(&mut self) {
        self.mode = Mode::Normal;
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
