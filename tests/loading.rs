//! Opening a workbook in the background, with its cells arriving while it's on screen.

mod common;

use std::path::Path;

use common::*;
use tuicalc::address::CellRef;
use tuicalc::model::{Cell, CellValue};
use tuicalc::xlsx::{LoadEvent, SheetQueue, Workbook, WorkbookLoad};

fn a1(text: &str) -> CellRef {
    CellRef::parse(text).unwrap()
}

fn load_everything(workbook: &mut Workbook, load: &WorkbookLoad) {
    while let Some(event) = load.next_blocking() {
        match event {
            LoadEvent::Cells { sheet, chunk } => chunk.apply_to(&mut workbook.sheets[sheet]),
            LoadEvent::Failed(error) => panic!("{error}"),
        }
    }
}

fn numbers(rows: u32) -> String {
    (1..=rows)
        .map(|row| format!(r#"<row r="{row}"><c r="A{row}"><v>{row}</v></c></row>"#))
        .collect()
}

#[test]
fn the_sheets_are_known_before_any_cells_arrive() {
    let file = write_temp(&single_sheet(&sheet(&numbers(10)), &[]));

    let (workbook, _load) = Workbook::open_loading(&file.path, 0).unwrap();

    assert_eq!(workbook.sheets.len(), 1);
    assert_eq!(workbook.sheets[0].name, "Sheet1");
    assert_eq!(workbook.sheets[0].cells().count(), 0);
    assert_eq!(workbook.path(), Some(file.path.as_path()));
}

#[test]
fn loading_produces_the_same_workbook_as_opening_it_outright() {
    let samples = Path::new(env!("CARGO_MANIFEST_DIR")).join("samples");
    for name in [
        "dates-1904.xlsx",
        "financial-model.xlsx",
        "foreign-writer.xlsx",
        "formula-edge-cases.xlsx",
        "sales-pivot.xlsx",
        "text-and-layout.xlsx",
    ] {
        let path = samples.join(name);
        let opened = Workbook::open(&path).unwrap();

        let (mut loaded, load) = Workbook::open_loading(&path, 0).unwrap();
        load_everything(&mut loaded, &load);

        assert_eq!(loaded.date1904, opened.date1904, "{name}");
        assert_eq!(loaded.sheets.len(), opened.sheets.len(), "{name}");
        for (ours, theirs) in loaded.sheets.iter().zip(&opened.sheets) {
            assert_eq!(ours.name, theirs.name, "{name}");
            let expected: Vec<_> = theirs.cells().collect();
            let actual: Vec<_> = ours.cells().collect();
            assert_eq!(actual, expected, "{name} {}", ours.name);
            for (at, _) in theirs.cells() {
                assert_eq!(ours.is_locked(*at), theirs.is_locked(*at), "{name} {at}");
            }
        }
    }
}

#[test]
fn the_first_chunk_of_a_large_sheet_is_about_a_screenful() {
    let file = write_temp(&single_sheet(&sheet(&numbers(5000)), &[]));

    let (_workbook, load) = Workbook::open_loading(&file.path, 0).unwrap();

    let Some(LoadEvent::Cells { chunk, .. }) = load.next_blocking() else {
        panic!("no cells");
    };
    assert!(
        (1..=500).contains(&chunk.cells.len()),
        "{}",
        chunk.cells.len()
    );
    assert_eq!(
        chunk.cells[0],
        (a1("A1"), Cell::value(CellValue::Number(1.0)))
    );
}

#[test]
fn a_file_that_is_not_a_workbook_fails_straight_away() {
    let file = write_temp(b"not a workbook");
    assert!(Workbook::open_loading(&file.path, 0).is_err());

    assert!(Workbook::open_loading(file.path.with_file_name("missing.xlsx"), 0).is_err());
}

#[test]
fn a_problem_deep_in_a_sheet_arrives_after_the_cells_before_it() {
    let broken = format!(r#"<worksheet><sheetData>{}</row>"#, numbers(2));
    let file = write_temp(&single_sheet(&broken, &[]));

    let (_workbook, load) = Workbook::open_loading(&file.path, 0).unwrap();

    let mut failed = false;
    while let Some(event) = load.next_blocking() {
        if let LoadEvent::Failed(error) = event {
            assert!(error.to_string().contains("malformed XML"), "{error}");
            failed = true;
        }
    }
    assert!(failed);
    assert!(
        Workbook::open(&file.path).is_err(),
        "opening outright agrees"
    );
}

#[test]
fn progress_runs_from_nothing_to_everything() {
    let file = write_temp(&single_sheet(&sheet(&numbers(5000)), &[]));

    let (mut workbook, load) = Workbook::open_loading(&file.path, 0).unwrap();
    assert!((0.0..=1.0).contains(&load.progress()));

    load_everything(&mut workbook, &load);
    assert_eq!(load.progress(), 1.0);
}

#[test]
fn dropping_the_load_stops_it() {
    let samples = Path::new(env!("CARGO_MANIFEST_DIR")).join("samples");

    let (_workbook, load) = Workbook::open_loading(samples.join("sales-pivot.xlsx"), 0).unwrap();
    drop(load);

    // Nothing to assert beyond not hanging: its workers fail to send, and stop.
}

#[test]
fn the_queue_starts_the_preferred_sheet_first_then_the_rest_in_order() {
    let queue = SheetQueue::new(4, 2);
    assert_eq!(queue.next(), Some(2));
    assert_eq!(queue.next(), Some(0));

    queue.prefer(3);
    assert_eq!(queue.next(), Some(3));

    queue.prefer(0);
    assert_eq!(queue.next(), Some(1), "the preferred sheet already started");
    assert_eq!(queue.next(), None);
}

#[test]
fn opening_on_a_later_sheet_loads_it_too() {
    let samples = Path::new(env!("CARGO_MANIFEST_DIR")).join("samples");

    let (mut workbook, load) =
        Workbook::open_loading(samples.join("financial-model.xlsx"), 3).unwrap();
    load_everything(&mut workbook, &load);

    assert!(workbook.sheets.iter().all(|sheet| sheet.extent().is_some()));
}
