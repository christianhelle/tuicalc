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

    assert_eq!(
        read_part(&file.path, "xl/charts/chart1.xml").unwrap(),
        chart
    );
    assert_eq!(read_part(&file.path, "xl/workbook.xml").unwrap(), WORKBOOK);
    let saved = read_part(&file.path, "xl/worksheets/sheet1.xml").unwrap();
    assert_eq!(
        saved,
        sheet_xml.replace(
            r#"<c r="A1" s="7"><v>1</v></c>"#,
            r#"<c r="A1" s="7"><v>5</v></c>"#
        ),
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
    assert!(
        saved.contains(
            r#"<c r="A1"><v>1</v></c><c r="B1"><v>2</v></c><c r="C1"><v>3</v></c></row>"#
        ),
        "{saved}"
    );

    let saved = saved_sheet(rows, "D1", number(4.0));
    assert!(
        saved.contains(r#"<c r="C1"><v>3</v></c><c r="D1"><v>4</v></c></row>"#),
        "{saved}"
    );
}

#[test]
fn inserts_new_rows_in_row_order() {
    let rows = r#"<row r="1"><c r="A1"><v>1</v></c></row><row r="5"><c r="A5"><v>5</v></c></row>"#;
    let saved = saved_sheet(rows, "B3", number(3.0));
    assert!(
        saved.contains(r#"</row><row r="3"><c r="B3"><v>3</v></c></row><row r="5">"#),
        "{saved}"
    );

    let saved = saved_sheet(rows, "A9", number(9.0));
    assert!(
        saved.contains(r#"</row><row r="9"><c r="A9"><v>9</v></c></row></sheetData>"#),
        "{saved}"
    );
}

#[test]
fn fills_empty_rows_and_empty_sheet_data() {
    let saved = saved_sheet(r#"<row r="2" ht="30"/>"#, "A2", number(1.0));
    assert!(
        saved.contains(r#"<row r="2" ht="30"><c r="A2"><v>1</v></c></row>"#),
        "{saved}"
    );

    let bare = r#"<worksheet xmlns="urn"><sheetData/></worksheet>"#;
    let file = write_temp(&single_sheet(bare, &[]));
    let mut book = Workbook::open(&file.path).unwrap();
    book.sheets[0].set_cell(a1("A1"), number(1.0));
    book.save().unwrap();
    let saved = read_part(&file.path, "xl/worksheets/sheet1.xml").unwrap();
    assert_eq!(
        saved,
        r#"<worksheet xmlns="urn"><sheetData><row r="1"><c r="A1"><v>1</v></c></row></sheetData></worksheet>"#
    );
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

const ONE_CELL: &str = r#"<row r="1"><c r="A1" s="4"><v>1</v></c></row>"#;

#[test]
fn saves_every_value_type_so_it_reads_back_identically() {
    let cells = [
        Cell::value(CellValue::Text("  a < b & \"c\"\nnext  ".into())),
        Cell::value(CellValue::Bool(false)),
        Cell::value(CellValue::Error("#N/A".into())),
        Cell::value(CellValue::Number(-1.5e300)),
        Cell::formula("IF(A2<1,\"x\",B2)"),
    ];
    for cell in cells {
        let (_file, book) = edited(ONE_CELL, "A1", cell.clone());
        assert_eq!(book.sheets[0].cell(a1("A1")), cell);
    }
}

#[test]
fn saves_a_formula_together_with_its_cached_text_result() {
    let cell = Cell {
        value: CellValue::Text("ab".into()),
        formula: Some("B1&C1".into()),
        is_date: false,
    };
    let (_file, book) = edited(ONE_CELL, "A1", cell.clone());
    assert_eq!(book.sheets[0].cell(a1("A1")), cell);
}

#[test]
fn clearing_a_cell_keeps_its_style_but_drops_its_content() {
    let saved = saved_sheet(ONE_CELL, "A1", Cell::default());
    assert!(
        saved.contains(r#"<row r="1"><c r="A1" s="4"/></row>"#),
        "{saved}"
    );

    let unstyled = r#"<row r="1"><c r="A1"><v>1</v></c></row>"#;
    let saved = saved_sheet(unstyled, "A1", Cell::default());
    assert!(saved.contains(r#"<row r="1"></row>"#), "{saved}");
}

#[test]
fn clearing_a_cell_that_never_existed_writes_nothing() {
    let saved = saved_sheet(ONE_CELL, "C9", Cell::default());
    assert_eq!(saved, sheet(ONE_CELL));
}

#[test]
fn control_characters_and_excel_escape_lookalikes_round_trip() {
    let text = "bell\u{7} cr\r literal _x0041_ end";
    let cell = Cell::value(CellValue::Text(text.into()));
    let (file, book) = edited(ONE_CELL, "A1", cell.clone());

    assert_eq!(book.sheets[0].cell(a1("A1")), cell);
    let saved = read_part(&file.path, "xl/worksheets/sheet1.xml").unwrap();
    assert!(
        !saved.contains('\u{7}'),
        "raw control characters are not legal XML"
    );
}

#[test]
fn decodes_excel_escapes_when_reading_shared_strings() {
    let strings = r#"<sst><si><t>a_x000D_b_x005F_x0041_</t></si></sst>"#;
    let rows = r#"<row r="1"><c r="A1" t="s"><v>0</v></c></row>"#;
    let file = write_temp(&single_sheet(
        &sheet(rows),
        &[("xl/sharedStrings.xml", strings)],
    ));
    let book = Workbook::open(&file.path).unwrap();
    assert_eq!(
        book.sheets[0].cell(a1("A1")).value,
        CellValue::Text("a\rb_x0041_".into())
    );
}

#[test]
fn editing_a_shared_formula_master_keeps_its_dependents_working() {
    let rows = r#"<row r="1"><c r="B1"><f t="shared" ref="B1:B2" si="0">A1*2</f><v>2</v></c></row><row r="2"><c r="B2" s="3"><f t="shared" si="0"/><v>4</v></c></row>"#;
    let (file, book) = edited(rows, "B1", number(100.0));

    assert_eq!(book.sheets[0].cell(a1("B1")), number(100.0));
    assert_eq!(
        book.sheets[0].cell(a1("B2")).formula.as_deref(),
        Some("A2*2")
    );
    let saved = read_part(&file.path, "xl/worksheets/sheet1.xml").unwrap();
    assert!(!saved.contains("shared"), "{saved}");
    assert!(
        saved.contains(r#"<c r="B2" s="3"><f>A2*2</f><v>4</v></c>"#),
        "{saved}"
    );
}

#[test]
fn unedited_sheets_keep_their_shared_formulas() {
    let rows = r#"<row r="1"><c r="B1"><f t="shared" ref="B1:B2" si="0">A1*2</f><v>2</v></c></row><row r="2"><c r="B2"><f t="shared" si="0"/><v>4</v></c></row>"#;
    let file = write_temp(&single_sheet(&sheet(rows), &[]));
    let mut book = Workbook::open(&file.path).unwrap();
    book.save().unwrap();
    assert_eq!(
        read_part(&file.path, "xl/worksheets/sheet1.xml").unwrap(),
        sheet(rows)
    );
}

const FORMULA_ROWS: &str =
    r#"<row r="1"><c r="A1"><v>1</v></c><c r="B1"><f>A1*2</f><v>2</v></c></row>"#;

fn save_edit_with_workbook(workbook: &str) -> String {
    let parts = [
        ("[Content_Types].xml", CONTENT_TYPES),
        ("_rels/.rels", ROOT_RELS),
        ("xl/workbook.xml", workbook),
        ("xl/_rels/workbook.xml.rels", WORKBOOK_RELS),
    ];
    let sheet_xml = sheet(FORMULA_ROWS);
    let mut all = parts.to_vec();
    all.push(("xl/worksheets/sheet1.xml", &sheet_xml));
    let file = write_temp(&package(&all));
    let mut book = Workbook::open(&file.path).unwrap();
    book.sheets[0].set_cell(a1("A1"), number(5.0));
    book.save().unwrap();
    read_part(&file.path, "xl/workbook.xml").unwrap()
}

#[test]
fn asks_excel_to_recalculate_when_a_workbook_with_formulas_changes() {
    let existing = WORKBOOK.replace("</sheets>", r#"</sheets><calcPr calcId="191029"/>"#);
    assert!(
        save_edit_with_workbook(&existing)
            .contains(r#"<calcPr fullCalcOnLoad="1" calcId="191029"/>"#)
    );

    let saved = save_edit_with_workbook(WORKBOOK);
    assert!(
        saved.contains(r#"</sheets><calcPr fullCalcOnLoad="1"/></workbook>"#),
        "{saved}"
    );

    let followed = WORKBOOK.replace("</sheets>", r#"</sheets><extLst/>"#);
    let saved = save_edit_with_workbook(&followed);
    assert!(
        saved.contains(r#"<calcPr fullCalcOnLoad="1"/><extLst/>"#),
        "schema order matters: {saved}"
    );
}

#[test]
fn leaves_the_workbook_part_alone_when_there_are_no_formulas() {
    let (file, _) = edited(
        r#"<row r="1"><c r="A1"><v>1</v></c></row>"#,
        "A1",
        number(2.0),
    );
    assert_eq!(read_part(&file.path, "xl/workbook.xml").unwrap(), WORKBOOK);
}

#[test]
fn removes_the_calculation_chain_so_it_cannot_go_stale() {
    let types = CONTENT_TYPES.replace(
        "</Types>",
        r#"<Override PartName="/xl/calcChain.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.calcChain+xml"/></Types>"#,
    );
    let rels = WORKBOOK_RELS.replace(
        "</Relationships>",
        r#"<Relationship Id="rId4" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/calcChain" Target="calcChain.xml"/></Relationships>"#,
    );
    let sheet_xml = sheet(FORMULA_ROWS);
    let file = write_temp(&package(&[
        ("[Content_Types].xml", &types),
        ("_rels/.rels", ROOT_RELS),
        ("xl/workbook.xml", WORKBOOK),
        ("xl/_rels/workbook.xml.rels", &rels),
        ("xl/worksheets/sheet1.xml", &sheet_xml),
        (
            "xl/calcChain.xml",
            r#"<calcChain><c r="B1" i="1"/></calcChain>"#,
        ),
    ]));
    let mut book = Workbook::open(&file.path).unwrap();

    book.sheets[0].set_cell(a1("B1"), number(7.0));
    book.save().unwrap();

    assert_eq!(read_part(&file.path, "xl/calcChain.xml"), None);
    assert_eq!(
        read_part(&file.path, "[Content_Types].xml").unwrap(),
        CONTENT_TYPES
    );
    assert_eq!(
        read_part(&file.path, "xl/_rels/workbook.xml.rels").unwrap(),
        WORKBOOK_RELS
    );
}

#[test]
fn refuses_to_overwrite_a_file_changed_by_another_program() {
    let file = write_temp(&single_sheet(&sheet(FORMULA_ROWS), &[]));
    let mut book = Workbook::open(&file.path).unwrap();
    let theirs = single_sheet(&sheet(""), &[]);
    std::fs::write(&file.path, &theirs).unwrap();

    book.sheets[0].set_cell(a1("A1"), number(5.0));
    let message = book.save().unwrap_err().to_string();

    assert!(message.contains("changed on disk"), "{message}");
    assert_eq!(std::fs::read(&file.path).unwrap(), theirs);
    assert!(book.is_dirty(), "the edit must not be lost");
}

#[test]
fn saving_without_edits_leaves_the_file_untouched_and_saves_repeat() {
    let bytes = single_sheet(&sheet(FORMULA_ROWS), &[]);
    let file = write_temp(&bytes);
    let mut book = Workbook::open(&file.path).unwrap();

    book.save().unwrap();
    assert_eq!(std::fs::read(&file.path).unwrap(), bytes);

    book.sheets[0].set_cell(a1("A1"), number(5.0));
    book.save().unwrap();
    assert!(!book.is_dirty());
    book.sheets[0].set_cell(a1("C1"), number(6.0));
    book.save().unwrap();

    let reloaded = Workbook::open(&file.path).unwrap();
    assert_eq!(reloaded.sheets[0].cell(a1("A1")), number(5.0));
    assert_eq!(reloaded.sheets[0].cell(a1("C1")), number(6.0));
}

#[test]
fn new_cells_across_several_rows_and_columns_are_written_in_sheet_order() {
    let rows = r#"<row r="1"><c r="A1"><v>1</v></c></row>"#;
    let file = write_temp(&single_sheet(&sheet(rows), &[]));
    let mut book = Workbook::open(&file.path).unwrap();
    for at in ["H26", "I25", "A30", "H25", "I26"] {
        book.sheets[0].set_cell(a1(at), number(1.0));
    }
    book.save().unwrap();

    let saved = read_part(&file.path, "xl/worksheets/sheet1.xml").unwrap();
    let expected = concat!(
        r#"<row r="25"><c r="H25"><v>1</v></c><c r="I25"><v>1</v></c></row>"#,
        r#"<row r="26"><c r="H26"><v>1</v></c><c r="I26"><v>1</v></c></row>"#,
        r#"<row r="30"><c r="A30"><v>1</v></c></row></sheetData>"#,
    );
    assert!(saved.contains(expected), "{saved}");
}

#[test]
fn new_rows_before_an_existing_row_keep_their_own_order() {
    let rows = r#"<row r="9"><c r="A9"><v>9</v></c></row>"#;
    let file = write_temp(&single_sheet(&sheet(rows), &[]));
    let mut book = Workbook::open(&file.path).unwrap();
    for at in ["B3", "A4", "C3", "B4"] {
        book.sheets[0].set_cell(a1(at), number(1.0));
    }
    book.save().unwrap();

    let saved = read_part(&file.path, "xl/worksheets/sheet1.xml").unwrap();
    let expected = concat!(
        r#"<row r="3"><c r="B3"><v>1</v></c><c r="C3"><v>1</v></c></row>"#,
        r#"<row r="4"><c r="A4"><v>1</v></c><c r="B4"><v>1</v></c></row>"#,
        r#"<row r="9">"#,
    );
    assert!(saved.contains(expected), "{saved}");
}

#[test]
fn a_blank_workbook_has_one_empty_sheet_and_nothing_to_save() {
    let book = Workbook::blank().unwrap();
    assert_eq!(book.sheets.len(), 1);
    assert_eq!(book.sheets[0].name, "Sheet1");
    assert_eq!(book.sheets[0].extent(), None);
    assert!(!book.date1904);
    assert!(!book.is_dirty());
}
