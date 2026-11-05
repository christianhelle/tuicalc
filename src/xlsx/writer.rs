//! Patches a package in place: untouched parts are copied without recompression, and
//! edited worksheets keep every byte outside the cells that changed.

use std::collections::BTreeMap;
use std::io::{Cursor, Write};

use quick_xml::Reader;
use quick_xml::escape::escape;
use quick_xml::events::{BytesStart, Event};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use super::escapes::starts_with_excel_escape;
use super::reader::{self, Layout};
use crate::address::CellRef;
use crate::model::{Cell, CellValue, Sheet};
use crate::workbook::WorkbookError;
use crate::workbook::xml::{attr, local_name, xml_error};

/// The parts of a package holding one empty sheet: all a workbook needs to open anywhere.
const BLANK_PARTS: [(&str, &str); 5] = [
    (
        "[Content_Types].xml",
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/></Types>"#,
    ),
    (
        "_rels/.rels",
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#,
    ),
    (
        "xl/workbook.xml",
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Sheet1" sheetId="1" r:id="rId1"/></sheets></workbook>"#,
    ),
    (
        "xl/_rels/workbook.xml.rels",
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/></Relationships>"#,
    ),
    (
        "xl/worksheets/sheet1.xml",
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData/></worksheet>"#,
    ),
];

pub fn blank_package() -> Result<Vec<u8>, WorkbookError> {
    let mut output = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    for (name, content) in BLANK_PARTS {
        output.start_file(name, options)?;
        output.write_all(content.as_bytes())?;
    }
    Ok(output.finish()?.into_inner())
}

pub fn patch(source: &[u8], layout: &Layout, sheets: &[Sheet]) -> Result<Vec<u8>, WorkbookError> {
    let mut archive = ZipArchive::new(Cursor::new(source))?;
    let mut output = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

    let dirty_sheets: BTreeMap<&str, &Sheet> = layout
        .sheet_parts
        .iter()
        .zip(sheets)
        .filter(|(_, sheet)| sheet.is_dirty())
        .map(|(part, sheet)| (part.as_str(), sheet))
        .collect();

    let has_formulas = sheets.iter().any(Sheet::has_formulas);
    let rels_part = reader::relationships_part(&layout.workbook_part);
    let calc_chain = find_calc_chain(&mut archive, &rels_part)?;

    for index in 0..archive.len() {
        let name = archive.by_index_raw(index)?.name()?.to_string();
        let patched = if let Some(sheet) = dirty_sheets.get(name.as_str()) {
            Some(patch_sheet(&read_text(&mut archive, index)?, sheet)?)
        } else if name == layout.workbook_part && has_formulas {
            Some(request_recalculation(&read_text(&mut archive, index)?)?)
        } else if calc_chain.is_some() && name == rels_part {
            let xml = read_text(&mut archive, index)?;
            Some(remove_elements(&xml, |element| {
                local_name(element) == "Relationship"
                    && attr(element, "Type").is_some_and(|kind| kind.ends_with(CALC_CHAIN))
            })?)
        } else if let (Some(chain), "[Content_Types].xml") = (&calc_chain, name.as_str()) {
            let xml = read_text(&mut archive, index)?;
            let part_name = format!("/{chain}");
            Some(remove_elements(&xml, |element| {
                attr(element, "PartName").as_deref() == Some(part_name.as_str())
            })?)
        } else {
            None
        };
        if calc_chain.as_deref() == Some(name.as_str()) {
            continue;
        }
        if let Some(patched) = patched {
            output.start_file(name, options)?;
            output.write_all(patched.as_bytes())?;
        } else {
            output.raw_copy_file(archive.by_index_raw(index)?)?;
        }
    }
    Ok(output.finish()?.into_inner())
}

const CALC_CHAIN: &str = "/calcChain";

/// The package path of the calculation chain, which lists every formula cell. Excel treats
/// an entry whose cell no longer holds a formula as corruption, and rebuilds a missing chain.
fn find_calc_chain(
    archive: &mut ZipArchive<Cursor<&[u8]>>,
    rels_part: &str,
) -> Result<Option<String>, WorkbookError> {
    let Some(index) = archive.index_for_name(rels_part) else {
        return Ok(None);
    };
    let xml = read_text(archive, index)?;
    let directory = rels_part
        .split("_rels/")
        .next()
        .unwrap_or("")
        .trim_end_matches('/');
    let relationships = reader::parse_relationships(&xml, directory)?;
    Ok(relationships
        .into_values()
        .find(|relationship| relationship.kind.ends_with(CALC_CHAIN))
        .map(|relationship| relationship.target))
}

