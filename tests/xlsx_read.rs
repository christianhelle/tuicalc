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

#[test]
fn reads_shared_strings_including_rich_text_without_phonetic_hints() {
    let strings = r#"<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><si><t>Fish &amp; chips</t></si><si><r><t>bo</t></r><r><t>ld</t></r><rPh sb="0" eb="1"><t>hint</t></rPh></si></sst>"#;
    let book = open_rows(
        r#"<row r="1"><c r="A1" t="s"><v>0</v></c><c r="B1" t="s"><v>1</v></c></row>"#,
        &[("xl/sharedStrings.xml", strings)],
    );

    let cell = |at| book.sheets[0].cell(a1(at)).value;
    assert_eq!(cell("A1"), CellValue::Text("Fish & chips".into()));
    assert_eq!(cell("B1"), CellValue::Text("bold".into()));
}

#[test]
fn reads_inline_strings_booleans_and_errors() {
    let book = open_rows(
        r##"<row r="1"><c r="A1" t="inlineStr"><is><t>inline</t></is></c><c r="B1" t="b"><v>1</v></c><c r="C1" t="e"><v>#DIV/0!</v></c></row>"##,
        &[],
    );

    let cell = |at| book.sheets[0].cell(a1(at)).value;
    assert_eq!(cell("A1"), CellValue::Text("inline".into()));
    assert_eq!(cell("B1"), CellValue::Bool(true));
    assert_eq!(cell("C1"), CellValue::Error("#DIV/0!".into()));
}

#[test]
fn reads_formulas_with_their_cached_results() {
    let book = open_rows(
        r#"<row r="1"><c r="A1"><f>SUM(B1:C1)</f><v>3</v></c><c r="B1" t="str"><f>"a"&amp;"b"</f><v>ab</v></c></row>"#,
        &[],
    );

    let a = book.sheets[0].cell(a1("A1"));
    assert_eq!(a.formula.as_deref(), Some("SUM(B1:C1)"));
    assert_eq!(a.value, CellValue::Number(3.0));
    let b = book.sheets[0].cell(a1("B1"));
    assert_eq!(b.formula.as_deref(), Some(r#""a"&"b""#));
    assert_eq!(b.value, CellValue::Text("ab".into()));
}

#[test]
fn locates_cells_that_omit_their_reference() {
    let book = open_rows(r#"<row><c><v>1</v></c><c><v>2</v></c></row><row><c><v>3</v></c></row>"#, &[]);

    assert_eq!(book.sheets[0].cell(a1("A1")).value, CellValue::Number(1.0));
    assert_eq!(book.sheets[0].cell(a1("B1")).value, CellValue::Number(2.0));
    assert_eq!(book.sheets[0].cell(a1("A2")).value, CellValue::Number(3.0));
}

#[test]
fn ignores_chartsheets_and_keeps_worksheet_order() {
    let workbook = r#"<workbook xmlns:r="x"><sheets><sheet name="Chart" sheetId="1" r:id="rId9"/><sheet name="Data" sheetId="2" r:id="rId1"/></sheets></workbook>"#;
    let rels = r#"<Relationships><Relationship Id="rId1" Type="http://x/worksheet" Target="/xl/worksheets/sheet1.xml"/><Relationship Id="rId9" Type="http://x/chartsheet" Target="chartsheets/sheet1.xml"/></Relationships>"#;
    let bytes = package(&[
        ("_rels/.rels", ROOT_RELS),
        ("xl/workbook.xml", workbook),
        ("xl/_rels/workbook.xml.rels", rels),
        ("xl/worksheets/sheet1.xml", &sheet("")),
    ]);

    let book = Workbook::open(&write_temp(&bytes).path).unwrap();

    let names: Vec<_> = book.sheets.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["Data"]);
}

#[test]
fn reads_namespace_prefixed_worksheets() {
    let xml = r#"<x:worksheet xmlns:x="urn"><x:sheetData><x:row r="1"><x:c r="A1"><x:v>7</x:v></x:c></x:row></x:sheetData></x:worksheet>"#;
    let book = Workbook::open(&write_temp(&single_sheet(xml, &[])).path).unwrap();
    assert_eq!(book.sheets[0].cell(a1("A1")).value, CellValue::Number(7.0));
}

#[test]
fn reports_files_that_are_not_workbooks() {
    let not_zip = write_temp(b"plain text");
    assert!(Workbook::open(&not_zip.path).is_err());

    let no_rels = write_temp(&package(&[("a.txt", "hi")]));
    let message = Workbook::open(&no_rels.path).unwrap_err().to_string();
    assert!(message.contains("_rels/.rels"), "{message}");

    assert!(Workbook::open("does-not-exist.xlsx").is_err());
}
