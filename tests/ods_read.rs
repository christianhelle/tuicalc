mod common;

use common::{TempXlsx, fods, ods_content, ods_package, ods_package_with, write_temp_as};
use tuicalc::address::CellRef;
use tuicalc::model::{Cell, CellValue};
use tuicalc::workbook::{Arrival, Workbook};

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
fn reads_dates_and_times_as_date_serials() {
    let book = open_row(concat!(
        r#"<table:table-cell office:value-type="date" office:date-value="2024-01-15"><text:p>15/01/24</text:p></table:table-cell>"#,
        r#"<table:table-cell office:value-type="date" office:date-value="2024-01-15T18:00:00"><text:p>x</text:p></table:table-cell>"#,
        r#"<table:table-cell office:value-type="time" office:time-value="PT06H00M00S"><text:p>06:00</text:p></table:table-cell>"#,
        r#"<table:table-cell office:value-type="time" office:time-value="PT36H00M00.000S"><text:p>36:00</text:p></table:table-cell>"#,
        r#"<table:table-cell office:value-type="date" office:date-value="1850-06-01"><text:p>x</text:p></table:table-cell>"#,
    ));
    let sheet = &book.sheets[0];
    let date = |serial| Cell {
        value: CellValue::Number(serial),
        formula: None,
        is_date: true,
    };
    assert_eq!(sheet.cell(at("A1")), date(45306.0));
    assert_eq!(sheet.cell(at("B1")), date(45306.75));
    assert_eq!(sheet.cell(at("C1")), date(0.25));
    assert_eq!(sheet.cell(at("D1")), date(1.5));
    assert_eq!(
        sheet.cell(at("E1")).value,
        CellValue::Text("1850-06-01".into()),
        "a date before any serial stays readable"
    );
}

#[test]
fn reads_errors_from_the_calcext_value_type_rather_than_as_text() {
    let book = open_row(
        r#"<table:table-cell table:formula="of:=1/0" office:value-type="string" office:string-value="" calcext:value-type="error"><text:p>#DIV/0!</text:p></table:table-cell>"#,
    );
    assert_eq!(
        book.sheets[0].cell(at("A1")).value,
        CellValue::Error("#DIV/0!".into())
    );
}

#[test]
fn a_calcext_value_type_never_replaces_the_office_one() {
    let book = open_row(concat!(
        r#"<table:table-cell calcext:value-type="float" office:value-type="float" office:value="2"><text:p>2</text:p></table:table-cell>"#,
        r#"<table:table-cell calcext:value-type="string" office:value-type="float" office:value="3"><text:p>3</text:p></table:table-cell>"#,
    ));
    assert_eq!(book.sheets[0].cell(at("A1")).value, CellValue::Number(2.0));
    assert_eq!(book.sheets[0].cell(at("B1")).value, CellValue::Number(3.0));
}

#[test]
fn repeated_cells_and_rows_fill_every_place_they_cover() {
    let book = open(concat!(
        r#"<table:table table:name="Sheet1">"#,
        r#"<table:table-row table:number-rows-repeated="2"><table:table-cell table:number-columns-repeated="2"/><table:table-cell office:value-type="float" office:value="7" table:number-columns-repeated="2"/></table:table-row>"#,
        r#"<table:table-row table:number-rows-repeated="3"/>"#,
        r#"<table:table-row><table:covered-table-cell/><table:table-cell office:value-type="float" office:value="8"/></table:table-row>"#,
        r#"</table:table>"#,
    ));
    let sheet = &book.sheets[0];
    for reference in ["C1", "D1", "C2", "D2"] {
        assert_eq!(sheet.cell(at(reference)).value, CellValue::Number(7.0));
    }
    assert!(!sheet.has_cell(at("B1")));
    assert!(!sheet.has_cell(at("E1")));
    assert!(!sheet.has_cell(at("C3")));
    assert_eq!(sheet.cell(at("B6")).value, CellValue::Number(8.0));
    assert_eq!(sheet.extent(), Some(at("D6")));
}

