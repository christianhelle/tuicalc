//! Application state: everything the keyboard can change.

use std::path::{Path, PathBuf};
use std::time::Duration;

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind};

use crate::address::{CellRange, CellRef, MAX_COLS, MAX_ROWS};
use crate::changes::Register;
use crate::command::{Command, Grammar, Operator};
use crate::document::{Document, LoadKind, Outcome, Refusal, SaveError};
use crate::file_dialog::{DialogAction, DialogKind, FileDialog};
use crate::help::{self, HelpTable};
use crate::keys::is_ctrl;
use crate::line_edit::LineEdit;
use crate::model::{Cell, Sheet};
use crate::motion::{Motion, Spot, last_filled_column, moved};
use crate::text::{cell_contains, edit_text, parse_input};
use crate::xlsx::{Workbook, WorkbookLoad};

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

/// A message for the user about background work.
struct News {
    message: String,
    is_error: bool,
}

pub struct App {
    /// The workbook on show, and where the user is in it.
    doc: Document,
    /// The visible grid size, in columns and rows.
    viewport: (u32, u32),
    mode: Mode,
    /// The text being typed into a cell or a prompt.
    line: LineEdit,
    /// The cell as it was when editing began.
    original: Cell,
    status: String,
    status_is_error: bool,
    should_quit: bool,
    /// The part of a command typed so far.
    grammar: Grammar,
    register: Option<Register>,
    file_dialog: Option<FileDialog>,
    /// Where the file dialog starts.
    dialog_directory: PathBuf,
    /// What's waiting on the unsaved edits being saved or discarded.
    next_step: Option<NextStep>,
    /// Whether `next_step` has only been waiting for the user to finish what they're doing.
    next_step_due: bool,
    /// A message about background work, kept until the user is back where it can be read.
    news: Option<News>,
    /// The latest search, which `n` and `N` repeat.
    search_query: Option<String>,
    /// The mode to go back to when the help closes.
    help_from: Mode,
    /// Why the workbook the app opened with couldn't be read, once it has given up.
    load_failure: Option<String>,
}

impl App {
    pub fn new(workbook: Workbook) -> Self {
        Self::showing(Document::new(workbook))
    }

    /// An app showing a workbook whose cells are on their way.
    pub fn opening(workbook: Workbook, load: WorkbookLoad) -> Self {
        Self::showing(Document::opening(workbook, load))
    }

    fn showing(doc: Document) -> Self {
        Self {
            doc,
            grammar: Grammar::default(),
            register: None,
            file_dialog: None,
            next_step: None,
            next_step_due: false,
            news: None,
            search_query: None,
            help_from: Mode::Normal,
            load_failure: None,
            dialog_directory: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            viewport: (10, 20),
            mode: Mode::Normal,
            line: LineEdit::default(),
            original: Cell::default(),
            status: String::new(),
            status_is_error: false,
            should_quit: false,
        }
    }

    /// The workbook on show, and what's going on with it.
    pub fn document(&self) -> &Document {
        &self.doc
    }

    /// Spends up to `budget` on the save or load in progress, stopping short if there's no news.
    pub fn absorb_background(&mut self, budget: Duration) {
        if let Some(outcome) = self.doc.absorb(budget) {
            self.take_outcome(outcome);
        }
    }

    /// Waits for the save in progress.
    pub fn finish_saving(&mut self) {
        if let Some(outcome) = self.doc.finish_saving() {
            self.take_outcome(outcome);
        }
    }

    /// Waits for the next chunk of cells, or for the load to end.
    pub fn absorb_next_chunk(&mut self) {
        if let Some(outcome) = self.doc.absorb_next_chunk() {
            self.take_outcome(outcome);
        }
    }

    /// Waits for the whole workbook.
    pub fn finish_loading(&mut self) {
        if let Some(outcome) = self.doc.finish_loading() {
            self.take_outcome(outcome);
        }
    }

