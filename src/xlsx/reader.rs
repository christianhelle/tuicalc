//! Loads a package into the model.

use std::collections::HashMap;
use std::io::{Cursor, Read};

use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};
use zip::ZipArchive;

use super::XlsxError;
use super::xml::{attr, local_name, push_text, xml_error};
use crate::address::CellRef;
use crate::model::{Cell, CellValue, Sheet};

const OFFICE_DOCUMENT: &str = "/officeDocument";
const WORKSHEET: &str = "/worksheet";
const SHARED_STRINGS: &str = "/sharedStrings";

/// Where the parts that saving needs live inside the package.
#[derive(Debug, Clone)]
pub struct Layout {
    pub workbook_part: String,
    pub sheet_parts: Vec<String>,
}

type Archive<'a> = ZipArchive<Cursor<&'a [u8]>>;

pub fn read(source: &[u8]) -> Result<(Vec<Sheet>, Layout), XlsxError> {
    let mut archive = ZipArchive::new(Cursor::new(source))?;
    let workbook_part = find_workbook_part(&mut archive)?;
    let relationships = read_relationships(&mut archive, &workbook_part)?;
    let declared = read_sheet_list(&mut archive, &workbook_part)?;
    let shared_strings = match relationships.values().find(|r| r.kind.ends_with(SHARED_STRINGS)) {
        Some(relationship) => match read_part(&mut archive, &relationship.target)? {
            Some(xml) => read_shared_strings(&xml)?,
            None => Vec::new(),
        },
        None => Vec::new(),
    };

    let mut sheets = Vec::new();
    let mut sheet_parts = Vec::new();
    for (name, relationship_id) in declared {
        let Some(relationship) = relationships.get(&relationship_id) else {
            return Err(XlsxError::Invalid(format!("sheet {name} has no relationship")));
        };
        if !relationship.kind.ends_with(WORKSHEET) {
            continue;
        }
        let xml = read_part(&mut archive, &relationship.target)?
            .ok_or_else(|| XlsxError::Invalid(format!("missing {}", relationship.target)))?;
        sheets.push(read_sheet(&name, &xml, &shared_strings)?);
        sheet_parts.push(relationship.target.clone());
    }
    if sheets.is_empty() {
        return Err(XlsxError::Invalid("no worksheets".into()));
    }
    Ok((sheets, Layout { workbook_part, sheet_parts }))
}

struct Relationship {
    kind: String,
    /// The target as a path from the package root.
    target: String,
}

fn read_part(archive: &mut Archive<'_>, name: &str) -> Result<Option<String>, XlsxError> {
    let Ok(mut file) = archive.by_name(name) else {
        return Ok(None);
    };
    let mut content = String::new();
    file.read_to_string(&mut content)?;
    Ok(Some(content))
}

fn find_workbook_part(archive: &mut Archive<'_>) -> Result<String, XlsxError> {
    let xml = read_part(archive, "_rels/.rels")?
        .ok_or_else(|| XlsxError::Invalid("missing _rels/.rels".into()))?;
    parse_relationships(&xml, "")?
        .into_values()
        .find(|relationship| relationship.kind.ends_with(OFFICE_DOCUMENT))
        .map(|relationship| relationship.target)
        .ok_or_else(|| XlsxError::Invalid("no workbook relationship".into()))
}

fn read_relationships(
    archive: &mut Archive<'_>,
    part: &str,
) -> Result<HashMap<String, Relationship>, XlsxError> {
    let (directory, file) = part.rsplit_once('/').unwrap_or(("", part));
    let rels_name = if directory.is_empty() {
        format!("_rels/{file}.rels")
    } else {
        format!("{directory}/_rels/{file}.rels")
    };
    match read_part(archive, &rels_name)? {
        Some(xml) => parse_relationships(&xml, directory),
        None => Ok(HashMap::new()),
    }
}

fn parse_relationships(
    xml: &str,
    base_directory: &str,
) -> Result<HashMap<String, Relationship>, XlsxError> {
    let mut reader = Reader::from_str(xml);
    let mut relationships = HashMap::new();
    loop {
        match reader.read_event().map_err(xml_error)? {
            Event::Start(element) | Event::Empty(element)
                if local_name(&element) == "Relationship" =>
            {
                let (Some(id), Some(kind), Some(target)) = (
                    attr(&element, "Id"),
                    attr(&element, "Type"),
                    attr(&element, "Target"),
                ) else {
                    continue;
                };
                let target = resolve_target(base_directory, &target);
                relationships.insert(id, Relationship { kind, target });
            }
            Event::Eof => return Ok(relationships),
            _ => {}
        }
    }
}

/// Resolves a relationship target against the directory of its source part.
fn resolve_target(base_directory: &str, target: &str) -> String {
    if let Some(absolute) = target.strip_prefix('/') {
        return absolute.to_string();
    }
    let mut segments: Vec<&str> = base_directory.split('/').filter(|s| !s.is_empty()).collect();
    for segment in target.split('/') {
        match segment {
            "." | "" => {}
            ".." => {
                segments.pop();
            }
            other => segments.push(other),
        }
    }
    segments.join("/")
}

