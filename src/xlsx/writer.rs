//! Patches a package in place: untouched parts are copied without recompression, and
//! edited worksheets keep every byte outside the cells that changed.

use std::collections::BTreeMap;
use std::io::{Cursor, Write};

use quick_xml::Reader;
use quick_xml::escape::escape;
use quick_xml::events::{BytesStart, Event};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use super::XlsxError;
use super::reader::Layout;
use super::xml::{attr, local_name, xml_error};
use crate::address::CellRef;
use crate::model::{Cell, CellValue, Sheet};

pub fn patch(source: &[u8], layout: &Layout, sheets: &[Sheet]) -> Result<Vec<u8>, XlsxError> {
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

    for index in 0..archive.len() {
        let name = archive.by_index_raw(index)?.name()?.to_string();
        if let Some(sheet) = dirty_sheets.get(name.as_str()) {
            let xml = read_text(&mut archive, index)?;
            let patched = patch_sheet(&xml, sheet)?;
            output.start_file(name, options)?;
            output.write_all(patched.as_bytes())?;
        } else {
            output.raw_copy_file(archive.by_index_raw(index)?)?;
        }
    }
    Ok(output.finish()?.into_inner())
}

fn read_text(archive: &mut ZipArchive<Cursor<&[u8]>>, index: usize) -> Result<String, XlsxError> {
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
        let keys: Vec<CellRef> = self.pending.keys().copied().filter(|at| wanted(*at)).collect();
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
        let cells = self.take_pending(|at| limit.is_none_or(|limit| at.row < limit));
        let mut index = 0;
        while index < cells.len() {
            let row = cells[index].0.row;
            let end = cells[index..]
                .iter()
                .position(|(at, _)| at.row != row)
                .map_or(cells.len(), |offset| index + offset);
            self.out.push_str(&format!(r#"<{}row r="{}">"#, self.prefix, row + 1));
            self.write_cells(&cells[index..end]);
            self.out.push_str(&format!("</{}row>", self.prefix));
            index = end;
        }
    }
}

fn qualified_prefix(element: &BytesStart<'_>) -> String {
    let name = element.name();
    let full: &str = name.as_ref();
    full.strip_suffix(&local_name(element)).unwrap_or("").to_string()
}

fn patch_sheet(xml: &str, sheet: &Sheet) -> Result<String, XlsxError> {
    let pending: BTreeMap<CellRef, Cell> =
        sheet.dirty_cells().map(|at| (*at, sheet.cell(*at))).collect();
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
                        if let Some(cell) = patch.pending.remove(&at) {
                            let style = attr(element, "s");
                            if !empty {
                                reader.read_to_end(element.name()).map_err(xml_error)?;
                            }
                            patch.copied_to = position(&reader);
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
    out.push_str(&format!(r#"<{prefix}c r="{at}""#));
    if let Some(style) = style {
        out.push_str(&format!(r#" s="{}""#, escape(style)));
    }
    if let CellValue::Number(number) = cell.value {
        out.push_str(&format!("><{prefix}v>{number}</{prefix}v></{prefix}c>"));
    }
}
