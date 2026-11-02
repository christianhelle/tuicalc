//! Reading and patching .xlsx packages.

mod loading;
mod reader;
mod writer;
mod xml;

use std::fmt;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::model::Sheet;

pub use loading::{LoadEvent, SheetQueue, Waited, WorkbookLoad};
pub use reader::SheetChunk;

#[derive(Debug)]
pub enum XlsxError {
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

impl fmt::Display for XlsxError {
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

impl std::error::Error for XlsxError {}

impl From<std::io::Error> for XlsxError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<zip::result::ZipError> for XlsxError {
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
    pub fn open(path: impl AsRef<Path>) -> Result<Self, XlsxError> {
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
    ) -> Result<(Self, WorkbookLoad), XlsxError> {
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
    pub fn blank() -> Result<Self, XlsxError> {
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

    /// Writes edits back to the file this workbook was opened from.
    pub fn save(&mut self) -> Result<(), XlsxError> {
        let Some(path) = &self.path else {
            return Err(XlsxError::NoFileName);
        };
        if !self.is_dirty() {
            return Ok(());
        }
        if std::fs::read(path)? != *self.source {
            return Err(XlsxError::ChangedOnDisk);
        }
        let patched = writer::patch(&self.source, &self.layout, &self.sheets)?;
        replace_file(path, &patched)?;
        self.source = Arc::new(patched);
        self.sheets.iter_mut().for_each(Sheet::mark_saved);
        Ok(())
    }

    /// Writes the workbook to a new file, which it then belongs to.
    pub fn save_as(&mut self, path: impl AsRef<Path>) -> Result<(), XlsxError> {
        let bytes = self.current_package()?;
        create_file(path.as_ref(), &bytes)?;
        self.adopt(path.as_ref(), bytes);
        Ok(())
    }

    /// Writes the workbook over an existing file, which it then belongs to.
    pub fn save_over(&mut self, path: impl AsRef<Path>) -> Result<(), XlsxError> {
        let bytes = self.current_package()?;
        replace_file(path.as_ref(), &bytes)?;
        self.adopt(path.as_ref(), bytes);
        Ok(())
    }

    /// The package as it would be saved now.
    fn current_package(&self) -> Result<Vec<u8>, XlsxError> {
        if self.is_dirty() {
            writer::patch(&self.source, &self.layout, &self.sheets)
        } else {
            Ok((*self.source).clone())
        }
    }

    /// Records that the workbook now lives in `path`, holding `bytes`.
    fn adopt(&mut self, path: &Path, bytes: Vec<u8>) {
        self.path = Some(path.to_path_buf());
        self.source = Arc::new(bytes);
        self.sheets.iter_mut().for_each(Sheet::mark_saved);
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }
}

/// Creates `path`, refusing a file that already exists, and removes it if the write fails.
fn create_file(path: &Path, bytes: &[u8]) -> Result<(), XlsxError> {
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    if let Err(error) = file.write_all(bytes).and_then(|()| file.sync_all()) {
        drop(file);
        let _ = std::fs::remove_file(path);
        return Err(error.into());
    }
    Ok(())
}

/// Replaces `path` atomically, so a failed save cannot leave a truncated workbook behind.
fn replace_file(path: &Path, bytes: &[u8]) -> Result<(), XlsxError> {
    let target = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let directory = target
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty());
    let mut temporary = tempfile::NamedTempFile::new_in(directory.unwrap_or(Path::new(".")))?;
    temporary.write_all(bytes)?;
    temporary.as_file().sync_all()?;
    if let Ok(metadata) = std::fs::metadata(&target) {
        std::fs::set_permissions(temporary.path(), metadata.permissions())?;
    }
    temporary
        .persist(&target)
        .map_err(|error| XlsxError::Io(error.error))?;
    Ok(())
}
