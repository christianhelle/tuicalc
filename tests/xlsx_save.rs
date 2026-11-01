mod common;

use common::*;
use tuicalc::address::CellRef;
use tuicalc::model::{Cell, CellValue};
use tuicalc::xlsx::Workbook;

fn a1(text: &str) -> CellRef {
    CellRef::parse(text).unwrap()
}

fn number(value: f64) -> Cell {
    Cell::value(CellValue::Number(value))
}

/// Applies one edit to a file containing `rows`, saves, and reloads it.
fn edited(rows: &str, at: &str, cell: Cell) -> (TempXlsx, Workbook) {
    let file = write_temp(&single_sheet(&sheet(rows), &[]));
    let mut book = Workbook::open(&file.path).unwrap();
    book.sheets[0].set_cell(a1(at), cell);
    book.save().unwrap();
    let reloaded = Workbook::open(&file.path).unwrap();
    (file, reloaded)
}

#[test]
fn saved_edits_replace_the_existing_cell_and_survive_a_reload() {
    let (_file, book) = edited(
        r#"<row r="1"><c r="A1"><v>1</v></c><c r="B1"><v>2</v></c></row>"#,
        "A1",
        number(9.5),
    );

    assert_eq!(book.sheets[0].cell(a1("A1")).value, CellValue::Number(9.5));
    assert_eq!(book.sheets[0].cell(a1("B1")).value, CellValue::Number(2.0));
}

#[test]
fn saving_keeps_everything_it_does_not_edit() {
    let chart = "<chartSpace>not understood by the model</chartSpace>";
    let sheet_xml = r#"<?xml version="1.0"?><worksheet xmlns="urn"><cols><col min="1" max="1" width="30" customWidth="1"/></cols><sheetData><row r="1" ht="40" customHeight="1"><c r="A1" s="7"><v>1</v></c><c r="B1" s="8"><v>2</v></c></row></sheetData><mergeCells count="1"><mergeCell ref="C1:D1"/></mergeCells></worksheet>"#;
    let file = write_temp(&single_sheet(sheet_xml, &[("xl/charts/chart1.xml", chart)]));
    let mut book = Workbook::open(&file.path).unwrap();

    book.sheets[0].set_cell(a1("A1"), number(5.0));
    book.save().unwrap();

    assert_eq!(read_part(&file.path, "xl/charts/chart1.xml").unwrap(), chart);
    assert_eq!(read_part(&file.path, "xl/workbook.xml").unwrap(), WORKBOOK);
    let saved = read_part(&file.path, "xl/worksheets/sheet1.xml").unwrap();
    assert_eq!(
        saved,
        sheet_xml.replace(r#"<c r="A1" s="7"><v>1</v></c>"#, r#"<c r="A1" s="7"><v>5</v></c>"#),
        "only the edited cell may change, and it keeps its style"
    );
}

fn saved_sheet(rows_xml: &str, at: &str, cell: Cell) -> String {
    let (file, _) = edited(rows_xml, at, cell);
    read_part(&file.path, "xl/worksheets/sheet1.xml").unwrap()
}

#[test]
fn inserts_new_cells_in_column_order() {
    let rows = r#"<row r="1"><c r="A1"><v>1</v></c><c r="C1"><v>3</v></c></row>"#;
    let saved = saved_sheet(rows, "B1", number(2.0));
    assert!(saved.contains(r#"<c r="A1"><v>1</v></c><c r="B1"><v>2</v></c><c r="C1"><v>3</v></c></row>"#), "{saved}");

    let saved = saved_sheet(rows, "D1", number(4.0));
    assert!(saved.contains(r#"<c r="C1"><v>3</v></c><c r="D1"><v>4</v></c></row>"#), "{saved}");
}

#[test]
fn inserts_new_rows_in_row_order() {
    let rows = r#"<row r="1"><c r="A1"><v>1</v></c></row><row r="5"><c r="A5"><v>5</v></c></row>"#;
    let saved = saved_sheet(rows, "B3", number(3.0));
    assert!(saved.contains(r#"</row><row r="3"><c r="B3"><v>3</v></c></row><row r="5">"#), "{saved}");

    let saved = saved_sheet(rows, "A9", number(9.0));
    assert!(saved.contains(r#"</row><row r="9"><c r="A9"><v>9</v></c></row></sheetData>"#), "{saved}");
}

#[test]
fn fills_empty_rows_and_empty_sheet_data() {
    let saved = saved_sheet(r#"<row r="2" ht="30"/>"#, "A2", number(1.0));
    assert!(saved.contains(r#"<row r="2" ht="30"><c r="A2"><v>1</v></c></row>"#), "{saved}");

    let bare = r#"<worksheet xmlns="urn"><sheetData/></worksheet>"#;
    let file = write_temp(&single_sheet(bare, &[]));
    let mut book = Workbook::open(&file.path).unwrap();
    book.sheets[0].set_cell(a1("A1"), number(1.0));
    book.save().unwrap();
    let saved = read_part(&file.path, "xl/worksheets/sheet1.xml").unwrap();
    assert_eq!(saved, r#"<worksheet xmlns="urn"><sheetData><row r="1"><c r="A1"><v>1</v></c></row></sheetData></worksheet>"#);
}

#[test]
fn new_cells_use_the_namespace_prefix_of_the_worksheet() {
    let xml = r#"<x:worksheet xmlns:x="urn"><x:sheetData><x:row r="1"><x:c r="A1"><x:v>1</x:v></x:c></x:row></x:sheetData></x:worksheet>"#;
    let file = write_temp(&single_sheet(xml, &[]));
    let mut book = Workbook::open(&file.path).unwrap();
    book.sheets[0].set_cell(a1("A1"), number(2.0));
    book.sheets[0].set_cell(a1("A2"), number(3.0));
    book.save().unwrap();

    let saved = read_part(&file.path, "xl/worksheets/sheet1.xml").unwrap();
    assert!(saved.contains(r#"<x:row r="1"><x:c r="A1"><x:v>2</x:v></x:c></x:row><x:row r="2"><x:c r="A2"><x:v>3</x:v></x:c></x:row></x:sheetData>"#), "{saved}");
}
