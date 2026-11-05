//! Streams an OpenDocument spreadsheet's content into the model.

use std::io::{BufRead, BufReader, Cursor};
use std::sync::atomic::AtomicU64;

use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};
use zip::ZipArchive;

use super::Layout;
use crate::address::{CellRef, MAX_COLS, MAX_ROWS};
use crate::date;
use crate::model::{Cell, CellValue, Sheet};
use crate::workbook::WorkbookError;
use crate::workbook::loading::{
    CHUNK_CELLS, Counting, FIRST_CHUNK_CELLS, READ_BUFFER_BYTES, SheetChunk,
};
use crate::workbook::xml::{attr, local_name, plain_attr, push_text, xml_error};

const CONTENT_PART: &str = "content.xml";

/// Reads all of a workbook at once.
pub fn read(source: &[u8], layout: Layout) -> Result<Vec<Sheet>, WorkbookError> {
    let progress = AtomicU64::new(0);
    let names = with_content(source, layout, &progress, &mut read_sheet_names)?;
    let mut sheets: Vec<Sheet> = names.iter().map(Sheet::new).collect();
    with_content(source, layout, &progress, &mut |input| {
        parse_content(input, &mut |sheet, chunk| {
            chunk.apply_to(&mut sheets[sheet]);
            true
        })
    })?;
    Ok(sheets)
}

/// Runs `read` over the workbook's content XML, counting the bytes read.
fn with_content<T>(
    source: &[u8],
    layout: Layout,
    progress: &AtomicU64,
    read: &mut dyn FnMut(&mut dyn BufRead) -> Result<T, WorkbookError>,
) -> Result<T, WorkbookError> {
    match layout {
        Layout::Package => {
            let mut archive = ZipArchive::new(Cursor::new(source))?;
            let file = archive.by_name(CONTENT_PART)?;
            let mut input = BufReader::with_capacity(
                READ_BUFFER_BYTES,
                Counting {
                    inner: file,
                    count: progress,
                },
            );
            read(&mut input)
        }
    }
}

/// The names of the sheets, skipping over everything in them.
fn read_sheet_names(input: &mut dyn BufRead) -> Result<Vec<String>, WorkbookError> {
    let mut reader = Reader::from_reader(input);
    let mut buffer = Vec::new();
    let mut skipped = Vec::new();
    let mut names = Vec::new();
    let mut in_spreadsheet = false;
    loop {
        buffer.clear();
        match reader.read_event_into(&mut buffer).map_err(xml_error)? {
            Event::Start(element) => match local_name(&element).as_str() {
                "spreadsheet" => in_spreadsheet = true,
                "table" if in_spreadsheet => {
                    names.push(attr(&element, "name").unwrap_or_default());
                    skipped.clear();
                    reader
                        .read_to_end_into(element.name(), &mut skipped)
                        .map_err(xml_error)?;
                }
                _ => {}
            },
            Event::Empty(element) if in_spreadsheet && local_name(&element) == "table" => {
                names.push(attr(&element, "name").unwrap_or_default());
            }
            Event::End(element) if AsRef::<str>::as_ref(&element.local_name()) == "spreadsheet" => {
                return Ok(names);
            }
            Event::Eof => return Ok(names),
            _ => {}
        }
    }
}

/// The cell being read, whose text buffers are reused from cell to cell.
#[derive(Default)]
struct Draft {
    value: CellValue,
    is_date: bool,
    repeated: u32,
    /// Whether the cell's value is its text, rather than something in its attributes.
    holds_text: bool,
    string_value: Option<String>,
    text: String,
    scratch: String,
    paragraphs: u32,
    in_paragraph: bool,
    /// Whether whitespace in the XML would be collapsed away here.
    after_space: bool,
    /// Whether the text ends in a space made by collapsing, which doesn't count at a line's end.
    collapsed_tail: bool,
}

impl Draft {
    fn start(&mut self, element: &BytesStart<'_>) {
        self.repeated = repeat_count(element, "number-columns-repeated");
        self.string_value = None;
        self.text.clear();
        self.paragraphs = 0;
        self.holds_text = false;
        self.is_date = false;
        self.value = match plain_attr(element, "value-type").as_deref() {
            Some("float" | "percentage" | "currency") => plain_attr(element, "value")
                .and_then(|value| value.parse().ok())
                .map_or(CellValue::Empty, CellValue::Number),
            Some("boolean") => {
                CellValue::Bool(plain_attr(element, "boolean-value").as_deref() == Some("true"))
            }
            Some("date") => {
                let text = attr(element, "date-value").unwrap_or_default();
                match date::serial_from_iso(&text) {
                    Some(serial) => {
                        self.is_date = true;
                        CellValue::Number(serial)
                    }
                    None => CellValue::Text(text),
                }
            }
            Some("time") => {
                let text = attr(element, "time-value").unwrap_or_default();
                match date::serial_from_duration(&text) {
                    Some(serial) => {
                        self.is_date = true;
                        CellValue::Number(serial)
                    }
                    None => CellValue::Text(text),
                }
            }
            Some("string") | None => {
                self.holds_text = true;
                self.string_value = attr(element, "string-value");
                CellValue::Empty
            }
            _ => CellValue::Empty,
        };
    }

    fn start_paragraph(&mut self) {
        if self.paragraphs > 0 {
            self.text.push('\n');
        }
        self.paragraphs += 1;
        self.in_paragraph = true;
        self.after_space = true;
        self.collapsed_tail = false;
    }

    fn end_paragraph(&mut self) {
        if self.collapsed_tail {
            self.text.pop();
        }
        self.in_paragraph = false;
    }

