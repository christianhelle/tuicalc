mod common;

use common::*;
use tuicalc::address::CellRef;
use tuicalc::model::{Cell, CellValue};
use tuicalc::xlsx::Workbook;

fn a1(text: &str) -> CellRef {
    CellRef::parse(text).unwrap()
}

fn open_rows(rows: &str, extras: &[(&str, &str)]) -> Workbook {
    let file = write_temp(&single_sheet(&sheet(rows), extras));
    Workbook::open(&file.path).unwrap()
}

#[test]
fn opens_sheet_names_and_numeric_cells() {
    let book = open_rows(r#"<row r="2"><c r="B2"><v>42.5</v></c></row>"#, &[]);

    assert_eq!(book.sheets.len(), 1);
    assert_eq!(book.sheets[0].name, "Sheet1");
    assert_eq!(book.sheets[0].cell(a1("B2")), Cell::value(CellValue::Number(42.5)));
    assert_eq!(book.sheets[0].cell(a1("A1")), Cell::default());
    assert!(!book.sheets[0].is_dirty());
}