#[test]
fn the_padding_spreadsheet_apps_add_to_fill_a_sheet_makes_no_cells() {
    let book = open(concat!(
        r#"<table:table table:name="Sheet1">"#,
        r#"<table:table-row><table:table-cell office:value-type="float" office:value="1"/><table:table-cell table:style-name="ce1" table:number-columns-repeated="1048575"/></table:table-row>"#,
        r#"<table:table-row table:number-rows-repeated="4294967295"><table:table-cell table:number-columns-repeated="4294967295"/></table:table-row>"#,
        r#"</table:table>"#,
    ));
    assert_eq!(book.sheets[0].extent(), Some(at("A1")));
    assert_eq!(book.sheets[0].cells().count(), 1);
}

#[test]
fn filled_repeats_stop_at_the_edges_of_a_sheet() {
    let book = open(concat!(
        r#"<table:table table:name="Sheet1"><table:table-row table:number-rows-repeated="1048570"/>"#,
        r#"<table:table-row table:number-rows-repeated="100"><table:table-cell office:value-type="float" office:value="1"/></table:table-row>"#,
        r#"</table:table>"#,
    ));
    assert_eq!(book.sheets[0].cells().count(), 6);
    assert_eq!(book.sheets[0].extent(), Some(at("A1048576")));
}

#[test]
fn a_tiny_file_cannot_ask_for_billions_of_cells() {
    let file = ods_file(concat!(
        r#"<table:table table:name="Sheet1"><table:table-row table:number-rows-repeated="1048576"><table:table-cell office:value-type="float" office:value="1" table:number-columns-repeated="16384"/></table:table-row></table:table>"#,
    ));
    let error = Workbook::open(&file.path).unwrap_err();
    assert!(error.to_string().contains("too many"), "{error}");
}

#[test]
fn notes_drawings_and_sub_tables_stay_out_of_the_cells() {
    let book = open(concat!(
        r#"<table:table table:name="Sheet1">"#,
        r#"<table:shapes><draw:frame><draw:text-box><text:p>a shape</text:p></draw:text-box></draw:frame></table:shapes>"#,
        r#"<table:table-row>"#,
        r#"<table:table-cell office:value-type="string"><office:annotation><dc:creator>Sam</dc:creator><text:p>a note</text:p></office:annotation><text:p>shown</text:p></table:table-cell>"#,
        r#"<table:table-cell office:value-type="string"><text:p>before<office:annotation><text:p>inner note</text:p></office:annotation> after</text:p></table:table-cell>"#,
        r#"<table:table-cell><draw:frame><draw:text-box><text:p>in a frame</text:p></draw:text-box></draw:frame></table:table-cell>"#,
        r#"<table:table-cell table:is-sub-table="true"><table:table table:name="inner"><table:table-row><table:table-cell office:value-type="float" office:value="9"/></table:table-row></table:table></table:table-cell>"#,
        r#"<table:table-cell office:value-type="float" office:value="5"/>"#,
        r#"</table:table-row>"#,
        r#"<table:table-row><table:table-cell office:value-type="float" office:value="6"/></table:table-row>"#,
        r#"</table:table>"#,
    ));
    let sheet = &book.sheets[0];
    assert_eq!(sheet.cell(at("A1")).value, CellValue::Text("shown".into()));
    assert_eq!(
        sheet.cell(at("B1")).value,
        CellValue::Text("before after".into())
    );
    assert!(!sheet.has_cell(at("C1")));
    assert!(!sheet.has_cell(at("D1")));
    assert_eq!(sheet.cell(at("E1")).value, CellValue::Number(5.0));
    assert_eq!(sheet.cell(at("A2")).value, CellValue::Number(6.0));
    assert_eq!(book.sheets.len(), 1);
}

#[test]
fn group_and_header_rows_count_as_rows() {
    let book = open(concat!(
        r#"<table:table table:name="Sheet1">"#,
        r#"<table:table-header-rows><table:table-row><table:table-cell office:value-type="float" office:value="1"/></table:table-row></table:table-header-rows>"#,
        r#"<table:table-row-group><table:table-rows><table:table-row><table:table-cell office:value-type="float" office:value="2"/></table:table-row></table:table-rows></table:table-row-group>"#,
        r#"<table:table-row><table:table-cell office:value-type="float" office:value="3"/></table:table-row>"#,
        r#"</table:table>"#,
    ));
    let sheet = &book.sheets[0];
    assert_eq!(sheet.cell(at("A1")).value, CellValue::Number(1.0));
    assert_eq!(sheet.cell(at("A2")).value, CellValue::Number(2.0));
    assert_eq!(sheet.cell(at("A3")).value, CellValue::Number(3.0));
}