    /// Takes in how some background work ended, and takes whatever step was waiting on it.
    fn take_outcome(&mut self, outcome: Outcome) {
        match outcome {
            Outcome::Saved => {
                if self.file_dialog.is_some() {
                    self.close_file_dialog();
                }
                self.announce(News {
                    message: format!("Saved {}", self.file_name()),
                    is_error: false,
                });
                if self.next_step == Some(NextStep::Quit) {
                    self.should_quit = true;
                } else {
                    self.next_step_due = self.next_step.is_some();
                    self.take_due_step();
                }
            }
            // The dialog stays up to try another name, with the step still waiting.
            Outcome::SaveFailed(error) => match &mut self.file_dialog {
                Some(dialog) => dialog.set_message(format!("Save failed: {error}")),
                None => {
                    self.next_step = None;
                    self.announce(News {
                        message: format!("Save failed: {error}"),
                        is_error: true,
                    });
                }
            },
            Outcome::Loaded {
                replaced: false, ..
            } => {}
            Outcome::Loaded { kind, .. } => {
                let verb = match kind {
                    LoadKind::Open => "Opened",
                    LoadKind::Reload => "Reloaded",
                };
                self.announce(News {
                    message: format!("{verb} {}", self.file_name()),
                    is_error: false,
                });
            }
            Outcome::LoadFailed {
                kind,
                went_back: true,
                error,
            } => {
                self.mode = Mode::Normal;
                self.help_from = Mode::Normal;
                let verb = match kind {
                    LoadKind::Open => "Open",
                    LoadKind::Reload => "Reload",
                };
                self.report_error(format!("{verb} failed: {error}"));
            }
            Outcome::LoadFailed { error, .. } => {
                self.load_failure = Some(error.to_string());
                self.should_quit = true;
            }
        }
    }

    /// Tells the user about background work now, or once they're back from a prompt or the help,
    /// whose own messages it would be taken for or hidden by.
    fn announce(&mut self, news: News) {
        if self.mode == Mode::Normal {
            self.status = news.message;
            self.status_is_error = news.is_error;
        } else {
            self.news = Some(news);
        }
    }

    /// Takes the waiting step if it's due and the user isn't in the middle of something.
    fn take_due_step(&mut self) {
        if self.next_step_due && self.mode == Mode::Normal {
            self.next_step_due = false;
            if let Some(step) = self.next_step.take() {
                self.take_step(step);
            }
        }
    }

    /// Catches up on what has been waiting for the user to finish what they were doing.
    fn catch_up(&mut self) {
        if self.mode == Mode::Normal
            && self.status.is_empty()
            && let Some(news) = self.news.take()
        {
            self.announce(news);
        }
        self.take_due_step();
    }

    pub fn load_failure(&self) -> Option<&str> {
        self.load_failure.as_deref()
    }

    /// Says why something isn't happening if the document refused it; returns whether it did.
    fn refuse(&mut self, check: Result<(), Refusal>) -> bool {
        match check {
            Ok(()) => false,
            Err(refusal @ Refusal::Locked(_)) => {
                self.report_error(refusal.to_string());
                true
            }
            Err(refusal) => {
                self.status = refusal.to_string();
                true
            }
        }
    }

