//! Reading and patching .xlsx packages.

mod loading;
mod reader;
mod saving;
mod writer;
mod xml;

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::model::Sheet;

pub use loading::{Arrival, WorkbookLoad};
use saving::{SaveJob, Target};
pub use saving::{SavedPackage, WorkbookSave};

#[derive(Debug)]
pub enum WorkbookError {
    Io(std::io::Error),
    Zip(zip::result::ZipError),
    Xml(String),
    /// A required part or relationship is absent.
    Invalid(String),
    /// Another program modified the file after it was opened.
    ChangedOnDisk,
    /// The workbook has never been saved, so it has no file to go to.
    NoFileName,
}

impl fmt::Display for WorkbookError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "{error}"),
            Self::Zip(error) => write!(f, "not a readable .xlsx package: {error}"),
            Self::Xml(message) => write!(f, "malformed XML: {message}"),
            Self::Invalid(message) => write!(f, "invalid workbook: {message}"),
            Self::ChangedOnDisk => write!(f, "the file changed on disk since it was opened"),
            Self::NoFileName => write!(f, "no file name"),
        }
    }
}

impl std::error::Error for WorkbookError {}

impl From<std::io::Error> for WorkbookError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<zip::result::ZipError> for WorkbookError {
    fn from(error: zip::result::ZipError) -> Self {
        Self::Zip(error)
    }
}

/// An open workbook, together with the package bytes it was loaded from.
#[derive(Debug)]
pub struct Workbook {
    pub sheets: Vec<Sheet>,
    /// Whether date serials count from 1904 rather than 1900.
    pub date1904: bool,
    /// The file this workbook belongs to, which a blank one doesn't have yet.
    path: Option<PathBuf>,
    source: Arc<Vec<u8>>,
    layout: reader::Layout,
}

impl Workbook {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, WorkbookError> {
        let path = path.as_ref().to_path_buf();
        let source = std::fs::read(&path)?;
        let (sheets, date1904, layout) = reader::read(&source)?;
        Ok(Self {
            sheets,
            date1904,
            path: Some(path),
            source: Arc::new(source),
            layout,
        })
    }

    /// Opens a workbook whose sheets start out empty, with their cells on the way through the
    /// returned load, preferring the given sheet. A file that isn't a workbook fails here, and a
    /// problem further in arrives as an event. It mustn't be saved until the load has ended.
    pub fn open_loading(
        path: impl AsRef<Path>,
        preferred_sheet: usize,
    ) -> Result<(Self, WorkbookLoad), WorkbookError> {
        let path = path.as_ref().to_path_buf();
        let source = Arc::new(std::fs::read(&path)?);
        let plan = reader::read_plan(&source)?;
        let load = WorkbookLoad::start(Arc::clone(&source), &plan, preferred_sheet);
        let workbook = Self {
            sheets: plan.sheet_names.iter().map(Sheet::new).collect(),
            date1904: plan.date1904,
            path: Some(path),
            source,
            layout: plan.layout,
        };
        Ok((workbook, load))
    }

    /// An empty one-sheet workbook that isn't tied to a file yet.
    pub fn blank() -> Result<Self, WorkbookError> {
        let source = writer::blank_package()?;
        let (sheets, date1904, layout) = reader::read(&source)?;
        Ok(Self {
            sheets,
            date1904,
            path: None,
            source: Arc::new(source),
            layout,
        })
    }

    pub fn is_dirty(&self) -> bool {
        self.sheets.iter().any(Sheet::is_dirty)
    }

    /// Starts writing edits back to the file this workbook was opened from. The workbook mustn't
    /// change until the save has ended, and counts as unsaved until `finish_save` takes it in.
    pub fn begin_save(&self) -> Result<WorkbookSave, WorkbookError> {
        let path = self.path.clone().ok_or(WorkbookError::NoFileName)?;
        Ok(self.begin_writing(path, Target::Own))
    }

    /// Starts writing the workbook to a new file, as `begin_save` does.
    pub fn begin_save_as(&self, path: impl AsRef<Path>) -> WorkbookSave {
        self.begin_writing(path.as_ref().to_path_buf(), Target::New)
    }

    /// Starts writing the workbook over an existing file, as `begin_save` does.
    pub fn begin_save_over(&self, path: impl AsRef<Path>) -> WorkbookSave {
        self.begin_writing(path.as_ref().to_path_buf(), Target::Over)
    }

    fn begin_writing(&self, path: PathBuf, target: Target) -> WorkbookSave {
        WorkbookSave::start(SaveJob {
            source: Arc::clone(&self.source),
            layout: self.layout.clone(),
            sheets: self.sheets.clone(),
            path,
            target,
        })
    }

    /// Records that the workbook now lives where it was saved, holding what was written there.
    pub fn finish_save(&mut self, saved: SavedPackage) {
        self.path = Some(saved.path);
        self.source = saved.bytes;
        self.sheets.iter_mut().for_each(Sheet::mark_saved);
    }

    /// Writes edits back to the file this workbook was opened from, and waits.
    pub fn save(&mut self) -> Result<(), WorkbookError> {
        let saved = self.begin_save()?.wait()?;
        self.finish_save(saved);
        Ok(())
    }

    /// Writes the workbook to a new file, which it then belongs to, and waits.
    pub fn save_as(&mut self, path: impl AsRef<Path>) -> Result<(), WorkbookError> {
        let saved = self.begin_save_as(path).wait()?;
        self.finish_save(saved);
        Ok(())
    }

    /// Writes the workbook over an existing file, which it then belongs to, and waits.
    pub fn save_over(&mut self, path: impl AsRef<Path>) -> Result<(), WorkbookError> {
        let saved = self.begin_save_over(path).wait()?;
        self.finish_save(saved);
        Ok(())
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }
}
