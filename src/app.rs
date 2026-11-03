//! Application state: everything the keyboard can change.

use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::address::{CellRange, CellRef, MAX_COLS, MAX_ROWS};
use crate::changes::{History, Locked, Register, Restored};
use crate::file_dialog::{DialogAction, DialogKind, FileDialog};
use crate::help::{self, HelpTable};
use crate::keys::is_ctrl;
use crate::line_edit::LineEdit;
use crate::model::{Cell, Sheet};
use crate::text::{cell_contains, edit_text, parse_input};
use crate::xlsx::{
    LoadEvent, SavedPackage, Waited, Workbook, WorkbookLoad, WorkbookSave, XlsxError,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Insert,
    /// Selecting a rectangle of cells.
    Visual,
    /// Selecting whole rows.
    VisualRow,
    /// Typing the cell or row to jump to.
    GoTo,
    /// Typing the text to look for.
    Search,
    /// Showing the keys of the mode it was opened from.
    Help,
    /// Waiting for the user to decide what happens to unsaved edits.
    ConfirmDiscard,
    /// Choosing where to save a workbook that has no file yet.
    FileDialog,
}

/// What to do once unsaved edits have been saved or discarded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NextStep {
    Quit,
    OpenFile,
    Reload,
}

/// A workbook on screen whose cells are still arriving.
struct Loading {
    load: WorkbookLoad,
    kind: LoadKind,
    /// What to go back to if the load doesn't work out. The first workbook has nothing to go to.
    previous: Option<Box<PreviousBook>>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum LoadKind {
    Open,
    Reload,
}

/// A workbook put aside while another loads, and the place the user was in it.
struct PreviousBook {
    workbook: Workbook,
    sheet: usize,
    cursor: CellRef,
    scroll: CellRef,
    anchor: CellRef,
    cursors: Vec<CellRef>,
    history: History,
}

/// Freeing a large workbook takes a noticeable moment, so it's done out of the user's way.
fn free_later<T: Send + 'static>(value: T) {
    thread::spawn(move || drop(value));
}

