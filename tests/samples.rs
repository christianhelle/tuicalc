//! Runs the workbooks in `samples/` through the reader and writer. Expected values come from
//! `samples/generate.ps1`, which is what put them there.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};

use tuicalc::address::CellRef;
use tuicalc::model::{Cell, CellValue, Sheet};
use tuicalc::xlsx::Workbook;
use zip::ZipArchive;

const SAMPLES: [&str; 6] = [
    "dates-1904.xlsx",
    "financial-model.xlsx",
    "foreign-writer.xlsx",
    "formula-edge-cases.xlsx",
    "sales-pivot.xlsx",
    "text-and-layout.xlsx",
];

fn sample_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("samples")
        .join(name)
}

fn open(name: &str) -> Workbook {
    Workbook::open(sample_path(name)).unwrap_or_else(|error| panic!("{name}: {error}"))
}

fn a1(text: &str) -> CellRef {
    CellRef::parse(text).unwrap()
}

fn sheet<'a>(book: &'a Workbook, name: &str) -> &'a Sheet {
    book.sheets
        .iter()
        .find(|sheet| sheet.name == name)
        .unwrap_or_else(|| panic!("no sheet {name}"))
}

fn text(value: &str) -> CellValue {
    CellValue::Text(value.to_string())
}

fn number(sheet: &Sheet, at: &str) -> f64 {
    match sheet.cell(a1(at)).value {
        CellValue::Number(number) => number,
        other => panic!("{at} is {other:?}"),
    }
}

fn formula(sheet: &Sheet, at: &str) -> String {
    sheet
        .cell(a1(at))
        .formula
        .unwrap_or_else(|| panic!("{at} has no formula"))
}

fn parts(path: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut archive = ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
    (0..archive.len())
        .map(|index| {
            let mut file = archive.by_index(index).unwrap();
            let name = file.name().unwrap().to_string();
            let mut content = Vec::new();
            file.read_to_end(&mut content).unwrap();
            (name, content)
        })
        .collect()
}

#[test]
fn financial_model_reads_names_cross_sheet_formulas_and_hidden_sheets() {
    let book = open("financial-model.xlsx");
    let names: Vec<_> = book
        .sheets
        .iter()
        .map(|sheet| sheet.name.as_str())
        .collect();
    assert_eq!(names, ["Assumptions", "Monthly Plan", "Lookups", "Summary"]);

    let plan = sheet(&book, "Monthly Plan");
    assert_eq!(plan.cell(a1("A1")).value, text("Monthly plan"));
    assert_eq!(number(plan, "B4"), 1000.0);
    assert_eq!(formula(plan, "B5"), "B4*(1+Growth/12)");
    assert_eq!(
        formula(plan, "B15"),
        "B14*(1+Growth/12)",
        "a shared formula's last cell"
    );
    assert!((number(plan, "B5") - 1004.1666666).abs() < 1e-6);
    assert_eq!(number(plan, "D4"), 400.0);
    assert_eq!(formula(plan, "F15"), "D15-E15");
    assert!(plan.cell(a1("A4")).is_date, "mmm yyyy is a date format");
    assert!(
        !plan.cell(a1("B4")).is_date,
        "[$USD] is a currency, not a date"
    );

    let summary = sheet(&book, "Summary");
    assert_eq!(formula(summary, "B1"), "'Monthly Plan'!F16");
    assert_eq!(summary.cell(a1("B3")).value, text("Large"));
    assert_eq!(summary.cell(a1("B4")).value, CellValue::Bool(true));
    assert_eq!(summary.cell(a1("A6")).value, text("Model notes"));
    assert_eq!(sheet(&book, "Lookups").cell(a1("B4")).value, text("Large"));
    assert!(sheet(&book, "Assumptions").cell(a1("B5")).is_date);
}

#[test]
fn sales_pivot_reads_tables_and_pivot_output() {
    let book = open("sales-pivot.xlsx");

    let sales = sheet(&book, "Sales");
    assert_eq!(sales.extent(), Some(a1("F1501")));
    assert!(sales.cell(a1("A2")).is_date);
    let revenue = formula(sales, "F1501");
    assert!(
        revenue.contains("[Units]") && revenue.contains("[Price]"),
        "{revenue}"
    );
    assert_eq!(
        number(sales, "F2"),
        number(sales, "D2") * number(sales, "E2")
    );

    let checks = sheet(&book, "Checks");
    assert_eq!(number(checks, "B1"), 1500.0);
    assert_eq!(formula(checks, "B1"), "ROWS(SalesTable[Date])");

    let report = sheet(&book, "Report");
    assert_eq!(
        report.cell(a1("A1")).value,
        text("Revenue by region and product")
    );
    assert_eq!(report.cell(a1("B3")).value, text("Column Labels"));
    assert_eq!(report.cell(a1("A5")).value, text("East"));
    assert!(
        (number(checks, "B2") - number(report, "G9")).abs() < 1e-6,
        "pivot grand total"
    );
}

