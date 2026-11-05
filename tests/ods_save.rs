mod common;

use std::io::Cursor;

use common::{ODS_MIMETYPE, TempXlsx, ods_content, ods_package_with, read_part, write_temp_as};
use tuicalc::address::CellRef;
use tuicalc::model::{Cell, CellValue, Sheet};
use tuicalc::workbook::{Workbook, WorkbookError};
use zip::{CompressionMethod, ZipArchive};

fn at(reference: &str) -> CellRef {
    CellRef::parse(reference).unwrap()
}

fn number(value: f64) -> Cell {
    Cell::value(CellValue::Number(value))
}

fn ods_file(tables: &str) -> TempXlsx {
    let bytes = ods_package_with(ODS_MIMETYPE, &ods_content(tables), &[]);
    write_temp_as(&bytes, "book.ods")
}

/// Applies the edits to a workbook of the given tables, and gives back the saved content's tables.
fn saved_tables(tables: &str, edits: &[(&str, Cell)]) -> String {
    let file = ods_file(tables);
    let mut book = Workbook::open(&file.path).unwrap();
    for (reference, cell) in edits {
        book.sheets[0].set_cell(at(reference), cell.clone());
    }
    book.save().unwrap();
    let content = read_part(&file.path, "content.xml").unwrap();
    let start = content.find("<office:spreadsheet>").unwrap() + "<office:spreadsheet>".len();
    let end = content.find("</office:spreadsheet>").unwrap();
    content[start..end].to_string()
}

/// The sheet as a fresh reader sees it after the edits are saved.
fn saved_sheet(tables: &str, edits: &[(&str, Cell)]) -> Sheet {
    let file = ods_file(tables);
    let mut book = Workbook::open(&file.path).unwrap();
    for (reference, cell) in edits {
        book.sheets[0].set_cell(at(reference), cell.clone());
    }
    book.save().unwrap();
    Workbook::open(&file.path).unwrap().sheets.remove(0)
}

const ALL_COLUMNS: &str = r#"<table:table-column table:number-columns-repeated="16384"/>"#;

