//! The workbook open in the app, and the rules for changing it.

use std::fmt;
use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

use crate::address::CellRef;
use crate::changes::{History, Locked, Restored};
use crate::model::{Cell, Sheet};
use crate::xlsx::{Arrival, SavedPackage, Workbook, WorkbookLoad, WorkbookSave, XlsxError};

/// Why something that was asked of the document isn't happening.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    StillOpening,
    StillSaving,
    Locked(CellRef),
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::StillOpening => write!(f, "Still opening: Esc cancels"),
            Self::StillSaving => write!(f, "Still saving"),
            Self::Locked(at) => Locked { at: *at }.fmt(f),
        }
    }
}

/// Why a save didn't start.
#[derive(Debug)]
pub enum SaveError {
    Refused(Refusal),
    /// The workbook has never been saved, so it has no file to go to.
    NoFile,
    Failed(XlsxError),
}

/// Why another workbook didn't start loading.
#[derive(Debug)]
pub enum OpenError {
    Refused(Refusal),
    Failed(XlsxError),
}

impl fmt::Display for OpenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Refused(refusal) => refusal.fmt(f),
            Self::Failed(error) => error.fmt(f),
        }
    }
}

/// News of background work that has ended.
#[derive(Debug)]
pub enum Outcome {
    Saved,
    SaveFailed(XlsxError),
    /// All the cells have arrived. `replaced` says whether another workbook gave way for this one.
    Loaded {
        kind: LoadKind,
        replaced: bool,
    },
    /// The cells stopped arriving. `went_back` says whether there was a workbook to return to.
    LoadFailed {
        kind: LoadKind,
        went_back: bool,
        error: XlsxError,
    },
}

/// Why a workbook is loading.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadKind {
    Open,
    Reload,
}

/// What's happening in the background. A save needs the whole workbook and mustn't see it change,
/// so there's only ever one thing going on.
enum Work {
    Saving(WorkbookSave),
    Loading(Loading),
}

/// A workbook on show whose cells are still arriving.
struct Loading {
    load: WorkbookLoad,
    kind: LoadKind,
    /// What to go back to if the load doesn't work out. The first workbook has nothing to go to.
    previous: Option<Box<Previous>>,
}

/// A workbook put aside while another loads, and the place the user was in it.
struct Previous {
    workbook: Workbook,
    place: Place,
    history: History,
}

/// Freeing a large workbook takes a noticeable moment, so it's done out of the user's way.
fn free_later<T: Send + 'static>(value: T) {
    thread::spawn(move || drop(value));
}

impl fmt::Debug for Work {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Saving(_) => "Saving",
            Self::Loading(_) => "Loading",
        })
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
    work: Option<Work>,
}

impl Document {
    pub fn new(workbook: Workbook) -> Self {
        Self {
            place: Place::start(workbook.sheets.len()),
            workbook,
            history: History::default(),
            work: None,
        }
    }

    /// A document showing a workbook whose cells are on their way.
    pub fn opening(workbook: Workbook, load: WorkbookLoad) -> Self {
        let mut document = Self::new(workbook);
        document.work = Some(Work::Loading(Loading {
            load,
            kind: LoadKind::Open,
            previous: None,
        }));
        document
    }

    pub fn is_loading(&self) -> bool {
        matches!(self.work, Some(Work::Loading(_)))
    }

    /// How much of the workbook has been read, from 0 to 1, while it's loading.
    pub fn loading_progress(&self) -> Option<f64> {
        match &self.work {
            Some(Work::Loading(loading)) => Some(loading.load.progress()),
            _ => None,
        }
    }

    pub fn is_saving(&self) -> bool {
        matches!(self.work, Some(Work::Saving(_)))
    }

    /// Whether something is going on that the screen should keep up with without a key press.
    pub fn is_busy(&self) -> bool {
        self.work.is_some()
    }

    /// Whether the workbook may change right now. A save is writing a snapshot, and taking in its
    /// outcome marks every cell as saved, so a change made meanwhile would be lost.
    pub fn ensure_changeable(&self) -> Result<(), Refusal> {
        match self.work {
            Some(Work::Saving(_)) => Err(Refusal::StillSaving),
            Some(Work::Loading(_)) => Err(Refusal::StillOpening),
            None => Ok(()),
        }
    }