#[test]
fn formula_edge_cases_lock_arrays_spills_and_data_tables() {
    let book = open("formula-edge-cases.xlsx");

    let arrays = sheet(&book, "Arrays");
    assert_eq!(formula(arrays, "B1"), "A1:A3*10");
    assert_eq!(
        number(arrays, "B3"),
        30.0,
        "an array member keeps its value"
    );
    assert!(arrays.is_locked(a1("B3")), "legacy array member");
    assert_eq!(number(arrays, "C1"), 14.0);
    assert_eq!(formula(arrays, "E1"), "_xlfn.SEQUENCE(4,2)");
    assert_eq!(number(arrays, "F4"), 8.0, "last cell of the spill");
    assert!(arrays.is_locked(a1("F4")), "spilled cell");
    assert_eq!(number(arrays, "I5"), 90.0, "data table result");
    assert!(arrays.is_locked(a1("I4")), "data table member");
    assert!(!arrays.is_locked(a1("A1")));
}

#[test]
fn formula_edge_cases_read_every_error_and_cached_type() {
    let book = open("formula-edge-cases.xlsx");
    let errors = sheet(&book, "Errors");
    let expected = [
        "#DIV/0!", "#N/A", "#REF!", "#VALUE!", "#NAME?", "#NUM!", "#NULL!",
    ];
    for (row, error) in expected.iter().enumerate() {
        let at = CellRef {
            col: 0,
            row: row as u32,
        };
        assert_eq!(
            errors.cell(at).value,
            CellValue::Error((*error).to_string()),
            "{at}"
        );
        assert!(errors.cell(at).formula.is_some(), "{at}");
    }
    assert_eq!(errors.cell(a1("C1")).value, CellValue::Bool(true));
    assert_eq!(errors.cell(a1("C2")).value, CellValue::Bool(false));
    assert_eq!(errors.cell(a1("C3")).value, CellValue::Bool(true));
    assert_eq!(errors.cell(a1("C4")).value, text("text"));
    assert_eq!(errors.cell(a1("C5")).formula.as_deref(), Some("\"\""));
    assert_eq!(errors.cell(a1("D1")).value, text("caught"));
}

#[test]
fn formula_edge_cases_expand_shared_formulas_like_excel() {
    let book = open("formula-edge-cases.xlsx");
    let shared = sheet(&book, "Shared");
    assert_eq!(formula(shared, "C6"), "A6+B6");
    assert_eq!(formula(shared, "D4"), "$A$1+A4+$B4+B$1");
    assert_eq!(formula(shared, "H1"), "D1*2");
    assert_eq!(formula(shared, "F8"), "SUM(B:B)");
    assert_eq!(formula(shared, "H8"), "SUM(D:D)");
    assert_eq!(formula(shared, "E12"), "SUM(4:4)");
    assert_eq!(
        formula(shared, "E3"),
        "\"A1\"&LOG10(A3)&IF(A3>1,\"$B$2\",A2)"
    );
}

#[test]
fn formula_edge_cases_read_sheet_names_needing_escapes() {
    let book = open("formula-edge-cases.xlsx");
    let odd = sheet(&book, "R&D <2024> \"é\"");
    assert_eq!(
        odd.cell(a1("A1")).value,
        text("Referenced from another sheet")
    );
    let cross = sheet(&book, "Cross refs");
    assert_eq!(formula(cross, "A1"), "'R&D <2024> \"é\"'!A1");
    assert_eq!(
        cross.cell(a1("A1")).value,
        text("Referenced from another sheet")
    );
    assert_eq!(formula(cross, "A4"), "Shared!C3+Arrays!A3");
}