fn table_of(content: &str) -> String {
    format!(r#"<table:table table:name="Sheet1">{content}</table:table>"#)
}

fn sheet_of(rows: &str) -> String {
    table_of(&format!("{ALL_COLUMNS}{rows}"))
}

const CELL_7: &str = r#"<table:table-cell office:value-type="float" office:value="7"><text:p>7</text:p></table:table-cell>"#;

#[test]
fn an_edit_changes_only_its_cell_and_the_rest_of_the_package_is_copied() {
    let tables = format!(
        r#"<table:table table:name="Sheet1"><table:table-column table:number-columns-repeated="3"/><table:table-row table:style-name="ro1"><table:table-cell table:style-name="ce1" office:value-type="float" office:value="1"><text:p>1</text:p></table:table-cell>{CELL_7}</table:table-row></table:table><table:table table:name="Other"><table:table-row>{CELL_7}</table:table-row></table:table>"#
    );
    let content = ods_content(&tables);
    let styles = "<office:document-styles/>";
    let bytes = ods_package_with(ODS_MIMETYPE, &content, &[("styles.xml", styles)]);
    let file = write_temp_as(&bytes, "book.ods");

    let mut book = Workbook::open(&file.path).unwrap();
    book.sheets[0].set_cell(at("A1"), number(2.0));
    book.save().unwrap();

    let saved = std::fs::read(&file.path).unwrap();
    let mut archive = ZipArchive::new(Cursor::new(saved)).unwrap();
    let first = archive.by_index(0).unwrap();
    assert_eq!(first.name().unwrap(), "mimetype");
    assert_eq!(first.compression(), CompressionMethod::Stored);
    drop(first);
    assert_eq!(read_part(&file.path, "styles.xml").unwrap(), styles);

    let expected = content.replace(
        r#"<table:table-cell table:style-name="ce1" office:value-type="float" office:value="1"><text:p>1</text:p></table:table-cell>"#,
        r#"<table:table-cell table:style-name="ce1" office:value-type="float" office:value="2"><text:p>2</text:p></table:table-cell>"#,
    );
    assert_eq!(read_part(&file.path, "content.xml").unwrap(), expected);
}

#[test]
fn an_edit_inside_repeated_cells_splits_them() {
    let tables = sheet_of(
        r#"<table:table-row><table:table-cell table:style-name="ce1" office:value-type="float" office:value="7" table:number-columns-repeated="5"><text:p>7</text:p></table:table-cell></table:table-row>"#,
    );
    let saved = saved_tables(&tables, &[("C1", number(9.0))]);
    assert_eq!(
        saved,
        sheet_of(concat!(
            r#"<table:table-row>"#,
            r#"<table:table-cell table:style-name="ce1" office:value-type="float" office:value="7" table:number-columns-repeated="2"><text:p>7</text:p></table:table-cell>"#,
            r#"<table:table-cell table:style-name="ce1" office:value-type="float" office:value="9"><text:p>9</text:p></table:table-cell>"#,
            r#"<table:table-cell table:style-name="ce1" office:value-type="float" office:value="7" table:number-columns-repeated="2"><text:p>7</text:p></table:table-cell>"#,
            r#"</table:table-row>"#,
        ))
    );
}

#[test]
fn an_edit_at_the_edge_of_repeated_cells_leaves_no_empty_piece() {
    let tables = sheet_of(
        r#"<table:table-row><table:table-cell table:number-columns-repeated="2"/></table:table-row>"#,
    );
    assert_eq!(
        saved_tables(&tables, &[("B1", number(9.0))]),
        sheet_of(
            r#"<table:table-row><table:table-cell/><table:table-cell office:value-type="float" office:value="9"><text:p>9</text:p></table:table-cell></table:table-row>"#
        )
    );
    assert_eq!(
        saved_tables(&tables, &[("A1", number(9.0))]),
        sheet_of(
            r#"<table:table-row><table:table-cell office:value-type="float" office:value="9"><text:p>9</text:p></table:table-cell><table:table-cell/></table:table-row>"#
        )
    );
}

#[test]
fn an_edit_inside_repeated_rows_splits_them() {
    let row = |attributes: &str| {
        format!(r#"<table:table-row table:style-name="ro1"{attributes}>{CELL_7}</table:table-row>"#)
    };
    let tables = sheet_of(&row(r#" table:number-rows-repeated="5""#));
    let saved = saved_tables(&tables, &[("A3", number(9.0))]);
    let edited = r#"<table:table-row table:style-name="ro1"><table:table-cell office:value-type="float" office:value="9"><text:p>9</text:p></table:table-cell></table:table-row>"#;
    assert_eq!(
        saved,
        sheet_of(&format!(
            r#"{0}{edited}{0}"#,
            row(r#" table:number-rows-repeated="2""#)
        ))
    );
}

#[test]
fn edits_on_the_padding_of_a_full_sheet_keep_the_sheet_the_same_size() {
    let tables = sheet_of(
        "<table:table-row table:number-rows-repeated=\"1048576\"><table:table-cell table:number-columns-repeated=\"16384\"/></table:table-row>",
    );
    let saved = saved_tables(&tables, &[("C4", number(9.0)), ("A4", number(8.0))]);
    assert_eq!(
        saved,
        sheet_of(concat!(
            r#"<table:table-row table:number-rows-repeated="3"><table:table-cell table:number-columns-repeated="16384"/></table:table-row>"#,
            r#"<table:table-row><table:table-cell office:value-type="float" office:value="8"><text:p>8</text:p></table:table-cell><table:table-cell/><table:table-cell office:value-type="float" office:value="9"><text:p>9</text:p></table:table-cell><table:table-cell table:number-columns-repeated="16381"/></table:table-row>"#,
            r#"<table:table-row table:number-rows-repeated="1048572"><table:table-cell table:number-columns-repeated="16384"/></table:table-row>"#,
        ))
    );
}

#[test]
fn an_edit_past_the_end_of_a_row_pads_up_to_it() {
    let tables = sheet_of(&format!(r#"<table:table-row>{CELL_7}</table:table-row>"#));
    let saved = saved_tables(&tables, &[("D1", number(9.0))]);
    assert_eq!(
        saved,
        sheet_of(&format!(
            r#"<table:table-row>{CELL_7}<table:table-cell table:number-columns-repeated="2"/><table:table-cell office:value-type="float" office:value="9"><text:p>9</text:p></table:table-cell></table:table-row>"#
        ))
    );
}

#[test]
fn an_edit_on_an_empty_row_element_opens_it_up() {
    let tables =
        sheet_of(r#"<table:table-row table:style-name="ro1" table:number-rows-repeated="2"/>"#);
    let saved = saved_tables(&tables, &[("A2", number(9.0))]);
    assert_eq!(
        saved,
        sheet_of(
            r#"<table:table-row table:style-name="ro1"/><table:table-row table:style-name="ro1"><table:table-cell office:value-type="float" office:value="9"><text:p>9</text:p></table:table-cell></table:table-row>"#
        )
    );
}

#[test]
fn an_edit_past_the_last_row_comes_after_the_rows_and_before_what_follows_them() {
    let tables = sheet_of(&format!(
        r#"<table:table-row>{CELL_7}</table:table-row><table:named-expressions/>"#
    ));
    let saved = saved_tables(&tables, &[("B4", number(9.0))]);
    assert_eq!(
        saved,
        sheet_of(&format!(
            r#"<table:table-row>{CELL_7}</table:table-row><table:table-row table:number-rows-repeated="2"><table:table-cell/></table:table-row><table:table-row><table:table-cell/><table:table-cell office:value-type="float" office:value="9"><text:p>9</text:p></table:table-cell></table:table-row><table:named-expressions/>"#
        ))
    );
}

#[test]
fn an_edit_past_the_declared_columns_declares_more() {
    let tables = table_of(&format!(
        r#"<table:table-column table:style-name="co1"/><table:table-column table:number-columns-repeated="2"/><table:table-row>{CELL_7}</table:table-row>"#
    ));
    let saved = saved_tables(&tables, &[("F1", number(9.0))]);
    assert_eq!(
        saved,
        table_of(&format!(
            r#"<table:table-column table:style-name="co1"/><table:table-column table:number-columns-repeated="2"/><table:table-column table:number-columns-repeated="3"/><table:table-row>{CELL_7}<table:table-cell table:number-columns-repeated="4"/><table:table-cell office:value-type="float" office:value="9"><text:p>9</text:p></table:table-cell></table:table-row>"#
        ))
    );
}

#[test]
fn a_sheet_declaring_no_columns_gets_them_ahead_of_its_rows() {
    let tables = table_of(&format!(r#"<table:table-row>{CELL_7}</table:table-row>"#));
    let saved = saved_tables(&tables, &[("A1", number(9.0))]);
    assert_eq!(
        saved,
        table_of(
            r#"<table:table-column/><table:table-row><table:table-cell office:value-type="float" office:value="9"><text:p>9</text:p></table:table-cell></table:table-row>"#
        )
    );
}

#[test]
fn an_edit_on_a_sheet_with_no_rows_gives_it_some() {
    let file = ods_file(r#"<table:table table:name="Empty"/>"#);
    let mut book = Workbook::open(&file.path).unwrap();
    book.sheets[0].set_cell(at("B2"), number(9.0));
    book.save().unwrap();
    let again = Workbook::open(&file.path).unwrap();
    assert_eq!(again.sheets[0].name, "Empty");
    assert_eq!(again.sheets[0].cell(at("B2")).value, CellValue::Number(9.0));
    assert_eq!(again.sheets[0].cells().count(), 1);
}

#[test]
fn clearing_a_cell_with_nothing_there_writes_nothing() {
    let tables = sheet_of(&format!(r#"<table:table-row>{CELL_7}</table:table-row>"#));
    let saved = saved_tables(&tables, &[("B5", Cell::default())]);
    assert_eq!(saved, tables);
}

#[test]
fn only_the_sheet_with_edits_is_touched() {
    let tables = format!(
        r#"<table:table table:name="Sheet1"><table:table-row>{CELL_7}</table:table-row></table:table><table:table table:name="Other"><table:table-row>{CELL_7}</table:table-row></table:table>"#
    );
    let file = ods_file(&tables);
    let mut book = Workbook::open(&file.path).unwrap();
    book.sheets[1].set_cell(at("A1"), number(9.0));
    book.save().unwrap();
    let again = Workbook::open(&file.path).unwrap();
    assert_eq!(again.sheets[0].cell(at("A1")).value, CellValue::Number(7.0));
    assert_eq!(again.sheets[1].cell(at("A1")).value, CellValue::Number(9.0));
}

#[test]
fn reading_back_finds_every_edit_in_place() {
    let tables = sheet_of(
        "<table:table-row table:number-rows-repeated=\"4\"><table:table-cell office:value-type=\"float\" office:value=\"1\" table:number-columns-repeated=\"4\"/></table:table-row>",
    );
    let sheet = saved_sheet(
        &tables,
        &[
            ("B2", number(5.0)),
            ("D4", number(6.0)),
            ("A9", number(7.0)),
        ],
    );
    assert_eq!(sheet.cell(at("B2")).value, CellValue::Number(5.0));
    assert_eq!(sheet.cell(at("D4")).value, CellValue::Number(6.0));
    assert_eq!(sheet.cell(at("A9")).value, CellValue::Number(7.0));
    assert_eq!(sheet.cell(at("A2")).value, CellValue::Number(1.0));
    assert_eq!(sheet.cell(at("C2")).value, CellValue::Number(1.0));
    assert_eq!(sheet.cell(at("D3")).value, CellValue::Number(1.0));
    assert_eq!(sheet.cells().count(), 17);
}

/// What a cell in a sheet of its own is saved as, given what it held before.
fn saved_cell(before: &str, cell: Cell) -> String {
    let saved = saved_tables(
        &sheet_of(&format!("<table:table-row>{before}</table:table-row>")),
        &[("A1", cell)],
    );
    let prefix = format!("{ALL_COLUMNS}<table:table-row>");
    let start = saved.find(&prefix).unwrap() + prefix.len();
    let end = saved.find("</table:table-row>").unwrap();
    saved[start..end].to_string()
}

const BLANK: &str = "<table:table-cell/>";

fn text(content: &str) -> Cell {
    Cell::value(CellValue::Text(content.into()))
}

#[test]
fn text_is_escaped_and_keeps_its_spaces_lines_and_tabs() {
    assert_eq!(
        saved_cell(BLANK, text("a & <b>")),
        r#"<table:table-cell office:value-type="string"><text:p>a &amp; &lt;b&gt;</text:p></table:table-cell>"#
    );
    assert_eq!(
        saved_cell(BLANK, text(" a  b\tc \nd")),
        r#"<table:table-cell office:value-type="string"><text:p><text:s/>a <text:s/>b<text:tab/>c<text:s/></text:p><text:p>d</text:p></table:table-cell>"#
    );
    assert_eq!(
        saved_cell(BLANK, text("a    b")),
        r#"<table:table-cell office:value-type="string"><text:p>a <text:s text:c="3"/>b</text:p></table:table-cell>"#
    );
}

#[test]
fn text_reads_back_the_same_whatever_is_in_it() {
    let tables = sheet_of(&format!("<table:table-row>{BLANK}</table:table-row>"));
    for content in [
        "plain",
        " leading",
        "trailing ",
        "  ",
        "a   b",
        "two\nlines",
        "\nblank first",
        "tab\there",
        "",
    ] {
        let sheet = saved_sheet(&tables, &[("A1", text(content))]);
        assert_eq!(sheet.cell(at("A1")).value, CellValue::Text(content.into()));
    }
}

#[test]
fn characters_xml_cannot_carry_are_dropped() {
    assert_eq!(
        saved_cell(BLANK, text("a\u{1}b\u{FFFF}c")),
        r#"<table:table-cell office:value-type="string"><text:p>abc</text:p></table:table-cell>"#
    );
}

#[test]
fn booleans_and_errors_say_what_they_are() {
    assert_eq!(
        saved_cell(BLANK, Cell::value(CellValue::Bool(true))),
        r#"<table:table-cell office:value-type="boolean" office:boolean-value="true"><text:p>TRUE</text:p></table:table-cell>"#
    );
    assert_eq!(
        saved_cell(BLANK, Cell::value(CellValue::Error("#N/A".into()))),
        r#"<table:table-cell office:value-type="string" office:string-value="" calcext:value-type="error"><text:p>#N/A</text:p></table:table-cell>"#
    );
}

fn date(serial: f64) -> Cell {
    Cell {
        value: CellValue::Number(serial),
        formula: None,
        is_date: true,
    }
}

#[test]
fn dates_and_times_are_written_as_such() {
    assert_eq!(
        saved_cell(BLANK, date(45306.0)),
        r#"<table:table-cell office:value-type="date" office:date-value="2024-01-15"><text:p>2024-01-15</text:p></table:table-cell>"#
    );
    assert_eq!(
        saved_cell(BLANK, date(45306.75)),
        r#"<table:table-cell office:value-type="date" office:date-value="2024-01-15T18:00:00"><text:p>2024-01-15 18:00:00</text:p></table:table-cell>"#
    );
    assert_eq!(
        saved_cell(BLANK, date(0.25)),
        r#"<table:table-cell office:value-type="time" office:time-value="PT06H00M00S"><text:p>06:00:00</text:p></table:table-cell>"#
    );
}

#[test]
fn a_time_of_more_than_a_day_stays_a_time() {
    let before = r#"<table:table-cell office:value-type="time" office:time-value="PT36H00M00S"><text:p>36:00</text:p></table:table-cell>"#;
    assert_eq!(
        saved_cell(before, date(2.5)),
        r#"<table:table-cell office:value-type="time" office:time-value="PT60H00M00S"><text:p>60:00:00</text:p></table:table-cell>"#
    );
}

#[test]
fn an_edit_keeps_the_style_merge_and_note_and_replaces_the_rest() {
    let before = concat!(
        r#"<table:table-cell table:style-name="ce1" table:number-columns-spanned="2" table:formula="of:=1" office:value-type="currency" office:currency="EUR" office:value="1" calcext:value-type="currency">"#,
        r#"<office:annotation><text:p>a note</text:p></office:annotation><text:p>1,00 €</text:p></table:table-cell>"#,
    );
    assert_eq!(
        saved_cell(before, number(2.0)),
        concat!(
            r#"<table:table-cell table:style-name="ce1" table:number-columns-spanned="2" office:value-type="float" office:value="2">"#,
            r#"<office:annotation><text:p>a note</text:p></office:annotation><text:p>2</text:p></table:table-cell>"#,
        )
    );
}

#[test]
fn clearing_a_cell_keeps_only_what_it_looks_like() {
    let before = r#"<table:table-cell table:style-name="ce1" office:value-type="float" office:value="1"><text:p>1</text:p></table:table-cell>"#;
    assert_eq!(
        saved_cell(before, Cell::default()),
        r#"<table:table-cell table:style-name="ce1"/>"#
    );
    assert_eq!(saved_cell(CELL_7, Cell::default()), BLANK);
}

#[test]
fn attribute_values_stay_escaped_when_a_cell_is_rewritten() {
    let before = r#"<table:table-cell table:style-name="a&amp;b" table:content-validation-name='say "hi"'/>"#;
    assert_eq!(
        saved_cell(before, number(1.0)),
        r#"<table:table-cell table:style-name="a&amp;b" table:content-validation-name="say &quot;hi&quot;" office:value-type="float" office:value="1"><text:p>1</text:p></table:table-cell>"#
    );
}

fn saved_formula(formula: &str) -> String {
    let xml = saved_cell(BLANK, Cell::formula(formula));
    let after = xml
        .strip_prefix(r#"<table:table-cell table:formula=""#)
        .unwrap();
    after.strip_suffix(r#""/>"#).unwrap().to_string()
}

#[test]
fn formulas_are_written_as_open_formula_with_no_cached_value() {
    let cases = [
        ("A1+1", "of:=[.A1]+1"),
        ("SUM(A1:B2,$C$3)", "of:=SUM([.A1:.B2];[.$C$3])"),
        ("Sheet2!C3*2", "of:=[$Sheet2.C3]*2"),
        ("Sheet2!C3:D4", "of:=[$Sheet2.C3:.D4]"),
        ("Sheet1:Sheet3!A1:B2", "of:=[$Sheet1.A1:$Sheet3.B2]"),
        ("'My Sheet'!A1", "of:=[$'My Sheet'.A1]"),
        ("'It''s'!A1", "of:=[$'It''s'.A1]"),
        ("My.Sheet!A1", "of:=[$'My.Sheet'.A1]"),
        ("SUM(A:B)", "of:=SUM([.A:.B])"),
        ("SUM($1:2)", "of:=SUM([.$1:.2])"),
        ("#REF!+1", "of:=[.#REF!]+1"),
        ("SUM({1,2;3,4})", "of:=SUM({1;2|3;4})"),
        ("TaxRate*A1", "of:=TaxRate*[.A1]"),
        ("LOG10(A1)", "of:=LOG10([.A1])"),
        ("1.5E+3+TRUE", "of:=1.5E+3+TRUE"),
        (
            "['file:///x.ods'#$Sheet1.A1]",
            "of:=['file:///x.ods'#$Sheet1.A1]",
        ),
    ];
    for (typed, stored) in cases {
        assert_eq!(saved_formula(typed), stored, "{typed}");
    }
}

#[test]
fn text_in_a_formula_is_left_alone_and_the_attribute_is_escaped() {
    assert_eq!(
        saved_formula(r#"IF(A1<2,"x,A2&""y","z")"#),
        "of:=IF([.A1]&lt;2;&quot;x,A2&amp;&quot;&quot;y&quot;;&quot;z&quot;)"
    );
}

#[test]
fn a_formula_in_a_grammar_the_app_does_not_know_stays_as_it_is() {
    assert_eq!(saved_formula("ooow:=<A1>"), "ooow:=&lt;A1&gt;");
}

#[test]
fn formulas_read_back_as_they_were_typed() {
    let tables = sheet_of(&format!("<table:table-row>{BLANK}</table:table-row>"));
    for typed in [
        "SUM(A1:B2,$C$3)",
        "Sheet2!C3*2",
        "'My Sheet'!A1:B2",
        r#"IF(A1<2,"x,y","z")"#,
        "SUM({1,2;3,4})",
    ] {
        let sheet = saved_sheet(&tables, &[("A1", Cell::formula(typed))]);
        assert_eq!(sheet.cell(at("A1")).formula.as_deref(), Some(typed));
    }
}

#[test]
fn a_formula_with_a_value_keeps_it_as_the_cached_result() {
    let cell = Cell {
        value: CellValue::Number(2.0),
        formula: Some("1+1".into()),
        is_date: false,
    };
    assert_eq!(
        saved_cell(BLANK, cell),
        r#"<table:table-cell table:formula="of:=1+1" office:value-type="float" office:value="2"><text:p>2</text:p></table:table-cell>"#
    );
}

/// Content where every namespace has a prefix other than the usual one.
const FOREIGN: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<o:document-content xmlns:o="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:t="urn:oasis:names:tc:opendocument:xmlns:table:1.0" xmlns:x="urn:oasis:names:tc:opendocument:xmlns:text:1.0" xmlns:c="urn:org:documentfoundation:names:experimental:calc:xmlns:calcext:1.0"><o:body><o:spreadsheet><t:table t:name="Sheet1"><t:table-column t:number-columns-repeated="2"/><t:table-row><t:table-cell o:value-type="float" o:value="1" t:number-columns-repeated="2"><x:p>1</x:p></t:table-cell></t:table-row></t:table></o:spreadsheet></o:body></o:document-content>"#;

#[test]
fn edits_use_the_prefixes_the_file_declares() {
    let file = write_temp_as(&ods_package_with(ODS_MIMETYPE, FOREIGN, &[]), "book.ods");
    let mut book = Workbook::open(&file.path).unwrap();
    assert_eq!(book.sheets[0].cell(at("B1")).value, CellValue::Number(1.0));
    book.sheets[0].set_cell(at("A1"), text("a  b"));
    book.sheets[0].set_cell(at("B1"), Cell::value(CellValue::Error("#N/A".into())));
    book.sheets[0].set_cell(at("A2"), Cell::formula("A1"));
    book.save().unwrap();
    let saved = read_part(&file.path, "content.xml").unwrap();
    assert!(
        saved.contains(concat!(
            r#"<t:table-row><t:table-cell o:value-type="string"><x:p>a <x:s/>b</x:p></t:table-cell>"#,
            r#"<t:table-cell o:value-type="string" o:string-value="" c:value-type="error"><x:p>#N/A</x:p></t:table-cell></t:table-row>"#,
            r#"<t:table-row><t:table-cell t:formula="of:=[.A1]"/></t:table-row>"#,
        )),
        "{saved}"
    );
    assert_eq!(
        Workbook::open(&file.path).unwrap().sheets[0]
            .cell(at("B1"))
            .value,
        CellValue::Error("#N/A".into())
    );
}

#[test]
fn an_error_is_written_as_text_when_the_file_does_not_declare_calcext() {
    let content = FOREIGN.replace(
        r#" xmlns:c="urn:org:documentfoundation:names:experimental:calc:xmlns:calcext:1.0""#,
        "",
    );
    let file = write_temp_as(&ods_package_with(ODS_MIMETYPE, &content, &[]), "book.ods");
    let mut book = Workbook::open(&file.path).unwrap();
    book.sheets[0].set_cell(at("A1"), Cell::value(CellValue::Error("#N/A".into())));
    book.save().unwrap();
    assert!(
        read_part(&file.path, "content.xml").unwrap().contains(
            r##"<t:table-cell o:value-type="string" o:string-value="#N/A"><x:p>#N/A</x:p></t:table-cell>"##
        )
    );
}

#[test]
fn a_flat_file_is_patched_in_place() {
    let original = common::fods(&sheet_of(&format!(
        "<table:table-row>{CELL_7}<table:table-cell/></table:table-row>"
    )));
    let file = write_temp_as(original.as_bytes(), "book.fods");
    let mut book = Workbook::open(&file.path).unwrap();
    book.sheets[0].set_cell(at("B1"), number(9.0));
    book.save().unwrap();
    let saved = std::fs::read_to_string(&file.path).unwrap();
    assert_eq!(
        saved,
        original.replace(
            "<table:table-cell/>",
            r#"<table:table-cell office:value-type="float" office:value="9"><text:p>9</text:p></table:table-cell>"#
        )
    );
}

#[test]
fn saving_with_no_edits_changes_nothing_and_another_program_changing_the_file_is_noticed() {
    let file = ods_file(&sheet_of(&format!(
        "<table:table-row>{CELL_7}</table:table-row>"
    )));
    let original = std::fs::read(&file.path).unwrap();
    let mut book = Workbook::open(&file.path).unwrap();
    book.save().unwrap();
    assert_eq!(std::fs::read(&file.path).unwrap(), original);

    book.sheets[0].set_cell(at("A1"), number(1.0));
    std::fs::write(&file.path, b"changed").unwrap();
    assert!(matches!(book.save(), Err(WorkbookError::ChangedOnDisk)));
}
