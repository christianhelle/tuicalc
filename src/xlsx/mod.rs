//! Reading and patching .xlsx packages.

mod reader;
mod writer;
mod xml;

use std::fmt;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::model::Sheet;

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
    source: Vec<u8>,
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
            source,
            layout,
        })
    }

    /// An empty one-sheet workbook that isn't tied to a file yet.
    pub fn blank() -> Result<Self, XlsxError> {
        let source = writer::blank_package()?;
        let (sheets, date1904, layout) = reader::read(&source)?;
        Ok(Self {
            sheets,
            date1904,
            path: None,
            source,
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
        if std::fs::read(path)? != self.source {
            return Err(XlsxError::ChangedOnDisk);
        }
        let patched = writer::patch(&self.source, &self.layout, &self.sheets)?;
        replace_file(path, &patched)?;
        self.source = patched;
        self.sheets.iter_mut().for_each(Sheet::mark_saved);
        Ok(())
    }

    /// Writes the workbook to a new file, which it then belongs to.
    pub fn save_as(&mut self, path: impl AsRef<Path>) -> Result<(), XlsxError> {
        let bytes = self.current_package()?;
        std::fs::write(&path, &bytes)?;
        self.adopt(path.as_ref(), bytes);
        Ok(())
    }

    /// The package as it would be saved now.
    fn current_package(&self) -> Result<Vec<u8>, XlsxError> {
        if self.is_dirty() {
            writer::patch(&self.source, &self.layout, &self.sheets)
        } else {
            Ok(self.source.clone())
        }
    }

    /// Records that the workbook now lives in `path`, holding `bytes`.
    fn adopt(&mut self, path: &Path, bytes: Vec<u8>) {
        self.path = Some(path.to_path_buf());
        self.source = bytes;
        self.sheets.iter_mut().for_each(Sheet::mark_saved);
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }
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