    /// Whether the whole workbook is here to be read.
    pub fn ensure_complete(&self) -> Result<(), Refusal> {
        match self.work {
            Some(Work::Loading(_)) => Err(Refusal::StillOpening),
            _ => Ok(()),
        }
    }

    /// Whether a cell may be edited right now.
    pub fn ensure_editable(&self, at: CellRef) -> Result<(), Refusal> {
        self.ensure_changeable()?;
        if self.current_sheet().is_locked(at) {
            return Err(Refusal::Locked(at));
        }
        Ok(())
    }

    /// Puts another workbook on show with its cells on their way, keeping this one until they have
    /// all arrived. A file that isn't a workbook fails here, and leaves things as they are.
    pub fn open(&mut self, path: impl AsRef<Path>) -> Result<(), OpenError> {
        self.ensure_changeable().map_err(OpenError::Refused)?;
        let (workbook, load) = Workbook::open_loading(path, 0).map_err(OpenError::Failed)?;
        self.begin_load(workbook, load, LoadKind::Open);
        Ok(())
    }

    /// Reads the file again, staying on the same sheet and cell if they still exist.
    pub fn reload(&mut self) -> Result<(), OpenError> {
        self.ensure_changeable().map_err(OpenError::Refused)?;
        let path = self.workbook.path();
        let path = path.ok_or(OpenError::Failed(XlsxError::NoFileName))?;
        let (workbook, load) =
            Workbook::open_loading(path, self.place.sheet).map_err(OpenError::Failed)?;
        let (sheet, cursor, scroll) = (self.place.sheet, self.place.cursor, self.place.scroll);
        self.begin_load(workbook, load, LoadKind::Reload);
        if sheet < self.workbook.sheets.len() {
            self.place.sheet = sheet;
            self.place.cursor = cursor;
            self.place.scroll = scroll;
        }
        Ok(())
    }

    fn begin_load(&mut self, workbook: Workbook, load: WorkbookLoad, kind: LoadKind) {
        let previous = Previous {
            place: std::mem::replace(&mut self.place, Place::start(workbook.sheets.len())),
            history: std::mem::take(&mut self.history),
            workbook: std::mem::replace(&mut self.workbook, workbook),
        };
        self.work = Some(Work::Loading(Loading {
            load,
            kind,
            previous: Some(Box::new(previous)),
        }));
    }

    /// Stops loading. Says whether there was another workbook to go back to.
    pub fn cancel_load(&mut self) -> bool {
        let Some(loading) = self.take_loading() else {
            return false;
        };
        match loading.previous {
            Some(previous) => {
                self.go_back_to(*previous);
                true
            }
            None => false,
        }
    }

    fn take_loading(&mut self) -> Option<Loading> {
        match self.work.take() {
            Some(Work::Loading(loading)) => Some(loading),
            other => {
                self.work = other;
                None
            }
        }
    }

    fn go_back_to(&mut self, previous: Previous) {
        free_later(std::mem::replace(&mut self.workbook, previous.workbook));
        self.place = previous.place;
        self.history = previous.history;
    }

    /// Waits for the next chunk of cells, or for the load to end. Says how it ended, if it has.
    pub fn absorb_next_chunk(&mut self) -> Option<Outcome> {
        let Some(Work::Loading(loading)) = &self.work else {
            return None;
        };
        let arrival = loading.load.receive(&mut self.workbook);
        self.take_arrival(arrival)
    }

    /// Waits for the whole workbook, and says how the load ended.
    pub fn finish_loading(&mut self) -> Option<Outcome> {
        while self.is_loading() {
            if let Some(outcome) = self.absorb_next_chunk() {
                return Some(outcome);
            }
        }
        None
    }

    fn absorb_loaded(&mut self, budget: Duration) -> Option<Outcome> {
        let deadline = Instant::now() + budget;
        while let Some(Work::Loading(loading)) = &self.work {
            let wait = deadline.saturating_duration_since(Instant::now());
            let arrival = loading.load.receive_within(&mut self.workbook, wait);
            if matches!(arrival, Arrival::Nothing) {
                return None;
            }
            let outcome = self.take_arrival(arrival);
            if outcome.is_some() || Instant::now() >= deadline {
                return outcome;
            }
        }
        None
    }

