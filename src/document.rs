//! The workbook open in the app, and the rules for changing it.

use std::fmt;
use std::path::Path;
use std::time::Duration;

use crate::address::CellRef;
use crate::changes::{History, Locked, Restored};
use crate::model::{Cell, Sheet};
use crate::xlsx::{SavedPackage, Workbook, WorkbookSave, XlsxError};

/// Why something that was asked of the document isn't happening.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    StillSaving,
    Locked(CellRef),
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
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

/// News of background work that has ended.
#[derive(Debug)]
pub enum Outcome {
    Saved,
    SaveFailed(XlsxError),
}

/// What's happening in the background. A save needs the whole workbook and mustn't see it change,
/// so there's only ever one thing going on.
enum Work {
    Saving(WorkbookSave),
}

impl fmt::Debug for Work {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Saving(_) => "Saving",
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
            None => Ok(()),
        }
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
