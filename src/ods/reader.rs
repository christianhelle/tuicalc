//! Streams an OpenDocument spreadsheet's content into the model.

use std::borrow::Cow;
use std::io::{BufRead, BufReader, Cursor};
use std::sync::Arc;
use std::sync::atomic::AtomicU64;

use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};
use zip::ZipArchive;

use super::Layout;
use super::formula::to_excel_style;
use crate::address::{CellRef, MAX_COLS, MAX_ROWS};
use crate::date;
use crate::model::{Cell, CellValue, Sheet};
use crate::workbook::WorkbookError;
use crate::workbook::loading::{
    CHUNK_CELLS, Counting, FIRST_CHUNK_CELLS, LoadEvent, READ_BUFFER_BYTES, SheetChunk,
    WorkbookLoad,
};
use crate::workbook::xml::{attr, local_name, plain_attr, push_text, xml_error};

pub(super) const CONTENT_PART: &str = "content.xml";
/// How many cells repeats may add to a sheet. A small file can ask for far more than fits in memory.
const MAX_REPEATED_CELLS: u64 = 10_000_000;

/// What can be learned from a workbook without keeping its cells.
pub struct Plan {
    pub sheet_names: Vec<String>,
    /// The uncompressed size of the content.
    pub total_bytes: u64,
}

pub fn read_plan(source: &[u8], layout: Layout) -> Result<Plan, WorkbookError> {
    let progress = AtomicU64::new(0);
    let sheet_names = with_content(source, layout, &progress, &mut read_sheet_names)?;
    if sheet_names.is_empty() {
        return Err(WorkbookError::Invalid("no worksheets".into()));
    }
    let total_bytes = match layout {
        Layout::Package => ZipArchive::new(Cursor::new(source))?
            .by_name(CONTENT_PART)?
            .size(),
        Layout::Flat => source.len() as u64,
    };
    Ok(Plan {
        sheet_names,
        total_bytes,
    })
}

/// Reads all of a workbook at once.
pub fn read(source: &[u8], layout: Layout) -> Result<Vec<Sheet>, WorkbookError> {
    let plan = read_plan(source, layout)?;
    let mut sheets: Vec<Sheet> = plan.sheet_names.iter().map(Sheet::new).collect();
    with_content(source, layout, &AtomicU64::new(0), &mut |input| {
        parse_content(input, &mut |sheet, chunk| {
            chunk.apply_to(&mut sheets[sheet]);
            true
        })
    })?;
    Ok(sheets)
}

