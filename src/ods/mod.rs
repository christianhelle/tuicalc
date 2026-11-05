//! Reading OpenDocument spreadsheets.

pub(crate) mod reader;

use std::io::{Cursor, Read};

use zip::ZipArchive;

use crate::workbook::WorkbookError;

const SPREADSHEET_MIMETYPE: &str = "application/vnd.oasis.opendocument.spreadsheet";

/// Where a workbook's content is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    /// The `content.xml` of a zip package, as in `.ods` and `.ots`.
    Package,
}

/// How to read `source` if it's an OpenDocument spreadsheet.
pub fn detect(source: &[u8]) -> Result<Option<Layout>, WorkbookError> {
    let Ok(mut archive) = ZipArchive::new(Cursor::new(source)) else {
        return Ok(None);
    };
    let Ok(mut file) = archive.by_name("mimetype") else {
        return Ok(None);
    };
    let mut mimetype = String::new();
    file.read_to_string(&mut mimetype)?;
    Ok(mimetype
        .starts_with(SPREADSHEET_MIMETYPE)
        .then_some(Layout::Package))
}