fn read_sheet_list(
    archive: &mut Archive<'_>,
    workbook_part: &str,
) -> Result<Vec<(String, String)>, XlsxError> {
    let xml = read_part(archive, workbook_part)?
        .ok_or_else(|| XlsxError::Invalid(format!("missing {workbook_part}")))?;
    let mut reader = Reader::from_str(&xml);
    let mut sheets = Vec::new();
    loop {
        match reader.read_event().map_err(xml_error)? {
            Event::Start(element) | Event::Empty(element) if local_name(&element) == "sheet" => {
                if let (Some(name), Some(id)) = (attr(&element, "name"), attr(&element, "id")) {
                    sheets.push((name, id));
                }
            }
            Event::Eof => return Ok(sheets),
            _ => {}
        }
    }
}

/// Reads all `<si>` entries; rich text runs are joined and phonetic hints dropped.
fn read_shared_strings(xml: &str) -> Result<Vec<String>, XlsxError> {
    let mut reader = Reader::from_str(xml);
    let mut strings = Vec::new();
    let mut current = String::new();
    let mut in_text = false;
    let mut in_phonetic = false;
    loop {
        let event = reader.read_event().map_err(xml_error)?;
        match &event {
            Event::Start(element) => match local_name(element).as_str() {
                "si" => current.clear(),
                "rPh" => in_phonetic = true,
                "t" => in_text = !in_phonetic,
                _ => {}
            },
            Event::Empty(element) if local_name(element) == "si" => strings.push(String::new()),
            Event::End(element) => match AsRef::<str>::as_ref(&element.local_name()) {
                "si" => strings.push(std::mem::take(&mut current)),
                "rPh" => in_phonetic = false,
                "t" => in_text = false,
                _ => {}
            },
            Event::Eof => return Ok(strings),
            _ if in_text => push_text(&event, &mut current)?,
            _ => {}
        }
    }
}

/// Which element's text is currently being collected.
#[derive(Clone, Copy, PartialEq)]
enum Capture {
    Nothing,
    Value,
    Formula,
    Inline,
}

/// The `<c>` element being read.
#[derive(Default)]
struct Draft {
    at: Option<CellRef>,
    kind: String,
    formula: Option<String>,
    value: String,
    inline: String,
}

impl Draft {
    fn start(element: &BytesStart<'_>, fallback: CellRef) -> Self {
        let at = attr(element, "r").and_then(|r| CellRef::parse(&r)).unwrap_or(fallback);
        Self {
            at: Some(at),
            kind: attr(element, "t").unwrap_or_default(),
            ..Self::default()
        }
    }

    fn into_cell(self, shared_strings: &[String]) -> Cell {
        let value = match self.kind.as_str() {
            "s" => self
                .value
                .parse::<usize>()
                .ok()
                .and_then(|index| shared_strings.get(index).cloned())
                .map_or(CellValue::Empty, CellValue::Text),
            "b" => CellValue::Bool(self.value == "1"),
            "e" => CellValue::Error(self.value),
            "inlineStr" => CellValue::Text(self.inline),
            "str" | "d" => CellValue::Text(self.value),
            _ if self.value.is_empty() => CellValue::Empty,
            _ => self
                .value
                .parse()
                .map_or_else(|_| CellValue::Text(self.value), CellValue::Number),
        };
        Cell { value, formula: self.formula }
    }
}

fn read_sheet(name: &str, xml: &str, shared_strings: &[String]) -> Result<Sheet, XlsxError> {
    let mut sheet = Sheet::new(name);
    let mut reader = Reader::from_str(xml);
    let mut draft = Draft::default();
    let mut capture = Capture::Nothing;
    let mut in_phonetic = false;
    // Rows and cells may omit `r`, in which case they follow their predecessor.
    let mut next = CellRef { col: 0, row: 0 };
    let mut row_seen = false;
    loop {
        let event = reader.read_event().map_err(xml_error)?;
        match &event {
            Event::Start(element) | Event::Empty(element) => {
                let empty = matches!(event, Event::Empty(_));
                match local_name(element).as_str() {
                    "row" => {
                        next.row = attr(element, "r")
                            .and_then(|r| r.parse::<u32>().ok())
                            .and_then(|r| r.checked_sub(1))
                            .unwrap_or(if row_seen { next.row + 1 } else { 0 });
                        next.col = 0;
                        row_seen = true;
                    }
                    "c" => {
                        draft = Draft::start(element, next);
                        next.col = draft.at.map_or(next.col, |at| at.col) + 1;
                        if empty {
                            draft = Draft::default();
                        }
                    }
                    "v" if !empty => capture = Capture::Value,
                    "f" => {
                        draft.formula = Some(String::new());
                        if !empty {
                            capture = Capture::Formula;
                        }
                    }
                    "rPh" => in_phonetic = !empty,
                    "t" if !empty && !in_phonetic => capture = Capture::Inline,
                    _ => {}
                }
            }
            Event::End(element) => match AsRef::<str>::as_ref(&element.local_name()) {
                "v" | "f" | "t" => capture = Capture::Nothing,
                "rPh" => in_phonetic = false,
                "c" => {
                    let finished = std::mem::take(&mut draft);
                    if let Some(at) = finished.at {
                        sheet.load_cell(at, finished.into_cell(shared_strings));
                    }
                }
                _ => {}
            },
            Event::Eof => return Ok(sheet),
            _ => match capture {
                Capture::Nothing => {}
                Capture::Value => push_text(&event, &mut draft.value)?,
                Capture::Inline => push_text(&event, &mut draft.inline)?,
                Capture::Formula => {
                    if let Some(formula) = &mut draft.formula {
                        push_text(&event, formula)?;
                    }
                }
            },
        }
    }
}
