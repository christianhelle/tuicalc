//! Loads a package into the model.

use std::collections::HashMap;
use std::io::{Cursor, Read};

use quick_xml::Reader;
use quick_xml::events::Event;
use zip::ZipArchive;

use super::XlsxError;
use super::xml::{attr, local_name, push_text, xml_error};
use crate::address::CellRef;
use crate::model::{Cell, CellValue, Sheet};

const OFFICE_DOCUMENT: &str = "/officeDocument";
const WORKSHEET: &str = "/worksheet";

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
        sheets.push(read_sheet(&name, &xml)?);
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

fn read_sheet(name: &str, xml: &str) -> Result<Sheet, XlsxError> {
    let mut sheet = Sheet::new(name);
    let mut reader = Reader::from_str(xml);
    let mut position = None;
    let mut in_value = false;
    let mut raw = String::new();
    loop {
        let event = reader.read_event().map_err(xml_error)?;
        match &event {
            Event::Start(element) => match local_name(element).as_str() {
                "c" => {
                    position = attr(element, "r").and_then(|r| CellRef::parse(&r));
                    raw.clear();
                }
                "v" => in_value = true,
                _ => {}
            },
            Event::End(element) => match AsRef::<str>::as_ref(&element.local_name()) {
                "v" => in_value = false,
                "c" => {
                    if let (Some(at), Ok(number)) = (position.take(), raw.parse()) {
                        sheet.load_cell(at, Cell::value(CellValue::Number(number)));
                    }
                }
                _ => {}
            },
            Event::Eof => return Ok(sheet),
            _ if in_value => push_text(&event, &mut raw)?,
            _ => {}
        }
    }
}