pub struct App {
    pub workbook: Workbook,
    sheet: usize,
    cursor: CellRef,
    /// The visible grid size, in columns and rows.
    viewport: (u32, u32),
    mode: Mode,
    /// The text being typed into a cell or a prompt.
    line: LineEdit,
    /// The cell as it was when editing began.
    original: Cell,
    status: String,
    status_is_error: bool,
    history: History,
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
    /// The latest search, which `n` and `N` repeat.
    search_query: Option<String>,
    /// The mode to go back to when the help closes.
    help_from: Mode,
    loading: Option<Loading>,
    /// The save on its way to the disk.
    saving: Option<WorkbookSave>,
    /// Why the workbook the app opened with couldn't be read, once it has given up.
    load_failure: Option<String>,
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
            search_query: None,
            help_from: Mode::Normal,
            loading: None,
            saving: None,
            load_failure: None,
            dialog_directory: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            workbook,
            sheet: 0,
            cursor: CellRef { col: 0, row: 0 },
            viewport: (10, 20),
            mode: Mode::Normal,
            line: LineEdit::default(),
            original: Cell::default(),
            status: String::new(),
            status_is_error: false,
            history: History::default(),
            should_quit: false,
        }
    }

    /// An app showing a workbook whose cells are on their way.
    pub fn opening(workbook: Workbook, load: WorkbookLoad) -> Self {
        let mut app = Self::new(workbook);
        app.loading = Some(Loading {
            load,
            kind: LoadKind::Open,
            previous: None,
        });
        app
    }

    pub fn is_loading(&self) -> bool {
        self.loading.is_some()
    }

    /// How much of the workbook has been read, from 0 to 1, while it's loading.
    pub fn loading_progress(&self) -> Option<f64> {
        self.loading.as_ref().map(|loading| loading.load.progress())
    }

    pub fn is_saving(&self) -> bool {
        self.saving.is_some()
    }

    /// Whether something is happening that the screen should keep up with without a key press.
    pub fn is_busy(&self) -> bool {
        self.is_loading() || self.is_saving()
    }

    /// Spends up to `budget` on the save or load in progress, stopping short if there's no news.
    pub fn absorb_background(&mut self, budget: Duration) {
        if let Some(save) = &self.saving {
            if let Some(result) = save.result_within(budget) {
                self.end_save(result);
            }
        } else {
            self.absorb_loaded(budget);
        }
    }

    /// Waits for the save in progress.
    pub fn finish_saving(&mut self) {
        if let Some(save) = &self.saving {
            let result = save.wait();
            self.end_save(result);
        }
    }

    /// Takes in the outcome of a save, and takes whatever step was waiting on it.
    fn end_save(&mut self, result: Result<SavedPackage, XlsxError>) {
        self.saving = None;
        match result {
            Ok(saved) => {
                self.workbook.finish_save(saved);
                if self.file_dialog.is_some() {
                    self.close_file_dialog();
                }
                self.status = format!("Saved {}", self.file_name());
                self.status_is_error = false;
                if let Some(step) = self.next_step.take() {
                    self.take_step(step);
                }
            }
            // The dialog stays up to try another name, with the step still waiting.
            Err(error) => match &mut self.file_dialog {
                Some(dialog) => dialog.set_message(format!("Save failed: {error}")),
                None => {
                    self.next_step = None;
                    self.report_error(format!("Save failed: {error}"));
                }
            },
        }
    }

    pub fn load_failure(&self) -> Option<&str> {
        self.load_failure.as_deref()
    }

    /// Takes in arriving cells for up to `budget`, which stops short if there are none yet.
    pub fn absorb_loaded(&mut self, budget: Duration) {
        let deadline = Instant::now() + budget;
        while let Some(loading) = &self.loading {
            let wait = deadline.saturating_duration_since(Instant::now());
            match loading.load.next_within(wait) {
                Waited::Event(event) => self.take_load_event(event),
                Waited::Ended => self.finish_load(),
                Waited::Nothing => return,
            }
            if Instant::now() >= deadline {
                return;
            }
        }
    }

    /// Waits for the next chunk of cells, or for the load to end.
    pub fn absorb_next_chunk(&mut self) {
        if let Some(loading) = &self.loading {
            match loading.load.next_blocking() {
                Some(event) => self.take_load_event(event),
                None => self.finish_load(),
            }
        }
    }

    /// Waits for the whole workbook.
    pub fn finish_loading(&mut self) {
        while self.loading.is_some() {
            self.absorb_next_chunk();
        }
    }

    fn take_load_event(&mut self, event: LoadEvent) {
        match event {
            LoadEvent::Cells { sheet, chunk } => chunk.apply_to(&mut self.workbook.sheets[sheet]),
            LoadEvent::Failed(error) => {
                let Some(loading) = self.loading.take() else {
                    return;
                };
                let verb = match loading.kind {
                    LoadKind::Open => "Open",
                    LoadKind::Reload => "Reload",
                };
                match loading.previous {
                    Some(previous) => {
                        self.go_back_to(*previous);
                        self.report_error(format!("{verb} failed: {error}"));
                    }
                    None => {
                        self.load_failure = Some(error.to_string());
                        self.should_quit = true;
                    }
                }
            }
        }
    }

    fn finish_load(&mut self) {
        let Some(loading) = self.loading.take() else {
            return;
        };
        if let Some(previous) = loading.previous {
            free_later(previous);
            let verb = match loading.kind {
                LoadKind::Open => "Opened",
                LoadKind::Reload => "Reloaded",
            };
            self.status = format!("{verb} {}", self.file_name());
        }
    }

    /// Puts another workbook on screen with its cells on their way, keeping this one until they
    /// have all arrived.
    fn begin_load(&mut self, workbook: Workbook, load: WorkbookLoad, kind: LoadKind) {
        let origin = CellRef { col: 0, row: 0 };
        let previous = PreviousBook {
            sheet: self.sheet,
            cursor: self.cursor,
            scroll: self.scroll,
            anchor: self.anchor,
            cursors: std::mem::replace(&mut self.cursors, vec![origin; workbook.sheets.len()]),
            history: std::mem::take(&mut self.history),
            workbook: std::mem::replace(&mut self.workbook, workbook),
        };
        self.sheet = 0;
        self.cursor = origin;
        self.scroll = origin;
        self.anchor = origin;
        self.loading = Some(Loading {
            load,
            kind,
            previous: Some(Box::new(previous)),
        });
    }

    fn go_back_to(&mut self, previous: PreviousBook) {
        free_later(std::mem::replace(&mut self.workbook, previous.workbook));
        self.sheet = previous.sheet;
        self.cursor = previous.cursor;
        self.scroll = previous.scroll;
        self.anchor = previous.anchor;
        self.cursors = previous.cursors;
        self.history = previous.history;
        self.mode = Mode::Normal;
        self.help_from = Mode::Normal;
    }

    /// Says why something that needs the whole workbook isn't happening; returns whether it isn't.
    fn refuse_while_loading(&mut self) -> bool {
        let loading = self.is_loading();
        if loading {
            self.status = "Still opening: Esc cancels".into();
        }
        loading
    }

    /// Says why something that changes the workbook, or needs it whole, isn't happening; returns
    /// whether it isn't. A save is writing a snapshot, and taking in its outcome marks every cell
    /// as saved, so a change made meanwhile would be lost.
    fn refuse_while_busy(&mut self) -> bool {
        if self.is_saving() {
            self.status = "Still saving".into();
            return true;
        }
        self.refuse_while_loading()
    }

    /// Stops loading at the user's request.
    fn cancel_load(&mut self) {
        let Some(loading) = self.loading.take() else {
            return;
        };
        match loading.previous {
            Some(previous) => {
                self.go_back_to(*previous);
                self.status = "Stopped opening".into();
            }
            None => self.should_quit = true,
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

    /// The text being typed into a cell or a prompt.
    pub fn line(&self) -> &LineEdit {
        &self.line
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
            Mode::GoTo => self.go_to_key(key),
            Mode::Search => self.search_key(key),
            Mode::Help => self.mode = self.help_from,
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
    pub fn selection(&self) -> Option<CellRange> {
        let corners = CellRange::between(self.anchor, self.cursor);
        match self.mode {
            Mode::Visual => Some(corners),
            Mode::VisualRow => Some(CellRange {
                first: CellRef {
                    col: 0,
                    ..corners.first
                },
                last: CellRef {
                    col: MAX_COLS - 1,
                    ..corners.last
                },
            }),
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
        let Some(CellRange { first, last }) = self.selection() else {
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
        let refused = if operator == 'y' {
            self.refuse_while_loading()
        } else {
            self.refuse_while_busy()
        };
        if refused {
            return;
        }
        let range = CellRange { first, last };
        let register = Register::yank(self.current_sheet(), range, rows);
        let yanked = register.len();
        self.register = Some(register);
        if operator != 'y' && !self.clear_cells(first, last) {
            return;
        }
        self.cursor = at;
        match operator {
            'y' => self.status = format!("{yanked} cells yanked"),
            'c' => self.begin_edit(""),
            _ => {}
        }
    }

    /// `p`: writes the register at the cursor, or below it for whole rows. `P` puts rows here.
    fn paste(&mut self, before: bool) {
        let Some(register) = &self.register else {
            self.status = "Nothing to paste".into();
            return;
        };
        let paste = register.paste(self.current_sheet(), self.cursor, before);
        if self.apply_edits(paste.cells) {
            self.cursor = paste.cursor;
        }
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
            (KeyCode::Char('?') | KeyCode::F(1), _) => self.open_help(),
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
            ('Z', KeyCode::Char('Q')) => self.quit_without_asking(),
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
            KeyCode::F(5) if self.workbook.path().is_none() => {
                self.status = "Nothing to reload".into();
            }
            KeyCode::F(5) => self.ask_before_losing_edits(NextStep::Reload),
            KeyCode::F(6) if shift => self.switch_sheet(-1),
            KeyCode::F(6) => self.switch_sheet(1),
            KeyCode::Char('q' | 'c') if ctrl => {
                self.ask_before_losing_edits(NextStep::Quit);
            }
            KeyCode::Char('q') => self.ask_before_losing_edits(NextStep::Quit),
            KeyCode::Char('s') if ctrl => self.save(),
            KeyCode::Char('o') if ctrl => self.ask_before_losing_edits(NextStep::OpenFile),
            KeyCode::Char('g') if ctrl => self.begin_prompt(Mode::GoTo),
            KeyCode::Char('/') if !ctrl => self.begin_prompt(Mode::Search),
            KeyCode::Char('?') if !ctrl => self.open_help(),
            KeyCode::F(1) => self.open_help(),
            KeyCode::Char('n') if !ctrl => self.find_match(true),
            KeyCode::Char('N') if !ctrl => self.find_match(false),
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
            KeyCode::F(2) => self.edit_existing(),
            KeyCode::Char('a' | 'A') if !ctrl => self.edit_existing(),
            KeyCode::Char('i' | 'I') if !ctrl => {
                self.edit_existing();
                self.line.go_home();
            }
            KeyCode::Char('s') if !ctrl => self.begin_edit(""),
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
                    None => self.begin_edit(""),
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
            KeyCode::Char('=') if !ctrl => self.begin_edit("="),
            KeyCode::Esc if self.is_loading() => self.cancel_load(),
            _ => {}
        }
    }

    /// Quits, though a save in progress finishes first: nothing should be left half done, and the
    /// user may want to hear if it fails.
    fn quit_without_asking(&mut self) {
        if self.is_saving() {
            self.next_step = Some(NextStep::Quit);
        } else {
            self.should_quit = true;
        }
    }

    /// `ZZ`: saves and quits, except that a blank workbook with no edits has nothing to save.
    fn save_if_edited_and_quit(&mut self) {
        if self.is_saving() {
            self.quit_without_asking();
            return;
        }
        let unsaved_file = self.workbook.path().is_none() || self.is_loading();
        if unsaved_file && !self.workbook.is_dirty() {
            self.should_quit = true;
        } else {
            self.save_and_quit();
        }
    }

    /// Saves and quits, as long as the save works out.
    fn save_and_quit(&mut self) {
        self.next_step = Some(NextStep::Quit);
        self.save();
    }

    /// Starts saving, or asks where to if the workbook has no file yet. Whatever `next_step` holds
    /// happens once the save has worked.
    fn save(&mut self) {
        if self.refuse_while_busy() {
            return;
        }
        match self.workbook.begin_save() {
            Ok(save) => self.saving = Some(save),
            Err(XlsxError::NoFileName) => self.open_file_dialog(DialogKind::Save),
            Err(error) => self.report_error(format!("Save failed: {error}")),
        }
    }

    fn open_file_dialog(&mut self, kind: DialogKind) {
        self.file_dialog = Some(FileDialog::new(kind, &self.dialog_directory));
        self.mode = Mode::FileDialog;
    }

    fn file_dialog_key(&mut self, key: KeyEvent) {
        if self.is_saving() {
            return;
        }
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
                dialog.set_message("Saving…".into());
                self.saving = Some(self.workbook.begin_save_as(path));
            }
            DialogAction::Overwrite(path) => {
                dialog.set_message("Saving…".into());
                self.saving = Some(self.workbook.begin_save_over(path));
            }
            DialogAction::Open(path) => match Workbook::open_loading(path, 0) {
                Ok((workbook, load)) => {
                    self.close_file_dialog();
                    self.begin_load(workbook, load, LoadKind::Open);
                }
                Err(error) => {
                    if let Some(dialog) = &mut self.file_dialog {
                        dialog.set_message(format!("Open failed: {error}"));
                    }
                }
            },
        }
    }

    fn close_file_dialog(&mut self) {
        self.file_dialog = None;
        self.mode = Mode::Normal;
    }

    /// Reads the file again, staying on the same sheet and cell if they still exist.
    fn reload(&mut self) {
        let Some(path) = self.workbook.path().map(Path::to_path_buf) else {
            return;
        };
        match Workbook::open_loading(path, self.sheet) {
            Ok((workbook, load)) => {
                let (sheet, cursor, scroll) = (self.sheet, self.cursor, self.scroll);
                self.begin_load(workbook, load, LoadKind::Reload);
                if sheet < self.workbook.sheets.len() {
                    self.sheet = sheet;
                    self.cursor = cursor;
                    self.scroll = scroll;
                }
            }
            Err(error) => self.report_error(format!("Reload failed: {error}")),
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
        if step == NextStep::Quit && self.is_saving() {
            self.quit_without_asking();
            return;
        }
        if step != NextStep::Quit && self.refuse_while_busy() {
            return;
        }
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
            NextStep::Reload => self.reload(),
        }
    }

    fn confirm_discard_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('s' | 'S') => {
                self.mode = Mode::Normal;
                self.save();
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
        if let Some(loading) = &self.loading {
            loading.load.prefer(next);
        }
    }

    /// `{n}gt`: shows a sheet by its 1-based position.
    fn go_to_sheet(&mut self, number: u32) {
        let index = number.saturating_sub(1) as isize;
        if (index as usize) < self.workbook.sheets.len() {
            self.switch_sheet(index - self.sheet as isize);
        }
    }

    fn undo(&mut self) {
        if self.refuse_while_busy() {
            return;
        }
        match self.history.undo(&mut self.workbook.sheets) {
            Some(restored) => self.show_restored(restored),
            None => self.status = "Nothing to undo".into(),
        }
    }

    fn redo(&mut self) {
        if self.refuse_while_busy() {
            return;
        }
        match self.history.redo(&mut self.workbook.sheets) {
            Some(restored) => self.show_restored(restored),
            None => self.status = "Nothing to redo".into(),
        }
    }

    /// Shows the user the first cell an undo or redo touched.
    fn show_restored(&mut self, restored: Restored) {
        self.cursors[self.sheet] = self.cursor;
        self.sheet = restored.sheet;
        if let Some(first) = restored.first {
            self.cursor = first;
        }
    }

    fn edit_existing(&mut self) {
        let cell = self.current_sheet().cell(self.cursor);
        let text = edit_text(&cell, self.workbook.date1904);
        self.begin_edit(&text);
    }

    /// A left click on a cell in normal mode, dropping any half-typed command.
    pub fn click_cell(&mut self, at: CellRef) {
        if self.mode == Mode::Normal {
            self.forget_pending_keys();
            self.status.clear();
            self.cursor = at;
            self.scroll_to_cursor();
        }
    }

    /// A left click on a sheet's tab in normal mode.
    pub fn click_sheet(&mut self, index: usize) {
        if self.mode == Mode::Normal {
            self.forget_pending_keys();
            self.status.clear();
            self.switch_sheet(index as isize - self.sheet as isize);
            self.scroll_to_cursor();
        }
    }

    /// Moves the cursor with the mouse wheel: scrolling alone would snap back to it.
    pub fn wheel(&mut self, rows: i32) {
        if matches!(self.mode, Mode::Normal | Mode::Visual | Mode::VisualRow) {
            self.forget_pending_keys();
            self.status.clear();
            self.cursor.row = self
                .cursor
                .row
                .saturating_add_signed(rows)
                .min(MAX_ROWS - 1);
            self.scroll_to_cursor();
        }
    }

    /// A left click while the help is showing closes it.
    pub fn close_help(&mut self) {
        if self.mode == Mode::Help {
            self.mode = self.help_from;
        }
    }

    fn forget_pending_keys(&mut self) {
        self.count = None;
        self.pending = None;
        self.operator = None;
        self.operator_count = None;
    }

    fn open_help(&mut self) {
        self.help_from = self.mode;
        self.mode = Mode::Help;
    }

    /// The table of keys the help shows.
    pub fn help_table(&self) -> HelpTable {
        help::table(self.help_from)
    }

    /// Starts typing into an empty one-line prompt.
    fn begin_prompt(&mut self, mode: Mode) {
        self.line = LineEdit::default();
        self.mode = mode;
    }

    fn go_to_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => self.mode = Mode::Normal,
            KeyCode::Char('c') if is_ctrl(key) => self.mode = Mode::Normal,
            KeyCode::Enter => {
                let text = self.line.text();
                let target = text.trim();
                if target.is_empty() || self.go_to(target) {
                    self.mode = Mode::Normal;
                } else {
                    self.report_error(format!("{target} isn't a cell or a row"));
                }
            }
            _ => self.line.handle_key(key),
        }
    }

    fn search_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => self.mode = Mode::Normal,
            KeyCode::Char('c') if is_ctrl(key) => self.mode = Mode::Normal,
            KeyCode::Enter => {
                self.mode = Mode::Normal;
                let text = self.line.text();
                if !text.is_empty() {
                    self.search_query = Some(text);
                }
                if self.search_query.is_some() {
                    self.find_match(true);
                }
            }
            _ => self.line.handle_key(key),
        }
    }

    /// Moves to the next or previous cell holding the latest search, wrapping around the sheet.
    fn find_match(&mut self, forward: bool) {
        let Some(query) = self.search_query.clone() else {
            self.status = "Nothing to search for".into();
            return;
        };
        let needle = query.to_lowercase();
        let date1904 = self.workbook.date1904;
        let next = self
            .current_sheet()
            .cells_around(self.cursor, forward)
            .find(|(_, cell)| cell_contains(cell, &needle, date1904))
            .map(|(at, _)| *at);
        match next {
            Some(at) => self.cursor = at,
            None if self.is_loading() => {
                self.report_error(format!("No match for {query} in what has loaded so far"));
            }
            None => self.report_error(format!("No match for {query}")),
        }
    }

    /// Moves to a cell such as `C7`, or to a row number in the same column.
    fn go_to(&mut self, target: &str) -> bool {
        if let Ok(row) = target.parse::<u32>() {
            self.cursor.row = row.saturating_sub(1).min(MAX_ROWS - 1);
            true
        } else if let Some(at) = CellRef::parse(target) {
            self.cursor = at;
            true
        } else {
            false
        }
    }

    fn begin_edit(&mut self, initial: &str) {
        if self.refuse_while_busy() {
            return;
        }
        if self.refuse_if_locked() {
            return;
        }
        self.original = self.current_sheet().cell(self.cursor);
        self.line = LineEdit::new(initial);
        self.mode = Mode::Insert;
    }

    fn insert_key(&mut self, key: KeyEvent) {
        match key.code {
            // As in vim, Esc keeps what was typed and Ctrl-C is the way out without it.
            KeyCode::Esc => self.commit_edit(),
            KeyCode::F(1) => self.open_help(),
            KeyCode::Char('c') if is_ctrl(key) => self.mode = Mode::Normal,
            KeyCode::Enter | KeyCode::Down => self.commit_and_move(0, 1),
            KeyCode::Up => self.commit_and_move(0, -1),
            KeyCode::Tab => self.commit_and_move(1, 0),
            KeyCode::BackTab => self.commit_and_move(-1, 0),
            _ => self.line.handle_key(key),
        }
    }

    fn refuse_if_locked(&mut self) -> bool {
        let locked = self.current_sheet().is_locked(self.cursor);
        if locked {
            self.report_error(Locked { at: self.cursor }.to_string());
        }
        locked
    }

    fn apply_edit(&mut self, at: CellRef, cell: Cell) {
        self.apply_edits(vec![(at, cell)]);
    }

    /// Applies edits as one undoable change, or none of them if any cell is read-only.
    fn apply_edits(&mut self, cells: Vec<(CellRef, Cell)>) -> bool {
        if self.refuse_while_busy() {
            return false;
        }
        match self
            .history
            .apply(&mut self.workbook.sheets, self.sheet, cells)
        {
            Ok(()) => true,
            Err(locked) => {
                self.report_error(locked.to_string());
                false
            }
        }
    }

    /// The populated cells of a rectangle.
    fn filled_cells(&self, first: CellRef, last: CellRef) -> Vec<CellRef> {
        self.current_sheet()
            .cells_in(CellRange { first, last })
            .map(|(at, _)| *at)
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
        let text = self.line.text();
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