    /// Adds text from the XML, where any run of whitespace means one space.
    fn push_collapsing(&mut self, event: &Event<'_>) -> Result<(), WorkbookError> {
        self.scratch.clear();
        push_text(event, &mut self.scratch)?;
        for c in self.scratch.chars() {
            if matches!(c, ' ' | '\t' | '\r' | '\n') {
                if !self.after_space {
                    self.text.push(' ');
                    self.after_space = true;
                    self.collapsed_tail = true;
                }
            } else {
                self.text.push(c);
                self.after_space = false;
                self.collapsed_tail = false;
            }
        }
        Ok(())
    }

    /// Adds characters that an element stands for, which are never collapsed.
    fn push_literal(&mut self, c: char, count: usize) {
        self.text.extend(std::iter::repeat_n(c, count));
        self.after_space = false;
        self.collapsed_tail = false;
    }

    fn take_cell(&mut self) -> Cell {
        let mut value = std::mem::take(&mut self.value);
        if self.holds_text {
            if let Some(text) = self.string_value.take() {
                value = CellValue::Text(text);
            } else if self.paragraphs > 0 {
                value = CellValue::Text(self.text.clone());
            }
        }
        Cell {
            value,
            formula: None,
            is_date: self.is_date,
        }
    }
}

fn repeat_count(element: &BytesStart<'_>, name: &str) -> u32 {
    plain_attr(element, name)
        .and_then(|count| count.parse().ok())
        .unwrap_or(1)
}

/// The row being read.
#[derive(Default)]
struct Row {
    at: u32,
    next_col: u32,
    repeated: u32,
    cells: Vec<(u32, Cell)>,
}

impl Row {
    fn finish_cell(&mut self, draft: &mut Draft) {
        let cell = draft.take_cell();
        if cell != Cell::default() {
            let end = self.next_col.saturating_add(draft.repeated).min(MAX_COLS);
            for col in self.next_col..end {
                self.cells.push((col, cell.clone()));
            }
        }
        self.next_col = self.next_col.saturating_add(draft.repeated);
    }
}

/// Reads the sheets, handing their cells to `sink` in chunks along with the sheet's position. It
/// stops early if `sink` returns false.
fn parse_content(
    input: &mut dyn BufRead,
    sink: &mut dyn FnMut(usize, SheetChunk) -> bool,
) -> Result<(), WorkbookError> {
    let mut reader = Reader::from_reader(input);
    let mut buffer = Vec::new();
    let mut in_spreadsheet = false;
    let mut sheet = None;
    let mut next_sheet = 0;
    let mut chunk = SheetChunk::default();
    let mut chunk_limit = FIRST_CHUNK_CELLS;
    let mut next_row = 0_u32;
    let mut row = Row::default();
    let mut draft = Draft::default();
    loop {
        buffer.clear();
        let event = reader.read_event_into(&mut buffer).map_err(xml_error)?;
        match &event {
            Event::Start(element) | Event::Empty(element) => {
                let empty = matches!(event, Event::Empty(_));
                let name = element.local_name();
                match AsRef::<str>::as_ref(&name) {
                    "spreadsheet" => in_spreadsheet = true,
                    "table" if in_spreadsheet && sheet.is_none() => {
                        if empty {
                            next_sheet += 1;
                        } else {
                            sheet = Some(next_sheet);
                            next_sheet += 1;
                            next_row = 0;
                            chunk_limit = FIRST_CHUNK_CELLS;
                        }
                    }
                    "table-row" if sheet.is_some() => {
                        let repeated = repeat_count(element, "number-rows-repeated");
                        if empty {
                            next_row = next_row.saturating_add(repeated);
                        } else {
                            row = Row {
                                at: next_row,
                                repeated,
                                ..Row::default()
                            };
                        }
                    }
                    "table-cell" | "covered-table-cell" if sheet.is_some() => {
                        draft.start(element);
                        if empty {
                            row.finish_cell(&mut draft);
                        }
                    }
                    "p" if sheet.is_some() => {
                        draft.start_paragraph();
                        if empty {
                            draft.end_paragraph();
                        }
                    }
                    "s" if draft.in_paragraph => {
                        let count = repeat_count(element, "c") as usize;
                        draft.push_literal(' ', count);
                    }
                    "tab" if draft.in_paragraph => draft.push_literal('\t', 1),
                    "line-break" if draft.in_paragraph => draft.push_literal('\n', 1),
                    _ => {}
                }
            }
            Event::End(element) => match AsRef::<str>::as_ref(&element.local_name()) {
                "p" if draft.in_paragraph => draft.end_paragraph(),
                "table-cell" | "covered-table-cell" if sheet.is_some() => {
                    row.finish_cell(&mut draft);
                }
                "table-row" if sheet.is_some() => {
                    next_row = next_row.saturating_add(row.repeated);
                    let end = row.at.saturating_add(row.repeated).min(MAX_ROWS);
                    let cells = std::mem::take(&mut row.cells);
                    for at in row.at..end {
                        for (col, cell) in &cells {
                            chunk
                                .cells
                                .push((CellRef { col: *col, row: at }, cell.clone()));
                        }
                    }
                    if chunk.cells.len() >= chunk_limit {
                        if let Some(sheet) = sheet
                            && !sink(sheet, std::mem::take(&mut chunk))
                        {
                            return Ok(());
                        }
                        chunk_limit = CHUNK_CELLS;
                    }
                }
                "table" => {
                    if let Some(sheet) = sheet.take()
                        && !sink(sheet, std::mem::take(&mut chunk))
                    {
                        return Ok(());
                    }
                }
                "spreadsheet" => return Ok(()),
                _ => {}
            },
            Event::Eof => return Ok(()),
            _ if draft.in_paragraph => draft.push_collapsing(&event)?,
            _ => {}
        }
    }
}
