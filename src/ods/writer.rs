//! Patches an OpenDocument spreadsheet: every other part is copied without recompression, and edited
//! sheets keep every byte outside the cells that changed.

use std::collections::BTreeMap;
use std::io::{Cursor, Write};

use quick_xml::Reader;
use quick_xml::escape::partial_escape;
use quick_xml::events::{BytesStart, Event};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use super::formula::to_open_formula;
use super::reader::CONTENT_PART;
use super::{CALCEXT_NAMESPACE, Layout, OFFICE_NAMESPACE, TEXT_NAMESPACE, declared_prefix};
use crate::address::CellRef;
use crate::date;
use crate::model::{Cell, CellValue, Sheet};
use crate::workbook::WorkbookError;
use crate::workbook::xml::{local_name, plain_attr, xml_error};

const ROWS_REPEATED: &str = "number-rows-repeated";
const COLUMNS_REPEATED: &str = "number-columns-repeated";

pub fn patch(source: &[u8], layout: Layout, sheets: &[Sheet]) -> Result<Vec<u8>, WorkbookError> {
    match layout {
        Layout::Flat => {
            let xml = std::str::from_utf8(source)
                .map_err(|_| WorkbookError::Invalid("the file isn't UTF-8".into()))?;
            Ok(patch_content(xml, sheets)?.into_bytes())
        }
        Layout::Package => {
            let mut archive = ZipArchive::new(Cursor::new(source))?;
            let mut output = ZipWriter::new(Cursor::new(Vec::new()));
            let options =
                SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
            for index in 0..archive.len() {
                let name = archive.by_index_raw(index)?.name()?.to_string();
                if name == CONTENT_PART {
                    let mut xml = String::new();
                    std::io::Read::read_to_string(&mut archive.by_index(index)?, &mut xml)?;
                    output.start_file(name, options)?;
                    output.write_all(patch_content(&xml, sheets)?.as_bytes())?;
                } else {
                    output.raw_copy_file(archive.by_index_raw(index)?)?;
                }
            }
            Ok(output.finish()?.into_inner())
        }
    }
}

fn position(reader: &Reader<&[u8]>) -> usize {
    reader.buffer_position() as usize
}

fn repeat_count(element: &BytesStart<'_>, name: &str) -> u32 {
    plain_attr(element, name)
        .and_then(|count| count.parse().ok())
        .unwrap_or(1)
}

/// An element's name and attributes as written, so it can be written again with changes.
struct Tag<'a> {
    name: String,
    attributes: Vec<(String, String)>,
    inner: &'a str,
}

impl<'a> Tag<'a> {
    fn new(element: &BytesStart<'_>, inner: &'a str) -> Self {
        Self {
            name: qualified_name(element),
            attributes: element
                .attributes()
                .with_checks(false)
                .flatten()
                .map(|attribute| {
                    let key: &str = attribute.key.as_ref();
                    (key.to_string(), attribute.value.replace('"', "&quot;"))
                })
                .collect(),
            inner,
        }
    }