    /// Stops loading at the user's request.
    fn cancel_load(&mut self) {
        if self.doc.cancel_load() {
            self.mode = Mode::Normal;
            self.help_from = Mode::Normal;
            self.status = "Stopped opening".into();
        } else {
            self.should_quit = true;
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
        self.doc.place.cursor
    }

    pub fn sheet_index(&self) -> usize {
        self.doc.sheet_index()
    }

    pub fn current_sheet(&self) -> &Sheet {
        self.doc.current_sheet()
    }

    pub fn set_viewport(&mut self, cols: u32, rows: u32) {
        self.viewport = (cols.max(1), rows.max(1));
        self.scroll_to_cursor();
    }

    /// The keys of a command still being typed, as vim's `showcmd` shows them.
    pub fn pending_keys(&self) -> String {
        self.grammar.pending_keys()
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
        self.doc.place.scroll
    }

    /// Adjusts the scroll position just enough to show the cursor.
    fn scroll_to_cursor(&mut self) {
        let (cols, rows) = self.viewport;
        if self.doc.place.cursor.col < self.doc.place.scroll.col {
            self.doc.place.scroll.col = self.doc.place.cursor.col;
        } else if self.doc.place.cursor.col >= self.doc.place.scroll.col + cols {
            self.doc.place.scroll.col = self.doc.place.cursor.col + 1 - cols;
        }
        if self.doc.place.cursor.row < self.doc.place.scroll.row {
            self.doc.place.scroll.row = self.doc.place.cursor.row;
        } else if self.doc.place.cursor.row >= self.doc.place.scroll.row + rows {
            self.doc.place.scroll.row = self.doc.place.cursor.row + 1 - rows;
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
            Mode::Normal => {
                if let Some(command) = self.grammar.normal(key) {
                    self.run(command);
                }
            }
            Mode::Insert => self.insert_key(key),
            Mode::Visual | Mode::VisualRow => {
                if let Some(command) = self.grammar.visual(key, self.mode == Mode::VisualRow) {
                    self.run(command);
                }
            }
            Mode::GoTo => self.go_to_key(key),
            Mode::Search => self.search_key(key),
            Mode::Help => self.mode = self.help_from,
            Mode::ConfirmDiscard => self.confirm_discard_key(key),
            Mode::FileDialog => self.file_dialog_key(key),
        }
        self.catch_up();
        self.scroll_to_cursor();
    }

    /// Carries out a command typed in normal or visual mode.
    fn run(&mut self, command: Command) {
        match command {
            Command::Move { motion, count } => self.run_motion(motion, count),
            Command::OperateOnMotion {
                operator,
                motion,
                count,
            } => {
                let origin = self.doc.place.cursor;
                self.run_motion(motion, count);
                if !(motion.is_backward() && origin == self.doc.place.cursor) {
                    self.finish_operator(operator, origin, motion.is_exclusive());
                }
            }
            Command::OperateOnRows { operator, count } => self.operate_on_rows(operator, count),
            Command::OperateOnSelection(operator) => self.operate_on_selection(operator),
            Command::SwitchSheet(offset) => self.switch_sheet(offset),
            Command::GoToSheet(number) => self.go_to_sheet(number),
            Command::SaveIfEditedAndQuit => self.save_if_edited_and_quit(),
            Command::QuitWithoutAsking => self.quit_without_asking(),
            Command::ScrollCursorToMiddle => {
                self.doc.place.scroll.row = self
                    .doc
                    .place
                    .cursor
                    .row
                    .saturating_sub(self.viewport.1 / 2);
            }
            Command::ScrollCursorToTop => self.doc.place.scroll.row = self.doc.place.cursor.row,
            Command::ScrollCursorToBottom => {
                self.doc.place.scroll.row =
                    (self.doc.place.cursor.row + 1).saturating_sub(self.viewport.1);
            }
            Command::Reload if self.doc.workbook().path().is_none() => {
                self.status = "Nothing to reload".into();
            }
            Command::Reload => self.ask_before_losing_edits(NextStep::Reload),
            Command::Quit => self.ask_before_losing_edits(NextStep::Quit),
            Command::Save => self.save(),
            Command::OpenFile => self.ask_before_losing_edits(NextStep::OpenFile),
            Command::BeginGoTo => self.begin_prompt(Mode::GoTo),
            Command::BeginSearch => self.begin_prompt(Mode::Search),
            Command::OpenHelp => self.open_help(),
            Command::FindNext => self.find_match(true),
            Command::FindPrevious => self.find_match(false),
            Command::Undo => self.undo(),
            Command::Redo => self.redo(),
            Command::ClearHere => self.apply_edit(self.doc.place.cursor, Cell::default()),
            Command::Paste { before } => self.paste(before),
            Command::ClearRight { count } => {
                let cursor = self.doc.place.cursor;
                let last = CellRef {
                    col: (cursor.col + (count - 1)).min(MAX_COLS - 1),
                    ..cursor
                };
                self.clear_cells(cursor, last);
            }
            Command::ClearLeft { count } => {
                let cursor = self.doc.place.cursor;
                if cursor.col > 0 {
                    let first = CellRef {
                        col: cursor.col.saturating_sub(count),
                        ..cursor
                    };
                    let last = CellRef {
                        col: cursor.col - 1,
                        ..cursor
                    };
                    if self.clear_cells(first, last) {
                        self.doc.place.cursor = first;
                    }
                }
            }
            Command::ClearToRowEnd => {
                if let Some(last) = self.last_filled_ahead() {
                    self.clear_cells(self.doc.place.cursor, last);
                }
            }
            Command::SelectCells if self.selection().is_some() => self.mode = Mode::Visual,
            Command::SelectRows if self.selection().is_some() => self.mode = Mode::VisualRow,
            Command::SelectCells => self.begin_visual(Mode::Visual),
            Command::SelectRows => self.begin_visual(Mode::VisualRow),
            Command::StopSelecting => self.mode = Mode::Normal,
            Command::SwapSelectionCorners => {
                let place = &mut self.doc.place;
                std::mem::swap(&mut place.anchor, &mut place.cursor);
            }
            Command::EditAtEnd => self.edit_existing(),
            Command::EditAtStart => {
                self.edit_existing();
                self.line.go_home();
            }
            Command::Replace => self.begin_edit(""),
            Command::ChangeToRowEnd => match self.last_filled_ahead() {
                Some(last) => {
                    let cursor = self.doc.place.cursor;
                    self.operate(Operator::Change, cursor, last, false, cursor);
                }
                None => self.begin_edit(""),
            },
            Command::EditBelow => {
                self.move_by(0, 1);
                self.edit_existing();
            }
            Command::EditAbove => {
                self.move_by(0, -1);
                self.edit_existing();
            }
            Command::BeginFormula => self.begin_edit("="),
            Command::CancelLoad => {
                if self.doc.is_loading() {
                    self.cancel_load();
                }
            }
        }
    }

    /// Moves the cursor, and the view for the motions that take it along.
    fn run_motion(&mut self, motion: Motion, count: Option<u32>) {
        let spot = Spot {
            cursor: self.doc.place.cursor,
            scroll: self.doc.place.scroll,
        };
        let landed = motion.land(count, self.doc.current_sheet(), spot, self.viewport.1);
        self.doc.place.cursor = landed.cursor;
        self.doc.place.scroll = landed.scroll;
    }

    /// The last filled cell of the cursor's row, if it's not behind the cursor.
    fn last_filled_ahead(&self) -> Option<CellRef> {
        let cursor = self.doc.place.cursor;
        last_filled_column(self.doc.current_sheet(), cursor.row)
            .filter(|col| *col >= cursor.col)
            .map(|col| CellRef { col, ..cursor })
    }

    fn begin_visual(&mut self, mode: Mode) {
        self.doc.place.anchor = self.doc.place.cursor;
        self.mode = mode;
    }

    /// The selected rectangle, as its top-left and bottom-right cells.
    pub fn selection(&self) -> Option<CellRange> {
        let corners = CellRange::between(self.doc.place.anchor, self.doc.place.cursor);
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

    /// Completes `d`, `y` or `c` with the motion that just moved the cursor from `origin`.
    /// Exclusive motions stop short of where they landed, so `dw` keeps the next run and `dl` takes
    /// one cell.
    fn finish_operator(&mut self, operator: Operator, origin: CellRef, exclusive: bool) {
        let target = self.doc.place.cursor;
        let CellRange { first, mut last } = CellRange::between(origin, target);
        if exclusive && target.col != origin.col {
            last.col -= 1;
        }
        self.doc.place.cursor = origin;
        self.operate(operator, first, last, false, first);
    }

    /// `dd`, `yy` and `cc`: the operator on whole rows, starting at the cursor's.
    fn operate_on_rows(&mut self, operator: Operator, count: u32) {
        let row = self.doc.place.cursor.row;
        let bottom = row.saturating_add(count - 1).min(MAX_ROWS - 1);
        let first = CellRef { col: 0, row };
        let last = CellRef {
            col: MAX_COLS - 1,
            row: bottom,
        };
        self.operate(operator, first, last, true, self.doc.place.cursor);
    }

    fn operate_on_selection(&mut self, operator: Operator) {
        let Some(CellRange { first, last }) = self.selection() else {
            return;
        };
        let rows = self.mode == Mode::VisualRow;
        self.mode = Mode::Normal;
        let at = if rows {
            CellRef {
                col: self.doc.place.cursor.col,
                row: first.row,
            }
        } else {
            first
        };
        self.operate(operator, first, last, rows, at);
    }

    /// Runs `d`, `y` or `c` on a rectangle, leaving the cursor at `at`.
    fn operate(
        &mut self,
        operator: Operator,
        first: CellRef,
        last: CellRef,
        rows: bool,
        at: CellRef,
    ) {
        let check = if operator == Operator::Yank {
            self.doc.ensure_complete()
        } else {
            self.doc.ensure_changeable()
        };
        if self.refuse(check) {
            return;
        }
        let range = CellRange { first, last };
        let register = Register::yank(self.current_sheet(), range, rows);
        let yanked = register.len();
        if operator != Operator::Yank && !self.clear_cells(first, last) {
            return;
        }
        self.register = Some(register);
        self.doc.place.cursor = at;
        match operator {
            Operator::Yank => self.status = format!("{yanked} cells yanked"),
            Operator::Change => self.begin_edit(""),
            Operator::Delete => {}
        }
    }

    /// `p`: writes the register at the cursor, or below it for whole rows. `P` puts rows here.
    fn paste(&mut self, before: bool) {
        let Some(register) = &self.register else {
            self.status = "Nothing to paste".into();
            return;
        };
        let paste = register.paste(self.current_sheet(), self.doc.place.cursor, before);
        if self.apply_edits(paste.cells) {
            self.doc.place.cursor = paste.cursor;
        }
    }

    /// Quits, though a save in progress finishes first: nothing should be left half done, and the
    /// user may want to hear if it fails.
    fn quit_without_asking(&mut self) {
        if self.doc.is_saving() {
            self.next_step = Some(NextStep::Quit);
        } else {
            self.should_quit = true;
        }
    }

    /// `ZZ`: saves and quits, except that a blank workbook with no edits has nothing to save.
    fn save_if_edited_and_quit(&mut self) {
        if self.doc.is_saving() {
            self.quit_without_asking();
            return;
        }
        let unsaved_file = self.doc.workbook().path().is_none() || self.doc.is_loading();
        if unsaved_file && !self.doc.workbook().is_dirty() {
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
        match self.doc.save() {
            Ok(()) => {}
            Err(SaveError::Refused(refusal)) => {
                self.refuse(Err(refusal));
            }
            Err(SaveError::NoFile) => self.open_file_dialog(DialogKind::Save),
            Err(SaveError::Failed(error)) => self.report_error(format!("Save failed: {error}")),
        }
    }

    fn open_file_dialog(&mut self, kind: DialogKind) {
        self.file_dialog = Some(FileDialog::new(kind, &self.dialog_directory));
        self.mode = Mode::FileDialog;
    }

    fn file_dialog_key(&mut self, key: KeyEvent) {
        if self.doc.is_saving() {
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
                let started = self.doc.save_as(path);
                dialog.set_message(saving_message(started));
            }
            DialogAction::Overwrite(path) => {
                let started = self.doc.save_over(path);
                dialog.set_message(saving_message(started));
            }
            DialogAction::Open(path) => match self.doc.open(path) {
                Ok(()) => self.close_file_dialog(),
                Err(error) => dialog.set_message(format!("Open failed: {error}")),
            },
        }
    }

    fn close_file_dialog(&mut self) {
        self.file_dialog = None;
        self.mode = Mode::Normal;
    }

    /// Reads the file again, staying on the same sheet and cell if they still exist.
    fn reload(&mut self) {
        if self.doc.workbook().path().is_none() {
            return;
        }
        if let Err(error) = self.doc.reload() {
            self.report_error(format!("Reload failed: {error}"));
        }
    }

    pub fn file_name(&self) -> String {
        let Some(path) = self.doc.workbook().path() else {
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
        if step == NextStep::Quit && self.doc.is_saving() {
            self.quit_without_asking();
            return;
        }
        if step != NextStep::Quit && self.refuse(self.doc.ensure_changeable()) {
            return;
        }
        if self.doc.workbook().is_dirty() {
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
        self.doc.switch_sheet(offset);
    }

    /// `{n}gt`: shows a sheet by its 1-based position.
    fn go_to_sheet(&mut self, number: u32) {
        let index = number.saturating_sub(1) as isize;
        if (index as usize) < self.doc.workbook().sheets.len() {
            self.switch_sheet(index - self.doc.sheet_index() as isize);
        }
    }

    fn undo(&mut self) {
        match self.doc.undo() {
            Ok(true) => {}
            Ok(false) => self.status = "Nothing to undo".into(),
            Err(refusal) => {
                self.refuse(Err(refusal));
            }
        }
    }

    fn redo(&mut self) {
        match self.doc.redo() {
            Ok(true) => {}
            Ok(false) => self.status = "Nothing to redo".into(),
            Err(refusal) => {
                self.refuse(Err(refusal));
            }
        }
    }

    fn edit_existing(&mut self) {
        let cell = self.current_sheet().cell(self.doc.place.cursor);
        let text = edit_text(&cell, self.doc.workbook().date1904);
        self.begin_edit(&text);
    }

    /// A left click on a cell in normal mode, dropping any half-typed command.
    pub fn click_cell(&mut self, at: CellRef) {
        if self.mode == Mode::Normal {
            self.forget_pending_keys();
            self.status.clear();
            self.doc.place.cursor = at;
            self.scroll_to_cursor();
        }
    }

    /// A left click on a sheet's tab in normal mode.
    pub fn click_sheet(&mut self, index: usize) {
        if self.mode == Mode::Normal {
            self.forget_pending_keys();
            self.status.clear();
            self.switch_sheet(index as isize - self.doc.sheet_index() as isize);
            self.scroll_to_cursor();
        }
    }

    /// Moves the cursor with the mouse wheel: scrolling alone would snap back to it.
    pub fn wheel(&mut self, rows: i32) {
        if matches!(self.mode, Mode::Normal | Mode::Visual | Mode::VisualRow) {
            self.forget_pending_keys();
            self.status.clear();
            self.doc.place.cursor.row = self
                .doc
                .place
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
            self.catch_up();
        }
    }

    fn forget_pending_keys(&mut self) {
        self.grammar.forget();
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
        let date1904 = self.doc.workbook().date1904;
        let next = self
            .current_sheet()
            .cells_around(self.doc.place.cursor, forward)
            .find(|(_, cell)| cell_contains(cell, &needle, date1904))
            .map(|(at, _)| *at);
        match next {
            Some(at) => self.doc.place.cursor = at,
            None if self.doc.is_loading() => {
                self.report_error(format!("No match for {query} in what has loaded so far"));
            }
            None => self.report_error(format!("No match for {query}")),
        }
    }

    /// Moves to a cell such as `C7`, or to a row number in the same column.
    fn go_to(&mut self, target: &str) -> bool {
        if let Ok(row) = target.parse::<u32>() {
            self.doc.place.cursor.row = row.saturating_sub(1).min(MAX_ROWS - 1);
            true
        } else if let Some(at) = CellRef::parse(target) {
            self.doc.place.cursor = at;
            true
        } else {
            false
        }
    }

    fn begin_edit(&mut self, initial: &str) {
        if self.refuse(self.doc.ensure_editable(self.doc.place.cursor)) {
            return;
        }
        self.original = self.current_sheet().cell(self.doc.place.cursor);
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

    fn apply_edit(&mut self, at: CellRef, cell: Cell) {
        self.apply_edits(vec![(at, cell)]);
    }

    /// Applies edits as one undoable change, or none of them if any cell is read-only.
    fn apply_edits(&mut self, cells: Vec<(CellRef, Cell)>) -> bool {
        let edited = self.doc.edit(cells);
        !self.refuse(edited)
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
        let at = self.doc.place.cursor;
        let text = self.line.text();
        let was_date = self.current_sheet().cell(at).is_date;
        let date1904 = self.doc.workbook().date1904;
        // Confirming unchanged text must not disturb the cell, cached formula results included.
        if text == edit_text(&self.original, date1904) {
            return;
        }
        let cell = parse_input(&text, was_date, date1904);
        self.apply_edit(at, cell);
    }

    fn move_by(&mut self, cols: i64, rows: i64) {
        self.doc.place.cursor = moved(self.doc.place.cursor, cols, rows);
    }
}

/// What the file dialog says while its save is under way.
fn saving_message(started: Result<(), Refusal>) -> String {
    match started {
        Ok(()) => "Saving…".into(),
        Err(refusal) => refusal.to_string(),
    }
}
