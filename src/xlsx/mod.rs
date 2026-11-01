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
}

impl fmt::Display for XlsxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "{error}"),
            Self::Zip(error) => write!(f, "not a readable .xlsx package: {error}"),
            Self::Xml(message) => write!(f, "malformed XML: {message}"),
            Self::Invalid(message) => write!(f, "invalid workbook: {message}"),
            Self::ChangedOnDisk => write!(f, "the file changed on disk since it was opened"),
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
    path: PathBuf,
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
            path,
            source,
            layout,
        })
    }

    pub fn is_dirty(&self) -> bool {
        self.sheets.iter().any(Sheet::is_dirty)
    }

    /// Writes edits back to the file this workbook was opened from.
    pub fn save(&mut self) -> Result<(), XlsxError> {
        if !self.is_dirty() {
            return Ok(());
        }
        if std::fs::read(&self.path)? != self.source {
            return Err(XlsxError::ChangedOnDisk);
        }
        let patched = writer::patch(&self.source, &self.layout, &self.sheets)?;
        replace_file(&self.path, &patched)?;
        self.source = patched;
        self.sheets.iter_mut().for_each(Sheet::mark_saved);
        Ok(())
    }

    pub fn path(&self) -> &Path {
        &self.path
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
