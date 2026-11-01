//! Loads a package into the model.

use std::collections::HashMap;
use std::io::{Cursor, Read};

use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};
use zip::ZipArchive;

use super::XlsxError;
use super::xml::{attr, decode_excel_escapes, local_name, push_text, xml_error};
use crate::address::CellRef;
use crate::formula::translate;
use crate::model::{Cell, CellValue, Sheet};

const OFFICE_DOCUMENT: &str = "/officeDocument";
const WORKSHEET: &str = "/worksheet";
const SHARED_STRINGS: &str = "/sharedStrings";
const STYLES: &str = "/styles";

/// Where the parts that saving needs live inside the package.
#[derive(Debug, Clone)]
pub struct Layout {
    pub workbook_part: String,
    pub sheet_parts: Vec<String>,
}

type Archive<'a> = ZipArchive<Cursor<&'a [u8]>>;

pub fn read(source: &[u8]) -> Result<(Vec<Sheet>, bool, Layout), XlsxError> {
    let mut archive = ZipArchive::new(Cursor::new(source))?;
    let workbook_part = find_workbook_part(&mut archive)?;
    let relationships = read_relationships(&mut archive, &workbook_part)?;
    let (declared, date1904) = read_sheet_list(&mut archive, &workbook_part)?;
    let date_styles = match relationships.values().find(|r| r.kind.ends_with(STYLES)) {
        Some(relationship) => match read_part(&mut archive, &relationship.target)? {
            Some(xml) => read_date_styles(&xml)?,
            None => Vec::new(),
        },
        None => Vec::new(),
    };
    let shared_strings = match relationships
        .values()
        .find(|r| r.kind.ends_with(SHARED_STRINGS))
    {
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
            return Err(XlsxError::Invalid(format!(
                "sheet {name} has no relationship"
            )));
        };
        if !relationship.kind.ends_with(WORKSHEET) {
            continue;
        }
        let xml = read_part(&mut archive, &relationship.target)?
            .ok_or_else(|| XlsxError::Invalid(format!("missing {}", relationship.target)))?;
        sheets.push(read_sheet(&name, &xml, &shared_strings, &date_styles)?);
        sheet_parts.push(relationship.target.clone());
    }
    if sheets.is_empty() {
        return Err(XlsxError::Invalid("no worksheets".into()));
    }
    Ok((
        sheets,
        date1904,
        Layout {
            workbook_part,
            sheet_parts,
        },
    ))
}

pub struct Relationship {
    pub kind: String,
    /// The target as a path from the package root.
    pub target: String,
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

/// The path of the part holding `part`'s relationships.
pub fn relationships_part(part: &str) -> String {
    match part.rsplit_once('/') {
        Some((directory, file)) => format!("{directory}/_rels/{file}.rels"),
        None => format!("_rels/{part}.rels"),
    }
}

fn read_relationships(
    archive: &mut Archive<'_>,
    part: &str,
) -> Result<HashMap<String, Relationship>, XlsxError> {
    let directory = part.rsplit_once('/').map_or("", |(directory, _)| directory);
    match read_part(archive, &relationships_part(part))? {
        Some(xml) => parse_relationships(&xml, directory),
        None => Ok(HashMap::new()),
    }
}

pub fn parse_relationships(
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
    let mut segments: Vec<&str> = base_directory
        .split('/')
        .filter(|s| !s.is_empty())
        .collect();
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
) -> Result<(Vec<(String, String)>, bool), XlsxError> {
    let xml = read_part(archive, workbook_part)?
        .ok_or_else(|| XlsxError::Invalid(format!("missing {workbook_part}")))?;
    let mut reader = Reader::from_str(&xml);
    let mut sheets = Vec::new();
    let mut date1904 = false;
    loop {
        match reader.read_event().map_err(xml_error)? {
            Event::Start(element) | Event::Empty(element) if local_name(&element) == "sheet" => {
                if let (Some(name), Some(id)) = (attr(&element, "name"), attr(&element, "id")) {
                    sheets.push((name, id));
                }
            }
            Event::Start(element) | Event::Empty(element)
                if local_name(&element) == "workbookPr" =>
            {
                date1904 = matches!(attr(&element, "date1904").as_deref(), Some("1" | "true"));
            }
            Event::Eof => return Ok((sheets, date1904)),
            _ => {}
        }
    }
}

/// Whether each entry of `cellXfs` uses a date format.
fn read_date_styles(xml: &str) -> Result<Vec<bool>, XlsxError> {
    let mut reader = Reader::from_str(xml);
    let mut custom_formats: HashMap<u32, String> = HashMap::new();
    let mut styles = Vec::new();
    let mut in_cell_xfs = false;
    loop {
        match reader.read_event().map_err(xml_error)? {
            Event::Start(element) | Event::Empty(element) => match local_name(&element).as_str() {
                "numFmt" => {
                    let id = attr(&element, "numFmtId").and_then(|id| id.parse().ok());
                    if let (Some(id), Some(code)) = (id, attr(&element, "formatCode")) {
                        custom_formats.insert(id, code);
                    }
                }
                "cellXfs" => in_cell_xfs = true,
                "xf" if in_cell_xfs => {
                    let id: u32 = attr(&element, "numFmtId")
                        .and_then(|id| id.parse().ok())
                        .unwrap_or(0);
                    styles.push(match custom_formats.get(&id) {
                        Some(code) => is_date_format_code(code),
                        None => is_builtin_date_format(id),
                    });
                }
                _ => {}
            },
            Event::End(element) if AsRef::<str>::as_ref(&element.local_name()) == "cellXfs" => {
                in_cell_xfs = false;
            }
            Event::Eof => return Ok(styles),
            _ => {}
        }
    }
}

fn is_builtin_date_format(id: u32) -> bool {
    matches!(id, 14..=22 | 27..=36 | 45..=47 | 50..=58)
}

/// Looks for date tokens outside quoted literals, escapes and `[...]` sections.
fn is_date_format_code(code: &str) -> bool {
    let mut chars = code.chars();
    let mut has_date_token = false;
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                chars.by_ref().find(|&next| next == '"');
            }
            '\\' | '_' | '*' => {
                chars.next();
            }
            '[' => {
                chars.by_ref().find(|&next| next == ']');
            }
            'y' | 'Y' | 'm' | 'M' | 'd' | 'D' | 'h' | 'H' | 's' | 'S' => has_date_token = true,
            _ => {}
        }
    }
    has_date_token
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
                "si" => strings.push(decode_excel_escapes(&std::mem::take(&mut current))),
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
    shared_index: Option<String>,
    style: usize,
}

