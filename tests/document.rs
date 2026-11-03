mod common;

use common::*;
use tuicalc::address::CellRef;
use tuicalc::document::{Document, Refusal};
use tuicalc::model::{Cell, CellValue};
use tuicalc::xlsx::Workbook;

fn a1(text: &str) -> CellRef {
    CellRef::parse(text).unwrap()
}

fn number(value: f64) -> Cell {
    Cell::value(CellValue::Number(value))
}

fn two_sheets() -> Vec<u8> {
    let content_types = CONTENT_TYPES.replace(
        r#"<Override PartName="/xl/worksheets/sheet1.xml""#,
        r#"<Override PartName="/xl/worksheets/sheet2.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/><Override PartName="/xl/worksheets/sheet1.xml""#,
    );
    let workbook = WORKBOOK.replace(
        r#"</sheets>"#,
        r#"<sheet name="Sheet2" sheetId="2" r:id="rId4"/></sheets>"#,
    );
    let rels = WORKBOOK_RELS.replace(
        "</Relationships>",
        r#"<Relationship Id="rId4" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet2.xml"/></Relationships>"#,
    );
    let first = sheet(r#"<row r="1"><c r="A1"><v>1</v></c></row>"#);
    let second = sheet(r#"<row r="1"><c r="A1"><v>2</v></c></row>"#);
    package(&[
        ("[Content_Types].xml", &content_types),
        ("_rels/.rels", ROOT_RELS),
        ("xl/workbook.xml", &workbook),
        ("xl/_rels/workbook.xml.rels", &rels),
        ("xl/worksheets/sheet1.xml", &first),
        ("xl/worksheets/sheet2.xml", &second),
    ])
}

fn document(bytes: &[u8]) -> (TempXlsx, Document) {
    let file = write_temp(bytes);
    let workbook = Workbook::open(&file.path).unwrap();
    (file, Document::new(workbook))
}

#[test]
fn an_edit_applies_to_the_sheet_on_show_and_can_be_undone() {
    let (_file, mut document) = document(&two_sheets());
    document.switch_sheet(1);

    document.edit(vec![(a1("B2"), number(5.0))]).unwrap();
    assert_eq!(document.workbook().sheets[1].cell(a1("B2")), number(5.0));
    assert!(document.workbook().is_dirty());

    assert_eq!(document.undo(), Ok(true));
    assert_eq!(
        document.workbook().sheets[1].cell(a1("B2")),
        Cell::default()
    );
    assert_eq!(document.undo(), Ok(false), "nothing left to undo");
    assert_eq!(document.redo(), Ok(true));
    assert_eq!(document.workbook().sheets[1].cell(a1("B2")), number(5.0));
}

#[test]
fn undo_shows_the_sheet_and_cell_it_changed() {
    let (_file, mut document) = document(&two_sheets());
    document.place.cursor = a1("C3");
    document.edit(vec![(a1("C3"), number(5.0))]).unwrap();
    document.switch_sheet(1);
    document.place.cursor = a1("A1");

    document.undo().unwrap();

    assert_eq!(document.sheet_index(), 0);
    assert_eq!(document.place.cursor, a1("C3"));
}

#[test]
fn each_sheet_remembers_where_the_cursor_was() {
    let (_file, mut document) = document(&two_sheets());
    document.place.cursor = a1("C3");

    document.switch_sheet(1);
    assert_eq!(document.place.cursor, a1("A1"));
    document.place.cursor = a1("D4");

    document.switch_sheet(1);
    assert_eq!(document.sheet_index(), 0, "going past the end wraps around");
    assert_eq!(document.place.cursor, a1("C3"));

    document.switch_sheet(-1);
    assert_eq!(document.sheet_index(), 1);
    assert_eq!(document.place.cursor, a1("D4"));
}

#[test]
fn a_locked_cell_is_refused_with_the_place_it_is() {
    let bytes = single_sheet(
        &sheet(r#"<row r="1"><c r="B1"><f t="array" ref="B1">C1*2</f><v>2</v></c></row>"#),
        &[],
    );
    let (_file, mut document) = document(&bytes);

    assert_eq!(
        document.edit(vec![(a1("B1"), number(1.0))]),
        Err(Refusal::Locked(a1("B1")))
    );
    assert_eq!(
        Refusal::Locked(a1("B1")).to_string(),
        "B1 is part of an array formula and is read-only"
    );
}
