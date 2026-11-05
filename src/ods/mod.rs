//! Reading OpenDocument spreadsheets.

mod formula;
pub(crate) mod reader;

use std::io::{Cursor, Read};

use quick_xml::Reader;
use quick_xml::events::Event;
use zip::ZipArchive;

use crate::workbook::WorkbookError;
use crate::workbook::xml::{attr, local_name};

const OPENDOCUMENT_MIMETYPE: &str = "application/vnd.oasis.opendocument.";
const SPREADSHEET_MIMETYPE: &str = "application/vnd.oasis.opendocument.spreadsheet";
const MANIFEST_PART: &str = "META-INF/manifest.xml";

/// Where a workbook's content is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    /// The `content.xml` of a zip package, as in `.ods` and `.ots`.
    Package,
    /// The whole file, as in `.fods`.
    Flat,
}

/// How to read `source` if it's an OpenDocument spreadsheet, and an error if it's OpenDocument but
/// can't be read as one.
pub fn detect(source: &[u8]) -> Result<Option<Layout>, WorkbookError> {
    let Ok(mut archive) = ZipArchive::new(Cursor::new(source)) else {
        return flat_mimetype(source).map_or(Ok(None), |mimetype| {
            check_mimetype(&mimetype).map(|known| known.then_some(Layout::Flat))
        });
    };
    let mimetype = match archive.by_name("mimetype") {
        Ok(mut file) => {
            let mut mimetype = String::new();
            file.read_to_string(&mut mimetype)?;
            mimetype
        }
        Err(_) => return Ok(None),
    };
    if !check_mimetype(&mimetype)? {
        return Ok(None);
    }
    if let Ok(mut file) = archive.by_name(MANIFEST_PART) {
        let mut manifest = String::new();
        file.read_to_string(&mut manifest)?;
        if manifest.contains(":encryption-data") {
            return Err(WorkbookError::Invalid(
                "the file is protected by a password".into(),
            ));
        }
    }
    Ok(Some(Layout::Package))
}

/// Whether a mimetype is that of a spreadsheet, and an error if it's another OpenDocument type.
fn check_mimetype(mimetype: &str) -> Result<bool, WorkbookError> {
    if mimetype.starts_with(SPREADSHEET_MIMETYPE) {
        Ok(true)
    } else if mimetype.starts_with(OPENDOCUMENT_MIMETYPE) {
        Err(WorkbookError::Invalid(format!(
            "not a spreadsheet: {mimetype}"
        )))
    } else {
        Ok(false)
    }
}

/// The mimetype declared by the root of a flat OpenDocument file.
fn flat_mimetype(source: &[u8]) -> Option<String> {
    let mut reader = Reader::from_reader(source);
    let mut buffer = Vec::new();
    loop {
        buffer.clear();
        match reader.read_event_into(&mut buffer).ok()? {
            Event::Start(element) | Event::Empty(element) => {
                return (local_name(&element) == "document")
                    .then(|| attr(&element, "mimetype"))
                    .flatten();
            }
            Event::Eof => return None,
            _ => {}
        }
    }
}
