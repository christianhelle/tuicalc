mod common;

use common::{TempXlsx, ods_content, ods_package, write_temp_as};
use tuicalc::address::CellRef;
use tuicalc::model::{Cell, CellValue};
use tuicalc::workbook::Workbook;

fn at(reference: &str) -> CellRef {
    CellRef::parse(reference).unwrap()
}

fn ods_file(tables: &str) -> TempXlsx {
    write_temp_as(&ods_package(&ods_content(tables)), "book.ods")
}

fn open(tables: &str) -> Workbook {
    let file = ods_file(tables);
    Workbook::open(&file.path).unwrap()
}

/// A one-sheet workbook holding one row with the given cells.
fn open_row(cells: &str) -> Workbook {
    open(&format!(
        r#"<table:table table:name="Sheet1"><table:table-row>{cells}</table:table-row></table:table>"#
    ))
}

#[test]
fn reads_sheet_names_and_a_number() {
    let book = open(
        r#"<table:table table:name="First"><table:table-row><table:table-cell office:value-type="float" office:value="1.5"><text:p>1.5</text:p></table:table-cell></table:table-row></table:table><table:table table:name="R&amp;D"/>"#,
    );
    let names: Vec<&str> = book.sheets.iter().map(|sheet| sheet.name.as_str()).collect();
    assert_eq!(names, ["First", "R&D"]);
    assert_eq!(book.sheets[0].cell(at("A1")).value, CellValue::Number(1.5));
    assert!(!book.date1904);
}
