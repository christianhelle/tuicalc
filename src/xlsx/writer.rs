//! Patches a package in place: untouched parts are copied without recompression, and
//! edited worksheets keep every byte outside the cells that changed.

use std::collections::BTreeMap;
use std::io::{Cursor, Write};

use quick_xml::Reader;
use quick_xml::escape::escape;
use quick_xml::events::Event;
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

fn patch_sheet(xml: &str, sheet: &Sheet) -> Result<String, XlsxError> {
    let mut pending: BTreeMap<CellRef, Cell> =
        sheet.dirty_cells().map(|at| (*at, sheet.cell(*at))).collect();
    let mut out = String::with_capacity(xml.len() + pending.len() * 32);
    let mut copied_to = 0;
    let mut reader = Reader::from_str(xml);
    loop {
        let before = position(&reader);
        let event = reader.read_event().map_err(xml_error)?;
        match &event {
            Event::Start(element) | Event::Empty(element) if local_name(element) == "c" => {
                let Some(at) = attr(element, "r").and_then(|r| CellRef::parse(&r)) else {
                    continue;
                };
                let Some(cell) = pending.remove(&at) else {
                    continue;
                };
                if matches!(event, Event::Start(_)) {
                    reader.read_to_end(element.name()).map_err(xml_error)?;
                }
                out.push_str(&xml[copied_to..before]);
                copied_to = position(&reader);
                write_cell(&mut out, at, attr(element, "s").as_deref(), &cell);
            }
            Event::Eof => break,
            _ => {}
        }
    }
    out.push_str(&xml[copied_to..]);
    Ok(out)
}

fn position(reader: &Reader<&[u8]>) -> usize {
    reader.buffer_position() as usize
}

fn write_cell(out: &mut String, at: CellRef, style: Option<&str>, cell: &Cell) {
    out.push_str(&format!(r#"<c r="{at}""#));
    if let Some(style) = style {
        out.push_str(&format!(r#" s="{}""#, escape(style)));
    }
    if let CellValue::Number(number) = cell.value {
        out.push_str(&format!("><v>{number}</v></c>"));
    }
}
