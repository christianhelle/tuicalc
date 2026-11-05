mod common;

use common::{TempXlsx, ods_content, ods_package, write_temp_as};
use tuicalc::address::CellRef;
use tuicalc::model::CellValue;
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
fn reads_percentages_currency_and_booleans() {
    let book = open_row(
        r#"<table:table-cell office:value-type="percentage" office:value="0.25"><text:p>25%</text:p></table:table-cell><table:table-cell office:value-type="currency" office:currency="EUR" office:value="9.5"><text:p>9,50 €</text:p></table:table-cell><table:table-cell office:value-type="boolean" office:boolean-value="true"><text:p>TRUE</text:p></table:table-cell><table:table-cell office:value-type="boolean" office:boolean-value="false"><text:p>FALSE</text:p></table:table-cell>"#,
    );
    let sheet = &book.sheets[0];
    assert_eq!(sheet.cell(at("A1")).value, CellValue::Number(0.25));
    assert_eq!(sheet.cell(at("B1")).value, CellValue::Number(9.5));
    assert_eq!(sheet.cell(at("C1")).value, CellValue::Bool(true));
    assert_eq!(sheet.cell(at("D1")).value, CellValue::Bool(false));
}

#[test]
fn reads_text_with_its_spaces_paragraphs_and_spans() {
    let book = open_row(concat!(
        r#"<table:table-cell office:value-type="string"><text:p>plain &amp; simple</text:p></table:table-cell>"#,
        r#"<table:table-cell office:value-type="string"><text:p>one</text:p><text:p>two</text:p></table:table-cell>"#,
        r#"<table:table-cell office:value-type="string"><text:p>a <text:s text:c="2"/>b<text:tab/>c<text:line-break/>d</text:p></table:table-cell>"#,
        r#"<table:table-cell office:value-type="string"><text:p><text:span text:style-name="T1">bo</text:span>ld</text:p></table:table-cell>"#,
        r#"<table:table-cell office:value-type="string"><text:p>
            wrapped   by a   writer
        </text:p></table:table-cell>"#,
        r#"<table:table-cell office:value-type="string" office:string-value="from the attribute"><text:p>shown</text:p></table:table-cell>"#,
        r#"<table:table-cell><text:p>no type</text:p></table:table-cell>"#,
    ));
    let text = |reference: &str| match book.sheets[0].cell(at(reference)).value {
        CellValue::Text(text) => text,
        other => panic!("{reference} holds {other:?}"),
    };
    assert_eq!(text("A1"), "plain & simple");
    assert_eq!(
        text("B1"),
        "one
two"
    );
    assert_eq!(
        text("C1"),
        "a   b	c
d"
    );
    assert_eq!(text("D1"), "bold");
    assert_eq!(text("E1"), "wrapped by a writer");
    assert_eq!(text("F1"), "from the attribute");
    assert_eq!(text("G1"), "no type");
}

#[test]
fn reads_sheet_names_and_a_number() {
    let book = open(
        r#"<table:table table:name="First"><table:table-row><table:table-cell office:value-type="float" office:value="1.5"><text:p>1.5</text:p></table:table-cell></table:table-row></table:table><table:table table:name="R&amp;D"/>"#,
    );
    let names: Vec<&str> = book
        .sheets
        .iter()
        .map(|sheet| sheet.name.as_str())
        .collect();
    assert_eq!(names, ["First", "R&D"]);
    assert_eq!(book.sheets[0].cell(at("A1")).value, CellValue::Number(1.5));
    assert!(!book.date1904);
}
