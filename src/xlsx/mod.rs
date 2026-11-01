//! Reading and patching .xlsx packages.

mod reader;
mod xml;

use std::fmt;
use std::path::{Path, PathBuf};

use crate::model::Sheet;

#[derive(Debug)]
pub enum XlsxError {
    Io(std::io::Error),
    Zip(zip::result::ZipError),
    Xml(String),
    /// A required part or relationship is absent.
    Invalid(String),
}

impl fmt::Display for XlsxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "{error}"),
            Self::Zip(error) => write!(f, "not a readable .xlsx package: {error}"),
            Self::Xml(message) => write!(f, "malformed XML: {message}"),
            Self::Invalid(message) => write!(f, "invalid workbook: {message}"),
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
    path: PathBuf,
    #[allow(dead_code)]
    source: Vec<u8>,
    #[allow(dead_code)]
    layout: reader::Layout,
}

impl Workbook {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, XlsxError> {
        let path = path.as_ref().to_path_buf();
        let source = std::fs::read(&path)?;
        let (sheets, layout) = reader::read(&source)?;
        Ok(Self { sheets, path, source, layout })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}