/// Reads a workbook on another thread. All its sheets are in one stream, so they come in file order.
pub fn start_load(source: Arc<Vec<u8>>, layout: Layout, plan: &Plan) -> WorkbookLoad {
    WorkbookLoad::start(plan.total_bytes, None, move |sender, read_bytes| {
        let result = with_content(&source, layout, read_bytes, &mut |input| {
            parse_content(input, &mut |sheet, chunk| {
                sender.send(LoadEvent::Cells { sheet, chunk }).is_ok()
            })
        });
        if let Err(error) = result {
            let _ = sender.send(LoadEvent::Failed(error));
        }
    })
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
        Layout::Flat => {
            let mut input = BufReader::with_capacity(
                READ_BUFFER_BYTES,
                Counting {
                    inner: source,
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
    is_error: bool,
    formula: Option<String>,
    /// The columns and rows of the array formula this cell starts.
    array_span: Option<(u32, u32)>,
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
        self.formula = attr(element, "formula").map(|stored| to_excel_style(&stored));
        let columns = plain_attr(element, "number-matrix-columns-spanned");
        let rows = plain_attr(element, "number-matrix-rows-spanned");
        self.array_span = (columns.is_some() || rows.is_some()).then(|| {
            let count = |text: Option<Cow<'_, str>>| {
                text.and_then(|text| text.parse().ok())
                    .unwrap_or(1_u32)
                    .max(1)
            };
            (count(columns), count(rows))
        });
        self.text.clear();
        self.paragraphs = 0;
        self.holds_text = false;
        self.is_date = false;
        let (value_type, is_error) = value_types(element);
        self.is_error = is_error;
        self.value = match value_type.as_deref() {
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
        if self.is_error {
            value = CellValue::Error(self.text.clone());
        } else if self.holds_text {
            if let Some(text) = self.string_value.take() {
                value = CellValue::Text(text);
            } else if self.paragraphs > 0 {
                value = CellValue::Text(self.text.clone());
            }
        }
        Cell {
            value,
            formula: self.formula.take(),
            is_date: self.is_date,
        }
    }
}

/// The `office:value-type` of a cell, and whether `calcext:value-type` says it holds an error. The
/// two share a local name, and either can come first.
fn value_types<'a>(element: &'a BytesStart<'_>) -> (Option<Cow<'a, str>>, bool) {
    let mut office = None;
    let mut is_error = false;
    for attribute in element.attributes().with_checks(false).flatten() {
        let key: &str = attribute.key.as_ref();
        if key.starts_with("calcext:") {
            is_error |= key.ends_with(":value-type") && attribute.value.as_ref() == "error";
        } else if AsRef::<str>::as_ref(&attribute.key.local_name()) == "value-type" {
            office = Some(attribute.value);
        }
    }
    (office, is_error)
}

fn repeat_count(element: &BytesStart<'_>, name: &str) -> u32 {
    plain_attr(element, name)
        .and_then(|count| count.parse().ok())
        .unwrap_or(1)
}

/// How many more cells repeats may add to the sheet being read.
struct RepeatBudget(u64);

impl RepeatBudget {
    fn spend(&mut self, cells: u64) -> Result<(), WorkbookError> {
        self.0 = self
            .0
            .checked_sub(cells)
            .ok_or_else(|| WorkbookError::Invalid("a sheet repeats too many cells".into()))?;
        Ok(())
    }
}

/// The row being read.
#[derive(Default)]
struct Row {
    at: u32,
    next_col: u32,
    repeated: u32,
    cells: Vec<(u32, Cell)>,
    locked: Vec<(CellRef, CellRef)>,
}

impl Row {
    fn finish_cell(
        &mut self,
        draft: &mut Draft,
        budget: &mut RepeatBudget,
    ) -> Result<(), WorkbookError> {
        if let Some((columns, rows)) = draft.array_span.take() {
            let first = CellRef {
                col: self.next_col,
                row: self.at,
            };
            let last = CellRef {
                col: self.next_col.saturating_add(columns - 1),
                row: self.at.saturating_add(rows - 1),
            };
            self.locked.push((first, last));
        }
        let cell = draft.take_cell();
        if cell != Cell::default() {
            let end = self.next_col.saturating_add(draft.repeated).min(MAX_COLS);
            budget.spend(u64::from(end.saturating_sub(self.next_col)))?;
            for col in self.next_col..end {
                self.cells.push((col, cell.clone()));
            }
        }
        self.next_col = self.next_col.saturating_add(draft.repeated);
        Ok(())
    }
}

/// Whether an element inside a sheet holds paragraphs that aren't its cell's text: everything in a
/// cell but its paragraphs, and whatever else hangs off a sheet or sits in a line of text.
fn holds_foreign_text(local: &str, in_cell: bool, in_paragraph: bool) -> bool {
    if in_paragraph {
        matches!(
            local,
            "annotation" | "note" | "frame" | "custom-shape" | "ruby-text"
        )
    } else if in_cell {
        local != "p"
    } else {
        matches!(local, "shapes" | "forms")
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
    let mut budget = RepeatBudget(MAX_REPEATED_CELLS);
    let mut in_cell = false;
    let mut skipped = Vec::new();
    loop {
        buffer.clear();
        let event = reader.read_event_into(&mut buffer).map_err(xml_error)?;
        match &event {
            Event::Start(element) | Event::Empty(element) => {
                let empty = matches!(event, Event::Empty(_));
                let name = element.local_name();
                let local = AsRef::<str>::as_ref(&name);
                if !empty
                    && sheet.is_some()
                    && holds_foreign_text(local, in_cell, draft.in_paragraph)
                {
                    skipped.clear();
                    reader
                        .read_to_end_into(element.name(), &mut skipped)
                        .map_err(xml_error)?;
                    continue;
                }
                match local {
                    "spreadsheet" => in_spreadsheet = true,
                    "table" if in_spreadsheet && sheet.is_none() => {
                        if empty {
                            next_sheet += 1;
                        } else {
                            sheet = Some(next_sheet);
                            next_sheet += 1;
                            next_row = 0;
                            budget = RepeatBudget(MAX_REPEATED_CELLS);
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
                            row.finish_cell(&mut draft, &mut budget)?;
                        } else {
                            in_cell = true;
                        }
                    }
                    "p" if in_cell => {
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
                    row.finish_cell(&mut draft, &mut budget)?;
                    in_cell = false;
                }
                "table-row" if sheet.is_some() => {
                    next_row = next_row.saturating_add(row.repeated);
                    let end = row.at.saturating_add(row.repeated).min(MAX_ROWS);
                    let cells = std::mem::take(&mut row.cells);
                    chunk.locked.append(&mut row.locked);
                    let rows = u64::from(end.saturating_sub(row.at));
                    budget.spend(rows.saturating_mul(cells.len() as u64))?;
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