#[test]
fn array_formula_regions_are_read_only() {
    let book = open(
        r#"<table:table table:name="Sheet1"><table:table-row><table:table-cell table:number-columns-repeated="1"/><table:table-cell table:formula="of:={1;2}" table:number-matrix-columns-spanned="2" table:number-matrix-rows-spanned="3" office:value-type="float" office:value="1"/></table:table-row></table:table>"#,
    );
    let sheet = &book.sheets[0];
    for reference in ["B1", "C1", "B3", "C3"] {
        assert!(sheet.is_locked(at(reference)), "{reference}");
    }
    for reference in ["A1", "D1", "B4"] {
        assert!(!sheet.is_locked(at(reference)), "{reference}");
    }
}

fn formula_of(stored: &str) -> Option<String> {
    let escaped = stored
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;");
    let book = open_row(&format!(
        r#"<table:table-cell table:formula="{escaped}" office:value-type="float" office:value="1"/>"#
    ));
    book.sheets[0].cell(at("A1")).formula
}

#[test]
fn formulas_read_as_excel_style_text() {
    let cases = [
        ("of:=[.A1]+1", "A1+1"),
        ("of:=SUM([.A1:.B2];[.$C$3])", "SUM(A1:B2,$C$3)"),
        ("of:=[$Sheet2.C3]*2", "Sheet2!C3*2"),
        ("of:=[Sheet2.C3:.D4]", "Sheet2!C3:D4"),
        ("of:=SUM(['My Sheet'.A1:.A9])", "SUM('My Sheet'!A1:A9)"),
        ("of:=SUM([.A:.B])", "SUM(A:B)"),
        ("of:=SUM([.$1:.2])", "SUM($1:2)"),
        ("of:=[.#REF!]+1", "#REF!+1"),
        ("of:=[#REF!]", "#REF!"),
        (r#"of:=IF([.A1]="x;[.y]";1;2)"#, r#"IF(A1="x;[.y]",1,2)"#),
        ("of:=SUM({1;2|3;4})", "SUM({1,2;3,4})"),
        ("oooc:=[.A1]", "A1"),
        ("msoxl:=A1+1", "A1+1"),
        ("=[.A1]", "A1"),
        ("of:=TaxRate*[.A1]", "TaxRate*A1"),
    ];
    for (stored, shown) in cases {
        assert_eq!(formula_of(stored).as_deref(), Some(shown), "{stored}");
    }
}

#[test]
fn a_formula_that_cannot_be_converted_is_kept_as_it_is() {
    assert_eq!(
        formula_of("of:=['file:///x.ods'#$Sheet1.A1]").as_deref(),
        Some("['file:///x.ods'#$Sheet1.A1]")
    );
    assert_eq!(
        formula_of("ooow:=<A1>").as_deref(),
        Some("ooow:=<A1>"),
        "an unknown grammar"
    );
}

#[test]
fn a_formula_keeps_its_cached_result() {
    let book = open_row(
        r#"<table:table-cell table:formula="of:=1+1" office:value-type="float" office:value="2"/><table:table-cell table:formula="of:=&quot;a&quot;" office:value-type="string" office:string-value="a"><text:p>a</text:p></table:table-cell><table:table-cell table:formula="of:=NOW()"/>"#,
    );
    let sheet = &book.sheets[0];
    assert_eq!(sheet.cell(at("A1")).value, CellValue::Number(2.0));
    assert_eq!(sheet.cell(at("B1")).value, CellValue::Text("a".into()));
    assert_eq!(sheet.cell(at("C1")).value, CellValue::Empty);
    assert_eq!(sheet.cell(at("C1")).formula.as_deref(), Some("NOW()"));
}

const TWO_SHEETS: &str = concat!(
    r#"<table:table table:name="One"><table:table-row><table:table-cell office:value-type="float" office:value="1"/></table:table-row></table:table>"#,
    r#"<table:table table:name="Two"><table:table-row><table:table-cell office:value-type="float" office:value="2"/></table:table-row></table:table>"#,
);

#[test]
fn a_load_shows_the_sheets_at_once_and_delivers_their_cells_after() {
    let file = ods_file(TWO_SHEETS);
    let (mut book, load) = Workbook::open_loading(&file.path, 1).unwrap();
    assert_eq!(book.sheets.len(), 2);
    assert_eq!(book.sheets[1].name, "Two");
    assert!(!book.date1904);
    load.prefer(1);
    loop {
        match load.receive(&mut book) {
            Arrival::Cells | Arrival::Nothing => {}
            Arrival::Ended => break,
            Arrival::Failed(error) => panic!("{error}"),
        }
    }
    assert_eq!(book.sheets[0].cell(at("A1")).value, CellValue::Number(1.0));
    assert_eq!(book.sheets[1].cell(at("A1")).value, CellValue::Number(2.0));
    assert!((load.progress() - 1.0).abs() < f64::EPSILON);
}

#[test]
fn broken_content_fails_at_opening() {
    let file = ods_file(
        r#"<table:table table:name="One"><table:table-row><table:table-cell></table:table-row></table:table>"#,
    );
    let opened = Workbook::open_loading(&file.path, 0);
    assert!(opened.is_err(), "the sheet list needs the whole content");
}

#[test]
fn templates_open_like_any_other_spreadsheet() {
    let bytes = ods_package_with(
        "application/vnd.oasis.opendocument.spreadsheet-template",
        &ods_content(TWO_SHEETS),
        &[],
    );
    let file = write_temp_as(&bytes, "book.ots");
    let book = Workbook::open(&file.path).unwrap();
    assert_eq!(book.sheets[1].cell(at("A1")).value, CellValue::Number(2.0));
}

#[test]
fn flat_files_open_whatever_they_are_called() {
    let file = write_temp_as(fods(TWO_SHEETS).as_bytes(), "book.fods");
    let book = Workbook::open(&file.path).unwrap();
    assert_eq!(book.sheets[1].name, "Two");
    assert_eq!(book.sheets[1].cell(at("A1")).value, CellValue::Number(2.0));

    let (mut book, load) = Workbook::open_loading(&file.path, 0).unwrap();
    while !matches!(load.receive(&mut book), Arrival::Ended) {}
    assert_eq!(book.sheets[0].cell(at("A1")).value, CellValue::Number(1.0));
}

#[test]
fn other_opendocument_files_are_refused_by_name() {
    let bytes = ods_package_with(
        "application/vnd.oasis.opendocument.text",
        "<office:document-content/>",
        &[],
    );
    let file = write_temp_as(&bytes, "letter.odt");
    let error = Workbook::open(&file.path).unwrap_err();
    assert!(error.to_string().contains("not a spreadsheet"), "{error}");
    assert!(Workbook::open_loading(&file.path, 0).is_err());
}

#[test]
fn a_flat_file_of_another_kind_is_refused_by_name() {
    let xml = r#"<?xml version="1.0"?><office:document xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" office:mimetype="application/vnd.oasis.opendocument.text"/>"#;
    let file = write_temp_as(xml.as_bytes(), "letter.fodt");
    let error = Workbook::open(&file.path).unwrap_err();
    assert!(error.to_string().contains("not a spreadsheet"), "{error}");
}

#[test]
fn encrypted_files_are_refused_by_name() {
    let manifest = r#"<manifest:manifest xmlns:manifest="urn:oasis:names:tc:opendocument:xmlns:manifest:1.0"><manifest:file-entry manifest:full-path="content.xml" manifest:media-type="text/xml"><manifest:encryption-data manifest:checksum="x"/></manifest:file-entry></manifest:manifest>"#;
    let bytes = ods_package_with(
        common::ODS_MIMETYPE,
        "not xml, as it is encrypted",
        &[("META-INF/manifest.xml", manifest)],
    );
    let file = write_temp_as(&bytes, "secret.ods");
    let error = Workbook::open(&file.path).unwrap_err();
    assert!(error.to_string().contains("password"), "{error}");
}

#[test]
fn a_spreadsheet_with_no_sheets_is_refused() {
    let file = ods_file("");
    let error = Workbook::open(&file.path).unwrap_err();
    assert!(error.to_string().contains("no worksheets"), "{error}");
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