/// Copies XML, leaving out elements accepted by `unwanted`.
fn remove_elements(
    xml: &str,
    unwanted: impl Fn(&BytesStart<'_>) -> bool,
) -> Result<String, WorkbookError> {
    let mut out = String::with_capacity(xml.len());
    let mut copied_to = 0;
    let mut reader = Reader::from_str(xml);
    loop {
        let before = position(&reader);
        match reader.read_event().map_err(xml_error)? {
            Event::Empty(element) if unwanted(&element) => {
                out.push_str(&xml[copied_to..before]);
                copied_to = position(&reader);
            }
            Event::Start(element) if unwanted(&element) => {
                reader.read_to_end(element.name()).map_err(xml_error)?;
                out.push_str(&xml[copied_to..before]);
                copied_to = position(&reader);
            }
            Event::Eof => break,
            _ => {}
        }
    }
    out.push_str(&xml[copied_to..]);
    Ok(out)
}

/// Elements that follow `calcPr` in the workbook schema.
const AFTER_CALC_PR: [&str; 9] = [
    "oleSize",
    "customWorkbookViews",
    "pivotCaches",
    "smartTagPr",
    "smartTagTypes",
    "webPublishing",
    "fileRecoveryPr",
    "webPublishObjects",
    "extLst",
];

/// Cached results are stale after an edit, so ask Excel to recalculate when it next opens.
fn request_recalculation(xml: &str) -> Result<String, WorkbookError> {
    let mut reader = Reader::from_str(xml);
    let mut depth = 0;
    let mut prefix = String::new();
    loop {
        let before = position(&reader);
        let event = reader.read_event().map_err(xml_error)?;
        let after = position(&reader);
        match &event {
            Event::Start(element) | Event::Empty(element) if depth == 0 => {
                prefix = qualified_prefix(element);
            }
            Event::Start(element) | Event::Empty(element) if depth == 1 => {
                let name = local_name(element);
                if name == "calcPr" {
                    return Ok(set_full_calc(xml, before, after, element));
                }
                if AFTER_CALC_PR.contains(&name.as_str()) {
                    return Ok(insert_calc_pr(xml, before, &prefix));
                }
            }
            Event::End(_) if depth == 1 => return Ok(insert_calc_pr(xml, before, &prefix)),
            Event::Eof => return Ok(xml.to_string()),
            _ => {}
        }
        match event {
            Event::Start(_) => depth += 1,
            Event::End(_) => depth -= 1,
            _ => {}
        }
    }
}

fn insert_calc_pr(xml: &str, at: usize, prefix: &str) -> String {
    let element = format!(r#"<{prefix}calcPr fullCalcOnLoad="1"/>"#);
    format!("{}{element}{}", &xml[..at], &xml[at..])
}

fn set_full_calc(xml: &str, start: usize, end: usize, element: &BytesStart<'_>) -> String {
    let tag = &xml[start..end];
    if attr(element, "fullCalcOnLoad").is_some() {
        let on = tag.replace(r#"fullCalcOnLoad="0""#, r#"fullCalcOnLoad="1""#);
        let on = on.replace(r#"fullCalcOnLoad="false""#, r#"fullCalcOnLoad="1""#);
        return format!("{}{on}{}", &xml[..start], &xml[end..]);
    }
    let name_end = start + 1 + element.name().as_ref().len();
    format!(
        "{} fullCalcOnLoad=\"1\"{}",
        &xml[..name_end],
        &xml[name_end..]
    )
}

fn read_text(
    archive: &mut ZipArchive<Cursor<&[u8]>>,
    index: usize,
) -> Result<String, WorkbookError> {
    let mut content = String::new();
    std::io::Read::read_to_string(&mut archive.by_index(index)?, &mut content)?;
    Ok(content)
}

/// Splices pending cells into worksheet XML, leaving all other bytes as they were.
struct SheetPatch<'a> {
    xml: &'a str,
    out: String,
    copied_to: usize,
    pending: BTreeMap<CellRef, Cell>,
    /// The namespace prefix used by the worksheet, including its colon.
    prefix: String,
}

impl SheetPatch<'_> {
    /// Copies source bytes up to `end`, then skips ahead to `resume`.
    fn copy_until(&mut self, end: usize, resume: usize) {
        self.out.push_str(&self.xml[self.copied_to..end]);
        self.copied_to = resume;
    }

    /// Removes and returns the pending cells accepted by `wanted`.
    fn take_pending(&mut self, wanted: impl Fn(CellRef) -> bool) -> Vec<(CellRef, Cell)> {
        let keys: Vec<CellRef> = self
            .pending
            .keys()
            .copied()
            .filter(|at| wanted(*at))
            .collect();
        keys.into_iter()
            .filter_map(|at| self.pending.remove(&at).map(|cell| (at, cell)))
            .collect()
    }

    fn write_cells(&mut self, cells: &[(CellRef, Cell)]) {
        for (at, cell) in cells {
            write_cell(&mut self.out, &self.prefix, *at, None, cell);
        }
    }

    /// Writes pending cells in rows that come before `limit` as new rows.
    fn write_rows_before(&mut self, limit: Option<u32>) {
        let mut cells = self.take_pending(|at| limit.is_none_or(|limit| at.row < limit));
        cells.retain(|(_, cell)| *cell != Cell::default());
        let mut index = 0;
        while index < cells.len() {
            let row = cells[index].0.row;
            let end = cells[index..]
                .iter()
                .position(|(at, _)| at.row != row)
                .map_or(cells.len(), |offset| index + offset);
            self.out
                .push_str(&format!(r#"<{}row r="{}">"#, self.prefix, row + 1));
            self.write_cells(&cells[index..end]);
            self.out.push_str(&format!("</{}row>", self.prefix));
            index = end;
        }
    }
}

fn qualified_prefix(element: &BytesStart<'_>) -> String {
    let name = element.name();
    let full: &str = name.as_ref();
    full.strip_suffix(&local_name(element))
        .unwrap_or("")
        .to_string()
}

fn patch_sheet(xml: &str, sheet: &Sheet) -> Result<String, WorkbookError> {
    let pending: BTreeMap<CellRef, Cell> = sheet
        .dirty_cells()
        .map(|at| (*at, sheet.cell(*at)))
        .collect();
    let mut patch = SheetPatch {
        xml,
        out: String::with_capacity(xml.len() + pending.len() * 32),
        copied_to: 0,
        pending,
        prefix: String::new(),
    };
    let mut reader = Reader::from_str(xml);
    let mut row = 0;
    let mut row_seen = false;
    let mut in_sheet_data = false;
    let mut next_col = 0;
    loop {
        let before = position(&reader);
        let event = reader.read_event().map_err(xml_error)?;
        let after = position(&reader);
        match &event {
            Event::Start(element) | Event::Empty(element) => {
                let empty = matches!(event, Event::Empty(_));
                match local_name(element).as_str() {
                    "sheetData" => {
                        patch.prefix = qualified_prefix(element);
                        in_sheet_data = true;
                        if empty {
                            let tag = xml[before..after].trim_end_matches("/>").trim_end();
                            patch.copy_until(before, after);
                            patch.out.push_str(tag);
                            patch.out.push('>');
                            patch.write_rows_before(None);
                            patch.out.push_str(&format!("</{}sheetData>", patch.prefix));
                        }
                    }
                    "row" if in_sheet_data => {
                        row = attr(element, "r")
                            .and_then(|r| r.parse::<u32>().ok())
                            .and_then(|r| r.checked_sub(1))
                            .unwrap_or(if row_seen { row + 1 } else { 0 });
                        row_seen = true;
                        next_col = 0;
                        patch.copy_until(before, before);
                        patch.write_rows_before(Some(row));
                        if empty {
                            let this_row = row;
                            let cells = patch.take_pending(|at| at.row == this_row);
                            if !cells.is_empty() {
                                let tag = xml[before..after].trim_end_matches("/>").trim_end();
                                patch.copy_until(before, after);
                                patch.out.push_str(tag);
                                patch.out.push('>');
                                patch.write_cells(&cells);
                                patch.out.push_str(&format!("</{}row>", patch.prefix));
                            }
                        }
                    }
                    "c" if in_sheet_data => {
                        let at = attr(element, "r")
                            .and_then(|r| CellRef::parse(&r))
                            .unwrap_or(CellRef { col: next_col, row });
                        next_col = at.col + 1;
                        patch.copy_until(before, before);
                        let earlier = patch.take_pending(|p| p.row == at.row && p.col < at.col);
                        patch.write_cells(&earlier);
                        let style = attr(element, "s");
                        let edit = patch.pending.remove(&at);
                        let mut end = after;
                        let mut shared = false;
                        if !empty {
                            let span = reader.read_to_end(element.name()).map_err(xml_error)?;
                            end = position(&reader);
                            let inner = &xml[span.start as usize..span.end as usize];
                            shared =
                                inner.contains(r#"t="shared""#) || inner.contains("t='shared'");
                        }
                        if let Some(cell) = edit.or_else(|| shared.then(|| sheet.cell(at))) {
                            patch.copy_until(before, end);
                            write_cell(&mut patch.out, &patch.prefix, at, style.as_deref(), &cell);
                        }
                    }
                    _ => {}
                }
            }
            Event::End(element) => match AsRef::<str>::as_ref(&element.local_name()) {
                "row" if in_sheet_data => {
                    patch.copy_until(before, before);
                    let this_row = row;
                    let cells = patch.take_pending(|at| at.row == this_row);
                    patch.write_cells(&cells);
                }
                "sheetData" => {
                    patch.copy_until(before, before);
                    patch.write_rows_before(None);
                    in_sheet_data = false;
                }
                _ => {}
            },
            Event::Eof => break,
            _ => {}
        }
    }
    patch.copy_until(xml.len(), xml.len());
    Ok(patch.out)
}

fn position(reader: &Reader<&[u8]>) -> usize {
    reader.buffer_position() as usize
}

fn write_cell(out: &mut String, prefix: &str, at: CellRef, style: Option<&str>, cell: &Cell) {
    if cell.formula.is_none() && cell.value == CellValue::Empty && style.is_none() {
        return;
    }
    out.push_str(&format!(r#"<{prefix}c r="{at}""#));
    if let Some(style) = style {
        out.push_str(&format!(r#" s="{}""#, escape(style)));
    }
    let kind = match (&cell.value, &cell.formula) {
        (CellValue::Text(_), None) => Some("inlineStr"),
        (CellValue::Text(_), Some(_)) => Some("str"),
        (CellValue::Bool(_), _) => Some("b"),
        (CellValue::Error(_), _) => Some("e"),
        _ => None,
    };
    if let Some(kind) = kind {
        out.push_str(&format!(r#" t="{kind}""#));
    }
    if cell.formula.is_none() && cell.value == CellValue::Empty {
        out.push_str("/>");
        return;
    }
    out.push('>');
    if let Some(formula) = &cell.formula {
        out.push_str(&format!(
            "<{prefix}f>{}</{prefix}f>",
            escape(formula.as_str())
        ));
    }
    match (&cell.value, &cell.formula) {
        (CellValue::Empty, _) => {}
        (CellValue::Number(number), _) => push_value(out, prefix, &number.to_string()),
        (CellValue::Bool(flag), _) => push_value(out, prefix, if *flag { "1" } else { "0" }),
        (CellValue::Error(text), _) => push_value(out, prefix, &escape(text.as_str())),
        (CellValue::Text(text), Some(_)) => push_value(out, prefix, &encode_text(text)),
        (CellValue::Text(text), None) => out.push_str(&format!(
            r#"<{prefix}is><{prefix}t xml:space="preserve">{}</{prefix}t></{prefix}is>"#,
            encode_text(text)
        )),
    }
    out.push_str(&format!("</{prefix}c>"));
}

fn push_value(out: &mut String, prefix: &str, text: &str) {
    out.push_str(&format!("<{prefix}v>{text}</{prefix}v>"));
}

/// XML-escapes text, using Excel's `_xHHHH_` form for what XML cannot carry literally.
fn encode_text(text: &str) -> String {
    let mut encoded = String::with_capacity(text.len());
    for (index, c) in text.char_indices() {
        if c == '_' && starts_with_excel_escape(&text[index..]) {
            encoded.push_str("_x005F_");
        } else if (c < ' ' && !matches!(c, '\n' | '\t')) || c == '\u{FFFE}' || c == '\u{FFFF}' {
            encoded.push_str(&format!("_x{:04X}_", u32::from(c)));
        } else {
            encoded.push(c);
        }
    }
    escape(encoded).into_owned()
}