fn parse_range(text: &str) -> Option<(CellRef, CellRef)> {
    let (first, last) = text.split_once(':').unwrap_or((text, text));
    Some((CellRef::parse(first)?, CellRef::parse(last)?))
}

impl Draft {
    /// Registers a shared formula's master, or expands a dependent from its master.
    fn resolve_formula(&mut self, at: CellRef, shared: &mut HashMap<String, (CellRef, String)>) {
        let text = self.formula.take().filter(|text| !text.is_empty());
        self.formula = match (self.shared_index.take(), text) {
            (Some(index), Some(text)) => {
                shared.insert(index, (at, text.clone()));
                Some(text)
            }
            (Some(index), None) => shared.get(&index).map(|(origin, text)| {
                let cols = i64::from(at.col) - i64::from(origin.col);
                let rows = i64::from(at.row) - i64::from(origin.row);
                translate(text, cols, rows)
            }),
            (None, text) => text,
        };
    }

    fn start(element: &BytesStart<'_>, fallback: CellRef) -> Self {
        let at = attr(element, "r")
            .and_then(|r| CellRef::parse(&r))
            .unwrap_or(fallback);
        Self {
            at: Some(at),
            kind: attr(element, "t").unwrap_or_default(),
            style: attr(element, "s").and_then(|s| s.parse().ok()).unwrap_or(0),
            ..Self::default()
        }
    }

    fn into_cell(self, shared_strings: &[String], date_styles: &[bool]) -> Cell {
        let is_date = date_styles.get(self.style).copied().unwrap_or(false);
        let value = match self.kind.as_str() {
            "s" => self
                .value
                .parse::<usize>()
                .ok()
                .and_then(|index| shared_strings.get(index).cloned())
                .map_or(CellValue::Empty, CellValue::Text),
            "b" => CellValue::Bool(self.value == "1"),
            "e" => CellValue::Error(self.value),
            "inlineStr" => CellValue::Text(decode_excel_escapes(&self.inline)),
            "str" | "d" => CellValue::Text(decode_excel_escapes(&self.value)),
            _ if self.value.is_empty() => CellValue::Empty,
            _ => self
                .value
                .parse()
                .map_or_else(|_| CellValue::Text(self.value), CellValue::Number),
        };
        Cell {
            is_date: is_date && matches!(value, CellValue::Number(_)),
            value,
            formula: self.formula,
        }
    }
}

fn read_sheet(
    name: &str,
    xml: &str,
    shared_strings: &[String],
    date_styles: &[bool],
) -> Result<Sheet, XlsxError> {
    let mut sheet = Sheet::new(name);
    let mut reader = Reader::from_str(xml);
    let mut draft = Draft::default();
    let mut capture = Capture::Nothing;
    let mut in_phonetic = false;
    // Rows and cells may omit `r`, in which case they follow their predecessor.
    let mut next = CellRef { col: 0, row: 0 };
    let mut row_seen = false;
    let mut shared_formulas: HashMap<String, (CellRef, String)> = HashMap::new();
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
                        draft.shared_index = attr(element, "si")
                            .filter(|_| attr(element, "t").as_deref() == Some("shared"));
                        if matches!(attr(element, "t").as_deref(), Some("array" | "dataTable")) {
                            let region = attr(element, "ref").unwrap_or_default();
                            if let Some((first, last)) = parse_range(&region) {
                                sheet.lock_range(first, last);
                            }
                        }
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
                    let mut finished = std::mem::take(&mut draft);
                    if let Some(at) = finished.at {
                        finished.resolve_formula(at, &mut shared_formulas);
                        sheet.load_cell(at, finished.into_cell(shared_strings, date_styles));
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