    fn take_arrival(&mut self, arrival: Arrival) -> Option<Outcome> {
        match arrival {
            Arrival::Cells | Arrival::Nothing => None,
            Arrival::Ended => self.finish_load(),
            Arrival::Failed(error) => {
                let loading = self.take_loading()?;
                let went_back = loading.previous.is_some();
                if let Some(previous) = loading.previous {
                    self.go_back_to(*previous);
                }
                Some(Outcome::LoadFailed {
                    kind: loading.kind,
                    went_back,
                    error,
                })
            }
        }
    }

    fn finish_load(&mut self) -> Option<Outcome> {
        let loading = self.take_loading()?;
        let replaced = loading.previous.is_some();
        if let Some(previous) = loading.previous {
            free_later(previous);
        }
        Some(Outcome::Loaded {
            kind: loading.kind,
            replaced,
        })
    }

    /// Starts saving to the file this workbook belongs to.
    pub fn save(&mut self) -> Result<(), SaveError> {
        self.ensure_changeable().map_err(SaveError::Refused)?;
        match self.workbook.begin_save() {
            Ok(save) => {
                self.work = Some(Work::Saving(save));
                Ok(())
            }
            Err(XlsxError::NoFileName) => Err(SaveError::NoFile),
            Err(error) => Err(SaveError::Failed(error)),
        }
    }

    /// Starts saving to a new file, which the workbook then belongs to.
    pub fn save_as(&mut self, path: impl AsRef<Path>) -> Result<(), Refusal> {
        self.ensure_changeable()?;
        self.work = Some(Work::Saving(self.workbook.begin_save_as(path)));
        Ok(())
    }

    /// Starts saving over an existing file, which the workbook then belongs to.
    pub fn save_over(&mut self, path: impl AsRef<Path>) -> Result<(), Refusal> {
        self.ensure_changeable()?;
        self.work = Some(Work::Saving(self.workbook.begin_save_over(path)));
        Ok(())
    }

    /// Spends up to `budget` on the work in progress, stopping short if there's no news. Says how
    /// it ended, if it has.
    pub fn absorb(&mut self, budget: Duration) -> Option<Outcome> {
        match &self.work {
            Some(Work::Saving(save)) => {
                let result = save.result_within(budget)?;
                Some(self.end_save(result))
            }
            Some(Work::Loading(_)) => self.absorb_loaded(budget),
            None => None,
        }
    }

    /// Waits for the save in progress.
    pub fn finish_saving(&mut self) -> Option<Outcome> {
        let Some(Work::Saving(save)) = &self.work else {
            return None;
        };
        let result = save.wait();
        Some(self.end_save(result))
    }

    fn end_save(&mut self, result: Result<SavedPackage, XlsxError>) -> Outcome {
        self.work = None;
        match result {
            Ok(saved) => {
                self.workbook.finish_save(saved);
                Outcome::Saved
            }
            Err(error) => Outcome::SaveFailed(error),
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
        if let Some(Work::Loading(loading)) = &self.work {
            loading.load.prefer(next);
        }
    }

    fn show_sheet(&mut self, sheet: usize) {
        self.place.cursors[self.place.sheet] = self.place.cursor;
        self.place.sheet = sheet;
        self.place.cursor = self.place.cursors[sheet];
    }

    /// Changes cells of the sheet on show as one undoable step, or none of them if it's refused.
    pub fn edit(&mut self, cells: Vec<(CellRef, Cell)>) -> Result<(), Refusal> {
        self.ensure_changeable()?;
        self.history
            .apply(&mut self.workbook.sheets, self.place.sheet, cells)
            .map_err(|locked| Refusal::Locked(locked.at))
    }

    /// Takes back the latest change and shows where it was. Says whether there was one.
    pub fn undo(&mut self) -> Result<bool, Refusal> {
        self.ensure_changeable()?;
        let restored = self.history.undo(&mut self.workbook.sheets);
        Ok(self.show_restored(restored))
    }

    /// Makes the latest undone change again and shows where it was. Says whether there was one.
    pub fn redo(&mut self) -> Result<bool, Refusal> {
        self.ensure_changeable()?;
        let restored = self.history.redo(&mut self.workbook.sheets);
        Ok(self.show_restored(restored))
    }

    fn show_restored(&mut self, restored: Option<Restored>) -> bool {
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