#[test]
fn text_and_layout_reads_awkward_text() {
    let book = open("text-and-layout.xlsx");
    let sheet_text = sheet(&book, "Text");
    for (at, expected) in [
        ("A1", "007"),
        ("A2", "=NOT A FORMULA"),
        ("A3", "TRUE"),
        ("A4", "1e5"),
        ("B1", "plain and bold and red"),
        ("B2", "日本語のテキスト"),
        ("B3", "📊 chart 🚀"),
        ("B4", "مرحبا"),
        ("B5", "line one\nline two\nline three"),
        ("B6", "  surrounded by spaces  "),
        ("B7", "markup <b>&amp;</b> \"quotes\" 'apostrophes'"),
        ("B8", "literal _x0041_ escape"),
    ] {
        assert_eq!(sheet_text.cell(a1(at)).value, text(expected), "{at}");
    }
    match sheet_text.cell(a1("B9")).value {
        CellValue::Text(long) => assert_eq!(long.len(), 5000),
        other => panic!("{other:?}"),
    }
}

#[test]
fn text_and_layout_reaches_the_far_corner_of_the_sheet() {
    let book = open("text-and-layout.xlsx");
    let layout = sheet(&book, "Layout");
    assert_eq!(layout.extent(), Some(a1("XFD1048576")));
    assert_eq!(
        layout.cell(a1("XFD1048576")).value,
        text("last cell on the sheet")
    );
    assert_eq!(layout.cell(a1("Z100")).value, text("further out"));
    assert_eq!(layout.cell(a1("A2")).value, text("in hidden row"));
    assert_eq!(layout.cell(a1("B1")).value, text("in hidden column"));
    assert_eq!(layout.cell(a1("D4")).value, text("merged block"));
}

#[test]
fn dates_1904_tells_dates_from_other_number_formats() {
    let book = open("dates-1904.xlsx");
    assert!(book.date1904);
    let formats = sheet(&book, "Formats");
    let mut date_rows = Vec::new();
    for row in 2..=15 {
        if formats.cell(a1(&format!("B{row}"))).is_date {
            date_rows.push(row);
        }
    }
    assert_eq!(date_rows, [2, 3, 4, 5, 6, 7, 8], "yyyy-mm-dd through mm:ss");
    assert_eq!(
        number(formats, "D2"),
        43538.0,
        "DATE(2023,3,15) in the 1904 system"
    );
    assert_eq!(formula(formats, "D3"), "B2+1");
}

#[test]
fn foreign_writer_reads_prefixes_missing_references_and_moved_parts() {
    let book = open("foreign-writer.xlsx");
    let prefixed = sheet(&book, "Prefixed");
    assert_eq!(prefixed.cell(a1("A1")).value, text("inline & prefixed"));
    assert_eq!(formula(prefixed, "A3"), "B1*2");
    assert_eq!(prefixed.cell(a1("B3")).value, CellValue::Bool(true));

    let bare = sheet(&book, "No references");
    assert_eq!(bare.cell(a1("A1")).value, text("first"));
    assert_eq!(number(bare, "B1"), 2.0);
    assert_eq!(number(bare, "A3"), 3.0);
}

/// Edits one spare cell on every sheet, then checks nothing else moved.
#[test]
fn every_sample_survives_an_edit_on_every_sheet() {
    let spare = a1("AB2000");
    let edit = Cell::value(CellValue::Number(12345.5));
    for name in SAMPLES {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(name);
        std::fs::copy(sample_path(name), &path).unwrap();
        let original = Workbook::open(&path).unwrap();
        let original_parts = parts(&path);

        let mut book = Workbook::open(&path).unwrap();
        for sheet in &mut book.sheets {
            sheet.set_cell(spare, edit.clone());
        }
        book.save()
            .unwrap_or_else(|error| panic!("{name}: {error}"));

        let saved = Workbook::open(&path).unwrap_or_else(|error| panic!("{name}: {error}"));
        assert_eq!(saved.sheets.len(), original.sheets.len(), "{name}");
        for (before, after) in original.sheets.iter().zip(&saved.sheets) {
            assert_eq!(after.cell(spare), edit, "{name} {}", after.name);
            let unchanged: Vec<_> = after.cells().filter(|(at, _)| **at != spare).collect();
            let expected: Vec<_> = before.cells().collect();
            assert_eq!(unchanged, expected, "{name} {}", after.name);
        }

        let saved_parts = parts(&path);
        for (part, content) in &original_parts {
            let may_change = part.starts_with("xl/worksheets/sheet")
                || part.contains("first.xml")
                || part.contains("second.xml")
                || part.ends_with("workbook.xml")
                || part.ends_with("book.xml")
                || part.ends_with("workbook.xml.rels")
                || part == "[Content_Types].xml";
            if part == "xl/calcChain.xml" {
                assert!(!saved_parts.contains_key(part), "{name}: stale calc chain");
            } else if !may_change {
                assert_eq!(
                    saved_parts.get(part),
                    Some(content),
                    "{name}: {part} changed"
                );
            }
        }
    }
}
