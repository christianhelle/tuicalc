//! Opening a workbook in the background, with its cells arriving while it's on screen.

mod common;

use std::path::Path;
use std::time::Duration;

use common::*;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use tuicalc::address::CellRef;
use tuicalc::app::{App, Mode};
use tuicalc::model::{Cell, CellValue};
use tuicalc::xlsx::{LoadEvent, SheetQueue, Workbook, WorkbookLoad};

const STILL_OPENING: &str = "Still opening: Esc cancels";

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

// The app with a workbook still arriving.

fn key(app: &mut App, code: KeyCode) {
    app.handle_key(KeyEvent::from(code));
}

fn ctrl(app: &mut App, c: char) {
    app.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL));
}

fn type_keys(app: &mut App, keys: &str) {
    for c in keys.chars() {
        key(app, KeyCode::Char(c));
    }
}

/// An app opening a workbook with numbers in column A, none of which have been taken in yet.
fn app_opening(rows: u32) -> (TempXlsx, App) {
    let file = write_temp(&single_sheet(&sheet(&numbers(rows)), &[]));
    let (workbook, load) = Workbook::open_loading(&file.path, 0).unwrap();
    (file, App::opening(workbook, load))
}

fn number_at(app: &App, at: &str) -> Option<f64> {
    match app.current_sheet().cell(a1(at)).value {
        CellValue::Number(number) => Some(number),
        _ => None,
    }
}

#[test]
fn an_app_can_show_a_workbook_before_its_cells_arrive() {
    let (_file, mut app) = app_opening(5000);

    assert!(app.is_loading());
    assert_eq!(app.mode(), Mode::Normal);
    assert_eq!(app.current_sheet().name, "Sheet1");
    assert_eq!(number_at(&app, "A1"), None);

    app.absorb_next_chunk();
    assert!(app.is_loading());
    assert_eq!(number_at(&app, "A1"), Some(1.0));
    assert_eq!(number_at(&app, "A5000"), None);

    app.finish_loading();
    assert!(!app.is_loading());
    assert_eq!(number_at(&app, "A5000"), Some(5000.0));
    assert!(!app.workbook.is_dirty(), "loading isn't editing");
}

#[test]
fn taking_in_cells_for_a_while_stops_when_they_run_out() {
    let (_file, mut app) = app_opening(10);

    app.absorb_loaded(Duration::from_secs(30));

    assert!(!app.is_loading());
    assert_eq!(number_at(&app, "A10"), Some(10.0));
}

#[test]
fn taking_in_cells_gives_up_when_time_runs_out() {
    let (_file, mut app) = app_opening(50_000);

    app.absorb_loaded(Duration::ZERO);

    assert!(app.is_loading());
}

#[test]
fn moving_around_works_while_loading() {
    let (_file, mut app) = app_opening(5000);
    app.absorb_next_chunk();

    type_keys(&mut app, "3jl");
    assert_eq!(app.cursor(), a1("B4"));

    type_keys(&mut app, "G");
    assert!(app.cursor().row > 3, "the last row so far");

    key(&mut app, KeyCode::Char('?'));
    assert_eq!(app.mode(), Mode::Help);
    key(&mut app, KeyCode::Esc);
    assert_eq!(app.mode(), Mode::Normal);
    assert!(!app.should_quit());
}

#[test]
fn q_quits_while_loading_without_asking() {
    let (_file, mut app) = app_opening(5000);

    key(&mut app, KeyCode::Char('q'));

    assert!(app.should_quit());
}

#[test]
fn esc_gives_up_on_opening_the_only_workbook() {
    let (_file, mut app) = app_opening(5000);

    key(&mut app, KeyCode::Esc);

    assert!(app.should_quit());
    assert_eq!(app.load_failure(), None, "giving up isn't a failure");
}

#[test]
fn esc_finishes_with_a_half_typed_command_first() {
    let (_file, mut app) = app_opening(5000);
    type_keys(&mut app, "d");

    key(&mut app, KeyCode::Esc);
    assert!(!app.should_quit());

    key(&mut app, KeyCode::Esc);
    assert!(app.should_quit());
}