    /// Writes the element, with its repeat count set to `repeat`.
    fn write_repeated(&self, out: &mut String, repeat_attribute: &str, repeat: u32) {
        let prefix = self.name.split_once(':').map_or("", |(prefix, _)| prefix);
        let qualified = if prefix.is_empty() {
            repeat_attribute.to_string()
        } else {
            format!("{prefix}:{repeat_attribute}")
        };
        out.push('<');
        out.push_str(&self.name);
        let mut placed = false;
        for (key, value) in &self.attributes {
            if local_of(key) == repeat_attribute {
                placed = true;
                if repeat > 1 {
                    out.push_str(&format!(r#" {qualified}="{repeat}""#));
                }
            } else {
                out.push_str(&format!(r#" {key}="{value}""#));
            }
        }
        if !placed && repeat > 1 {
            out.push_str(&format!(r#" {qualified}="{repeat}""#));
        }
        if self.inner.is_empty() {
            out.push_str("/>");
        } else {
            out.push('>');
            out.push_str(self.inner);
            out.push_str(&format!("</{}>", self.name));
        }
    }
}

/// Escapes an attribute's value, leaving the apostrophes in sheet names readable.
fn escape_attribute(value: &str) -> String {
    partial_escape(value).replace('"', "&quot;")
}

fn prefix_of(key: &str) -> &str {
    key.split_once(':').map_or("", |(prefix, _)| prefix)
}

fn local_of(key: &str) -> &str {
    key.rsplit_once(':').map_or(key, |(_, local)| local)
}

/// Splices edited cells into the content XML, leaving all other bytes as they were.
fn patch_content(xml: &str, sheets: &[Sheet]) -> Result<String, WorkbookError> {
    let mut patch = ContentPatch {
        xml,
        out: String::with_capacity(xml.len()),
        copied_to: 0,
    };
    let mut reader = Reader::from_str(xml);
    let mut in_spreadsheet = false;
    let mut next_sheet = 0;
    let mut table: Option<TablePatch> = None;
    let mut prefixes = None;
    loop {
        let before = position(&reader);
        let event = reader.read_event().map_err(xml_error)?;
        let after = position(&reader);
        match &event {
            Event::Start(element) | Event::Empty(element) => {
                let empty = matches!(event, Event::Empty(_));
                if prefixes.is_none() {
                    prefixes = Some(Prefixes::declared_by(element)?);
                }
                let Some(prefixes) = &prefixes else {
                    continue;
                };
                match local_name(element).as_str() {
                    "spreadsheet" => in_spreadsheet = true,
                    "table" if in_spreadsheet && table.is_none() => {
                        let sheet = sheets.get(next_sheet);
                        next_sheet += 1;
                        let dirty = sheet.filter(|sheet| sheet.is_dirty());
                        match (dirty, empty) {
                            (None, false) => {
                                reader.read_to_end(element.name()).map_err(xml_error)?;
                            }
                            (None, true) => {}
                            (Some(sheet), false) => table = Some(TablePatch::new(sheet, after)),
                            (Some(sheet), true) => {
                                let mut state = TablePatch::new(sheet, after);
                                let tag = xml[before..after].trim_end_matches("/>").trim_end();
                                patch.copy_until(before, after);
                                patch.out.push_str(tag);
                                patch.out.push('>');
                                state.finish(&mut patch, None, table_prefix(element), prefixes);
                                patch
                                    .out
                                    .push_str(&format!("</{}>", qualified_name(element)));
                            }
                        }
                    }
                    "table-column" => {
                        if let Some(table) = &mut table {
                            table.declared_columns = table
                                .declared_columns
                                .saturating_add(repeat_count(element, COLUMNS_REPEATED));
                            table.last_column_end = after;
                        }
                    }
                    "shapes" | "forms" if !empty && table.is_some() => {
                        reader.read_to_end(element.name()).map_err(xml_error)?;
                    }
                    "table-row" => {
                        if let Some(table) = &mut table {
                            table.declare_columns(&mut patch, table_prefix(element));
                            let inner = if empty {
                                ""
                            } else {
                                let span = reader.read_to_end(element.name()).map_err(xml_error)?;
                                &xml[span.start as usize..span.end as usize]
                            };
                            let end = position(&reader);
                            table.patch_row(&mut patch, element, inner, before, end, prefixes);
                        }
                    }
                    _ => {}
                }
            }
            Event::End(element) => {
                if AsRef::<str>::as_ref(&element.local_name()) == "table" {
                    if let Some(mut state) = table.take() {
                        let name = element.name();
                        let full: &str = name.as_ref();
                        let prefix = full.split_once(':').map_or("", |(prefix, _)| prefix);
                        if let Some(prefixes) = &prefixes {
                            state.finish(&mut patch, Some(before), prefix.to_string(), prefixes);
                        }
                    }
                } else if AsRef::<str>::as_ref(&element.local_name()) == "spreadsheet" {
                    in_spreadsheet = false;
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    patch.copy_until(xml.len(), xml.len());
    Ok(patch.out)
}

fn qualified_name(element: &BytesStart<'_>) -> String {
    let name = element.name();
    let full: &str = name.as_ref();
    full.to_string()
}

fn table_prefix(element: &BytesStart<'_>) -> String {
    let name = qualified_name(element);
    name.split_once(':')
        .map_or(String::new(), |(prefix, _)| prefix.to_string())
}

/// The prefixes the file uses for the namespaces edited cells need.
struct Prefixes {
    office: String,
    text: String,
    calcext: Option<String>,
}

impl Prefixes {
    fn declared_by(root: &BytesStart<'_>) -> Result<Self, WorkbookError> {
        let required = |namespace| {
            declared_prefix(root, namespace).ok_or_else(|| {
                WorkbookError::Invalid(format!("the file doesn't declare {namespace}"))
            })
        };
        Ok(Self {
            office: required(OFFICE_NAMESPACE)?,
            text: required(TEXT_NAMESPACE)?,
            calcext: declared_prefix(root, CALCEXT_NAMESPACE),
        })
    }
}

/// The output so far, and how far through the source it has got.
struct ContentPatch<'a> {
    xml: &'a str,
    out: String,
    copied_to: usize,
}

impl ContentPatch<'_> {
    /// Copies source bytes up to `end`, then skips ahead to `resume`.
    fn copy_until(&mut self, end: usize, resume: usize) {
        self.out.push_str(&self.xml[self.copied_to..end]);
        self.copied_to = resume;
    }
}

/// An edited sheet being written.
struct TablePatch {
    pending: BTreeMap<CellRef, Cell>,
    columns_needed: u32,
    declared_columns: u32,
    /// Where the column declarations end, or the table's start if it has none.
    last_column_end: usize,
    columns_declared: bool,
    next_row: u32,
    last_row_end: Option<usize>,
}

impl TablePatch {
    fn new(sheet: &Sheet, start: usize) -> Self {
        let pending: BTreeMap<CellRef, Cell> = sheet
            .dirty_cells()
            .map(|at| (*at, sheet.cell(*at)))
            .collect();
        let columns_needed = pending
            .iter()
            .filter(|(_, cell)| **cell != Cell::default())
            .map(|(at, _)| at.col + 1)
            .max()
            .unwrap_or(0);
        Self {
            pending,
            columns_needed,
            declared_columns: 0,
            last_column_end: start,
            columns_declared: false,
            next_row: 0,
            last_row_end: None,
        }
    }

    /// Adds column declarations for edits past the last one. They must come before any row.
    fn declare_columns(&mut self, patch: &mut ContentPatch<'_>, prefix: String) {
        if self.columns_declared {
            return;
        }
        self.columns_declared = true;
        if self.columns_needed <= self.declared_columns {
            return;
        }
        let missing = self.columns_needed - self.declared_columns;
        patch.copy_until(self.last_column_end, self.last_column_end);
        let name = qualified(&prefix, "table-column");
        let repeat = qualified(&prefix, COLUMNS_REPEATED);
        if missing > 1 {
            patch
                .out
                .push_str(&format!(r#"<{name} {repeat}="{missing}"/>"#));
        } else {
            patch.out.push_str(&format!("<{name}/>"));
        }
    }

    /// Replaces a row element covering edited rows with its pieces.
    fn patch_row(
        &mut self,
        patch: &mut ContentPatch<'_>,
        element: &BytesStart<'_>,
        inner: &str,
        start: usize,
        end: usize,
        prefixes: &Prefixes,
    ) {
        let first = self.next_row;
        let repeated = repeat_count(element, ROWS_REPEATED);
        self.next_row = first.saturating_add(repeated);
        self.last_row_end = Some(end);
        let edited: Vec<u32> = self
            .pending
            .keys()
            .map(|at| at.row)
            .filter(|row| (first..self.next_row).contains(row))
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        if edited.is_empty() {
            return;
        }
        patch.copy_until(start, end);
        let tag = Tag::new(element, inner);
        let mut row_at = first;
        for row in edited {
            if row > row_at {
                tag.write_repeated(&mut patch.out, ROWS_REPEATED, row - row_at);
            }
            let cells = self.take_row(row);
            let new_inner = patch_row_cells(inner, &tag.name, &cells, prefixes);
            Tag {
                inner: &new_inner,
                name: tag.name.clone(),
                attributes: tag.attributes.clone(),
            }
            .write_repeated(&mut patch.out, ROWS_REPEATED, 1);
            row_at = row + 1;
        }
        if self.next_row > row_at {
            tag.write_repeated(&mut patch.out, ROWS_REPEATED, self.next_row - row_at);
        }
    }

    fn take_row(&mut self, row: u32) -> Vec<(u32, Cell)> {
        let columns: Vec<u32> = self
            .pending
            .keys()
            .filter(|at| at.row == row)
            .map(|at| at.col)
            .collect();
        columns
            .into_iter()
            .filter_map(|col| {
                self.pending
                    .remove(&CellRef { col, row })
                    .map(|cell| (col, cell))
            })
            .collect()
    }

    /// Writes the rows left over, past the end of the table. `table_end` is where the table's end
    /// tag starts in the source, if it has one.
    fn finish(
        &mut self,
        patch: &mut ContentPatch<'_>,
        table_end: Option<usize>,
        prefix: String,
        prefixes: &Prefixes,
    ) {
        self.pending.retain(|_, cell| *cell != Cell::default());
        if !self.pending.is_empty() || !self.columns_declared {
            self.declare_columns(patch, prefix.clone());
        }
        if self.pending.is_empty() {
            return;
        }
        let at = self.last_row_end.or(table_end);
        if let Some(at) = at {
            patch.copy_until(at, at);
        }
        let row_name = qualified(&prefix, "table-row");
        let cell_name = qualified(&prefix, "table-cell");
        let repeat = qualified(&prefix, ROWS_REPEATED);
        let rows: Vec<u32> = self
            .pending
            .keys()
            .map(|at| at.row)
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        for row in rows {
            let gap = row - self.next_row;
            if gap > 0 {
                let count = if gap > 1 {
                    format!(r#" {repeat}="{gap}""#)
                } else {
                    String::new()
                };
                patch
                    .out
                    .push_str(&format!("<{row_name}{count}><{cell_name}/></{row_name}>"));
            }
            let cells = self.take_row(row);
            let inner = patch_row_cells("", &row_name, &cells, prefixes);
            patch
                .out
                .push_str(&format!("<{row_name}>{inner}</{row_name}>"));
            self.next_row = row + 1;
        }
    }
}

fn qualified(prefix: &str, local: &str) -> String {
    if prefix.is_empty() {
        local.to_string()
    } else {
        format!("{prefix}:{local}")
    }
}

/// A row's cells with the edits made, splitting repeated cells around them.
fn patch_row_cells(
    inner: &str,
    row_name: &str,
    edits: &[(u32, Cell)],
    prefixes: &Prefixes,
) -> String {
    let prefix = row_name.split_once(':').map_or("", |(prefix, _)| prefix);
    let mut out = String::with_capacity(inner.len() + edits.len() * 64);
    let mut reader = Reader::from_str(inner);
    let mut copied_to = 0;
    let mut col = 0_u32;
    let mut edits = edits.iter().peekable();
    loop {
        let before = position(&reader);
        let Ok(event) = reader.read_event() else {
            break;
        };
        let after = position(&reader);
        match &event {
            Event::Start(element) | Event::Empty(element) => {
                let cell_inner = if matches!(event, Event::Empty(_)) {
                    ""
                } else {
                    let Ok(span) = reader.read_to_end(element.name()) else {
                        break;
                    };
                    &inner[span.start as usize..span.end as usize]
                };
                let end = if matches!(event, Event::Empty(_)) {
                    after
                } else {
                    position(&reader)
                };
                let local = local_name(element);
                if local != "table-cell" && local != "covered-table-cell" {
                    continue;
                }
                let repeated = repeat_count(element, COLUMNS_REPEATED);
                let past = col.saturating_add(repeated);
                if edits.peek().is_some_and(|(at, _)| *at < past) {
                    out.push_str(&inner[copied_to..before]);
                    copied_to = end;
                    let tag = Tag::new(element, cell_inner);
                    let mut piece_at = col;
                    while let Some((at, cell)) = edits.next_if(|(at, _)| *at < past) {
                        if *at > piece_at {
                            tag.write_repeated(&mut out, COLUMNS_REPEATED, at - piece_at);
                        }
                        write_edited_cell(&mut out, &tag, cell, prefixes);
                        piece_at = at + 1;
                    }
                    if past > piece_at {
                        tag.write_repeated(&mut out, COLUMNS_REPEATED, past - piece_at);
                    }
                }
                col = past;
            }
            Event::Eof => break,
            _ => {}
        }
    }
    out.push_str(&inner[copied_to..]);
    let cell_name = qualified(prefix, "table-cell");
    let repeat = qualified(prefix, COLUMNS_REPEATED);
    for (at, cell) in edits {
        if *cell == Cell::default() {
            continue;
        }
        let gap = at - col;
        if gap > 1 {
            out.push_str(&format!(r#"<{cell_name} {repeat}="{gap}"/>"#));
        } else if gap == 1 {
            out.push_str(&format!("<{cell_name}/>"));
        }
        let blank = Tag {
            name: cell_name.clone(),
            attributes: Vec::new(),
            inner: "",
        };
        write_edited_cell(&mut out, &blank, cell, prefixes);
        col = at + 1;
    }
    out
}

/// Attributes that say what a cell holds, which an edit replaces. The rest are kept.
const VALUE_ATTRIBUTES: [&str; 8] = [
    "value-type",
    "value",
    "date-value",
    "time-value",
    "boolean-value",
    "string-value",
    "currency",
    "formula",
];

fn write_edited_cell(out: &mut String, tag: &Tag<'_>, cell: &Cell, prefixes: &Prefixes) {
    let office = &prefixes.office;
    let text_prefix = &prefixes.text;
    let mut was_time = false;
    out.push('<');
    out.push_str(&tag.name);
    for (key, value) in &tag.attributes {
        let local = local_of(key);
        if local == "value-type" && prefix_of(key) == office {
            was_time = value == "time";
        }
        if local != COLUMNS_REPEATED && !VALUE_ATTRIBUTES.contains(&local) {
            out.push_str(&format!(r#" {key}="{value}""#));
        }
    }
    if let Some(formula) = &cell.formula {
        let table = prefix_of(&tag.name);
        let stored = escape_attribute(&to_open_formula(formula));
        out.push_str(&format!(r#" {}="{stored}""#, qualified(table, "formula")));
    }
    let mut paragraphs = String::new();
    match &cell.value {
        CellValue::Empty => {}
        CellValue::Number(serial) if cell.is_date && (was_time || *serial < 1.0) => {
            let duration = date::duration_from_serial(*serial);
            out.push_str(&format!(
                r#" {office}:value-type="time" {office}:time-value="{duration}""#
            ));
            write_paragraphs(
                &mut paragraphs,
                text_prefix,
                &date::clock_from_serial(*serial),
            );
        }
        CellValue::Number(serial) if cell.is_date => {
            let iso = date::iso_from_serial(*serial);
            out.push_str(&format!(
                r#" {office}:value-type="date" {office}:date-value="{iso}""#
            ));
            write_paragraphs(
                &mut paragraphs,
                text_prefix,
                &date::format_serial(*serial, false),
            );
        }
        CellValue::Number(number) => {
            out.push_str(&format!(
                r#" {office}:value-type="float" {office}:value="{number}""#
            ));
            write_paragraphs(&mut paragraphs, text_prefix, &number.to_string());
        }
        CellValue::Bool(flag) => {
            out.push_str(&format!(
                r#" {office}:value-type="boolean" {office}:boolean-value="{flag}""#
            ));
            write_paragraphs(
                &mut paragraphs,
                text_prefix,
                if *flag { "TRUE" } else { "FALSE" },
            );
        }
        CellValue::Error(text) => {
            match &prefixes.calcext {
                Some(calcext) => out.push_str(&format!(
                    r#" {office}:value-type="string" {office}:string-value="" {calcext}:value-type="error""#
                )),
                None => out.push_str(&format!(
                    r#" {office}:value-type="string" {office}:string-value="{}""#,
                    escape_attribute(text)
                )),
            }
            write_paragraphs(&mut paragraphs, text_prefix, text);
        }
        CellValue::Text(text) => {
            out.push_str(&format!(r#" {office}:value-type="string""#));
            write_paragraphs(&mut paragraphs, text_prefix, text);
        }
    }
    let kept = kept_children(tag.inner);
    if kept.is_empty() && paragraphs.is_empty() {
        out.push_str("/>");
        return;
    }
    out.push('>');
    out.push_str(&kept);
    out.push_str(&paragraphs);
    out.push_str(&format!("</{}>", tag.name));
}

/// What's in a cell apart from its text, such as its note, as it was written.
fn kept_children(inner: &str) -> String {
    let mut kept = String::new();
    let mut reader = Reader::from_str(inner);
    loop {
        let before = position(&reader);
        match reader.read_event() {
            Ok(Event::Start(element)) => {
                let wanted = local_name(&element) != "p";
                if reader.read_to_end(element.name()).is_err() {
                    return kept;
                }
                if wanted {
                    kept.push_str(&inner[before..position(&reader)]);
                }
            }
            Ok(Event::Empty(element)) => {
                if local_name(&element) != "p" {
                    kept.push_str(&inner[before..position(&reader)]);
                }
            }
            Ok(Event::Eof) | Err(_) => return kept,
            Ok(_) => {}
        }
    }
}

/// Writes text as paragraphs, using elements for what XML whitespace would lose.
fn write_paragraphs(out: &mut String, name: &str, text: &str) {
    for line in text.split('\n') {
        let mut content = String::with_capacity(line.len());
        let mut chars = line.chars().peekable();
        let mut at_start = true;
        while let Some(c) = chars.next() {
            match c {
                ' ' => {
                    let mut run = 1;
                    while chars.next_if_eq(&' ').is_some() {
                        run += 1;
                    }
                    if !at_start && chars.peek().is_some() {
                        content.push(' ');
                        run -= 1;
                    }
                    match run {
                        0 => {}
                        1 => content.push_str(&format!("<{name}:s/>")),
                        _ => content.push_str(&format!(r#"<{name}:s {name}:c="{run}"/>"#)),
                    }
                }
                '\t' => content.push_str(&format!("<{name}:tab/>")),
                c if c < ' ' || c == '\u{FFFE}' || c == '\u{FFFF}' => {}
                '&' => content.push_str("&amp;"),
                '<' => content.push_str("&lt;"),
                '>' => content.push_str("&gt;"),
                c => content.push(c),
            }
            at_start = false;
        }
        if content.is_empty() {
            out.push_str(&format!("<{name}:p/>"));
        } else {
            out.push_str(&format!("<{name}:p>{content}</{name}:p>"));
        }
    }
}
