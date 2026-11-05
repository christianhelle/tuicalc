//! Reading OpenDocument spreadsheets.

mod formula;
pub(crate) mod reader;
pub(crate) mod writer;

use std::io::{Cursor, Read};
use std::path::Path;

use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};
use zip::ZipArchive;

use crate::workbook::WorkbookError;
use crate::workbook::xml::{attr, local_name};

const OPENDOCUMENT_MIMETYPE: &str = "application/vnd.oasis.opendocument.";
const SPREADSHEET_MIMETYPE: &str = "application/vnd.oasis.opendocument.spreadsheet";
const MANIFEST_PART: &str = "META-INF/manifest.xml";

const OFFICE_NAMESPACE: &str = "urn:oasis:names:tc:opendocument:xmlns:office:1.0";
const TEXT_NAMESPACE: &str = "urn:oasis:names:tc:opendocument:xmlns:text:1.0";
const CALCEXT_NAMESPACE: &str =
    "urn:org:documentfoundation:names:experimental:calc:xmlns:calcext:1.0";

/// The prefix a root element declares for a namespace.
fn declared_prefix(root: &BytesStart<'_>, namespace: &str) -> Option<String> {
    root.attributes()
        .with_checks(false)
        .flatten()
        .find_map(|attribute| {
            let key: &str = attribute.key.as_ref();
            let prefix = key.strip_prefix("xmlns:")?;
            (attribute.value.as_ref() == namespace).then(|| prefix.to_string())
        })
}

/// Where a workbook's content is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    /// The `content.xml` of a zip package, as in `.ods` and `.ots`.
    Package,
    /// The whole file, as in `.fods`.
    Flat,
}

/// What a file's name says it holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Spreadsheet,
    Template,
    Flat,
}

impl Kind {
    pub fn of_path(path: &Path) -> Option<Self> {
        let extension = path.extension()?.to_string_lossy().to_ascii_lowercase();
        match extension.as_str() {
            "ods" => Some(Self::Spreadsheet),
            "ots" => Some(Self::Template),
            "fods" => Some(Self::Flat),
            _ => None,
        }
    }

    pub fn layout(self) -> Layout {
        match self {
            Self::Spreadsheet | Self::Template => Layout::Package,
            Self::Flat => Layout::Flat,
        }
    }
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