#[test]
fn a_problem_deep_in_the_only_workbook_ends_the_app_and_says_why() {
    let broken = format!(r#"<worksheet><sheetData>{}</row>"#, numbers(2));
    let file = write_temp(&single_sheet(&broken, &[]));
    let (workbook, load) = Workbook::open_loading(&file.path, 0).unwrap();
    let mut app = App::opening(workbook, load);

    app.finish_loading();

    assert!(app.should_quit());
    assert!(!app.is_loading());
    assert!(app.load_failure().unwrap().contains("malformed XML"));
}

#[test]
fn nothing_can_change_or_be_copied_until_the_workbook_has_arrived() {
    let (file, mut app) = app_opening(5000);
    app.absorb_next_chunk();
    let before = std::fs::read(&file.path).unwrap();

    for keys in [
        "s", "i", "a", "=", "o", "x", "D", "dd", "cc", "yy", "S", "C", "Vd", "Vy", "Vc",
    ] {
        type_keys(&mut app, keys);
        assert_eq!(app.status(), STILL_OPENING, "{keys}");
        assert_ne!(app.mode(), Mode::Insert, "{keys}");
        if app.selection().is_some() {
            key(&mut app, KeyCode::Esc);
        }
        assert_eq!(app.mode(), Mode::Normal, "{keys}");
        assert!(!app.workbook.is_dirty(), "{keys}");
    }

    ctrl(&mut app, 's');
    assert_eq!(app.status(), STILL_OPENING, "saving");
    ctrl(&mut app, 'o');
    assert_eq!(app.status(), STILL_OPENING, "opening another");
    assert_eq!(app.mode(), Mode::Normal);
    key(&mut app, KeyCode::F(5));
    assert_eq!(app.status(), STILL_OPENING, "reloading");
    assert_eq!(std::fs::read(&file.path).unwrap(), before);
}

#[test]
fn zz_just_quits_while_loading_as_there_is_nothing_to_save() {
    let (_file, mut app) = app_opening(5000);

    type_keys(&mut app, "ZZ");

    assert!(app.should_quit());
}

#[test]
fn editing_and_saving_work_once_the_workbook_has_arrived() {
    let (file, mut app) = app_opening(5000);
    app.finish_loading();

    type_keys(&mut app, "s7");
    key(&mut app, KeyCode::Enter);
    ctrl(&mut app, 's');
    app.finish_saving();

    assert_eq!(app.status(), "Saved book.xlsx");
    let saved = Workbook::open(&file.path).unwrap();
    assert_eq!(saved.sheets[0].cell(a1("A1")).value, CellValue::Number(7.0));
    assert_eq!(
        saved.sheets[0].cell(a1("A5000")).value,
        CellValue::Number(5000.0)
    );
}

#[test]
fn a_search_while_loading_says_it_only_covered_what_has_arrived() {
    let (_file, mut app) = app_opening(5000);
    app.absorb_next_chunk();

    type_keys(&mut app, "/plums");
    key(&mut app, KeyCode::Enter);
    assert_eq!(app.status(), "No match for plums in what has loaded so far");

    type_keys(&mut app, "/");
    type_keys(&mut app, "=");
    key(&mut app, KeyCode::Esc);
    app.finish_loading();
    type_keys(&mut app, "/plums");
    key(&mut app, KeyCode::Enter);
    assert_eq!(app.status(), "No match for plums");
}

/// An app with `5` typed over A1 of a finished workbook holding numbers, and the cursor on A2.
fn app_with_an_edit() -> (TempXlsx, App) {
    let file = write_temp(&single_sheet(&sheet(&numbers(3)), &[]));
    let mut app = App::new(Workbook::open(&file.path).unwrap());
    type_keys(&mut app, "s5");
    key(&mut app, KeyCode::Enter);
    (file, app)
}

fn open_through_the_dialog(app: &mut App, path: &Path) {
    ctrl(app, 'o');
    if app.mode() == Mode::ConfirmDiscard {
        key(app, KeyCode::Char('d'));
    }
    type_keys(app, &path.display().to_string());
    key(app, KeyCode::Enter);
}

#[test]
fn opening_another_file_shows_it_straight_away_and_says_so_when_done() {
    let (first, mut app) = app_with_an_edit();
    let second = write_temp(&single_sheet(&sheet(&numbers(5000)), &[]));

    open_through_the_dialog(&mut app, &second.path);

    assert_eq!(app.mode(), Mode::Normal);
    assert!(app.is_loading());
    assert_eq!(app.workbook.path(), Some(second.path.as_path()));
    assert_ne!(app.workbook.path(), Some(first.path.as_path()));

    app.finish_loading();
    assert_eq!(app.status(), "Opened book.xlsx");
    assert_eq!(number_at(&app, "A5000"), Some(5000.0));
    key(&mut app, KeyCode::Char('u'));
    assert_eq!(app.status(), "Nothing to undo");
}

#[test]
fn esc_while_opening_another_file_returns_to_the_last_one_intact() {
    let (first, mut app) = app_with_an_edit();
    let second = write_temp(&single_sheet(&sheet(&numbers(5000)), &[]));

    open_through_the_dialog(&mut app, &second.path);
    key(&mut app, KeyCode::Esc);

    assert!(!app.should_quit());
    assert!(!app.is_loading());
    assert_eq!(app.workbook.path(), Some(first.path.as_path()));
    assert_eq!(app.cursor(), a1("A2"));
    assert_eq!(number_at(&app, "A1"), Some(5.0), "the edit is still there");
    assert!(app.workbook.is_dirty());
    assert_eq!(app.status(), "Stopped opening");
    key(&mut app, KeyCode::Char('u'));
    assert_eq!(number_at(&app, "A1"), Some(1.0), "and can be undone");
}

#[test]
fn a_problem_deep_in_another_file_returns_to_the_last_one_with_the_error() {
    let (first, mut app) = app_with_an_edit();
    let broken = format!(r#"<worksheet><sheetData>{}</row>"#, numbers(2));
    let second = write_temp(&single_sheet(&broken, &[]));

    open_through_the_dialog(&mut app, &second.path);
    app.finish_loading();

    assert!(!app.should_quit());
    assert_eq!(app.workbook.path(), Some(first.path.as_path()));
    assert_eq!(number_at(&app, "A1"), Some(5.0));
    assert!(app.status().starts_with("Open failed:"), "{}", app.status());
    assert!(app.status_is_error());
}

#[test]
fn a_file_that_is_not_a_workbook_still_stays_in_the_dialog() {
    let (first, mut app) = app_with_an_edit();
    let broken = write_temp(b"not a workbook");

    open_through_the_dialog(&mut app, &broken.path);

    assert_eq!(app.mode(), Mode::FileDialog);
    assert!(!app.is_loading());
    assert_eq!(app.workbook.path(), Some(first.path.as_path()));
}

#[test]
fn reloading_keeps_the_place_straight_away_and_survives_a_problem_deep_in_the_file() {
    let file = write_temp(&single_sheet(&sheet(&numbers(5000)), &[]));
    let mut app = App::new(Workbook::open(&file.path).unwrap());
    type_keys(&mut app, "3jl");

    key(&mut app, KeyCode::F(5));
    assert!(app.is_loading());
    assert_eq!(app.cursor(), a1("B4"));
    app.finish_loading();
    assert_eq!(app.status(), "Reloaded book.xlsx");
    assert_eq!(app.cursor(), a1("B4"));

    let broken = format!(r#"<worksheet><sheetData>{}</row>"#, numbers(2));
    std::fs::write(&file.path, single_sheet(&broken, &[])).unwrap();
    key(&mut app, KeyCode::F(5));
    app.finish_loading();
    assert!(
        app.status().starts_with("Reload failed:"),
        "{}",
        app.status()
    );
    assert_eq!(
        number_at(&app, "A5000"),
        Some(5000.0),
        "the last read stands"
    );
    assert_eq!(app.cursor(), a1("B4"));
}

#[test]
fn switching_sheets_while_loading_lands_on_the_sheet_and_it_fills_in() {
    let samples = Path::new(env!("CARGO_MANIFEST_DIR")).join("samples");
    let (workbook, load) = Workbook::open_loading(samples.join("financial-model.xlsx"), 0).unwrap();
    let mut app = App::opening(workbook, load);

    key(&mut app, KeyCode::F(6));
    assert_eq!(app.current_sheet().name, "Monthly Plan");

    app.finish_loading();
    assert_eq!(
        app.current_sheet().cell(a1("B4")).value,
        CellValue::Number(1000.0)
    );
}
