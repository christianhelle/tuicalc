//! Saving a workbook in the background, with the app still usable while the file is written.

mod common;

use std::time::Duration;

use common::*;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use tuicalc::address::CellRef;
use tuicalc::app::{App, Mode};
use tuicalc::model::{Cell, CellValue};
use tuicalc::xlsx::{Workbook, XlsxError};

fn a1(text: &str) -> CellRef {
    CellRef::parse(text).unwrap()
}

fn numbers(rows: u32) -> String {
    (1..=rows)
        .map(|row| format!(r#"<row r="{row}"><c r="A{row}"><v>{row}</v></c></row>"#))
        .collect()
}

fn number_on_disk(path: &std::path::Path, at: &str) -> CellValue {
    Workbook::open(path).unwrap().sheets[0].cell(a1(at)).value
}

/// A workbook with numbers in column A and `5` typed over A1.
fn edited_book() -> (TempXlsx, Workbook) {
    let file = write_temp(&single_sheet(&sheet(&numbers(3)), &[]));
    let mut book = Workbook::open(&file.path).unwrap();
    book.sheets[0].set_cell(a1("A1"), Cell::value(CellValue::Number(5.0)));
    (file, book)
}

#[test]
fn a_workbook_stays_unsaved_until_its_finished_save_is_taken_in() {
    let (file, mut book) = edited_book();

    let save = book.begin_save().unwrap();
    assert!(book.is_dirty());

    let saved = save
        .result_within(Duration::from_secs(30))
        .unwrap()
        .unwrap();
    assert!(book.is_dirty());
    assert_eq!(number_on_disk(&file.path, "A1"), CellValue::Number(5.0));

    book.finish_save(saved);
    assert!(!book.is_dirty());

    book.sheets[0].set_cell(a1("A2"), Cell::value(CellValue::Number(6.0)));
    book.save().unwrap();
    assert_eq!(number_on_disk(&file.path, "A1"), CellValue::Number(5.0));
    assert_eq!(number_on_disk(&file.path, "A2"), CellValue::Number(6.0));
}

#[test]
fn a_file_changed_on_disk_fails_the_save_and_keeps_the_edits() {
    let (file, book) = edited_book();
    std::fs::write(&file.path, b"replaced by something else").unwrap();

    let result = book.begin_save().unwrap().wait();

    assert!(matches!(result, Err(XlsxError::ChangedOnDisk)));
    assert!(book.is_dirty());
    assert_eq!(
        std::fs::read(&file.path).unwrap(),
        b"replaced by something else"
    );
}

#[test]
fn a_workbook_without_a_file_cannot_begin_a_plain_save() {
    let book = Workbook::blank().unwrap();

    assert!(matches!(book.begin_save(), Err(XlsxError::NoFileName)));
}

#[test]
fn saving_to_a_new_file_moves_the_workbook_there_once_taken_in() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("new.xlsx");
    let mut book = Workbook::blank().unwrap();
    book.sheets[0].set_cell(a1("A1"), Cell::value(CellValue::Number(5.0)));

    let saved = book.begin_save_as(&path).wait().unwrap();
    assert_eq!(book.path(), None);
    assert_eq!(number_on_disk(&path, "A1"), CellValue::Number(5.0));

    book.finish_save(saved);
    assert_eq!(book.path(), Some(path.as_path()));
    assert!(!book.is_dirty());
}

#[test]
fn saving_to_a_new_file_refuses_one_that_exists_and_saving_over_replaces_it() {
    let (other, _) = edited_book();
    let before = std::fs::read(&other.path).unwrap();
    let mut book = Workbook::blank().unwrap();
    book.sheets[0].set_cell(a1("A1"), Cell::value(CellValue::Number(9.0)));

    assert!(book.begin_save_as(&other.path).wait().is_err());
    assert_eq!(std::fs::read(&other.path).unwrap(), before);

    let saved = book.begin_save_over(&other.path).wait().unwrap();
    book.finish_save(saved);
    assert_eq!(number_on_disk(&other.path, "A1"), CellValue::Number(9.0));
}

// The app with a save on its way to the disk.

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

/// An app with `5` typed over A1 of a workbook holding numbers, and the cursor on A2.
fn app_with_an_edit() -> (TempXlsx, App) {
    let file = write_temp(&single_sheet(&sheet(&numbers(3)), &[]));
    let mut app = App::new(Workbook::open(&file.path).unwrap());
    type_keys(&mut app, "s5");
    key(&mut app, KeyCode::Enter);
    (file, app)
}

#[test]
fn ctrl_s_returns_at_once_and_reports_when_the_save_has_finished() {
    let (file, mut app) = app_with_an_edit();

    ctrl(&mut app, 's');
    assert!(app.is_saving());
    assert!(app.is_busy());
    assert!(
        app.workbook.is_dirty(),
        "not saved until it's known to have worked"
    );
    assert_eq!(app.status(), "");

    app.finish_saving();
    assert!(!app.is_saving());
    assert_eq!(app.status(), "Saved book.xlsx");
    assert!(!app.workbook.is_dirty());
    assert_eq!(number_on_disk(&file.path, "A1"), CellValue::Number(5.0));
}

#[test]
fn a_save_that_fails_says_why_and_keeps_the_edits() {
    let (file, mut app) = app_with_an_edit();
    std::fs::write(&file.path, b"replaced by something else").unwrap();

    ctrl(&mut app, 's');
    app.finish_saving();

    assert!(!app.is_saving());
    assert!(app.status().starts_with("Save failed:"), "{}", app.status());
    assert!(app.status_is_error());
    assert!(app.workbook.is_dirty());
}

#[test]
fn taking_in_the_outcome_for_a_while_ends_the_save() {
    let (_file, mut app) = app_with_an_edit();

    ctrl(&mut app, 's');
    app.absorb_background(Duration::from_secs(30));

    assert!(!app.is_saving());
    assert_eq!(app.status(), "Saved book.xlsx");
}

#[test]
fn zz_quits_only_once_the_save_has_worked() {
    let (_file, mut app) = app_with_an_edit();

    type_keys(&mut app, "ZZ");
    assert!(!app.should_quit());
    assert!(app.is_saving());

    app.finish_saving();
    assert!(app.should_quit());
}

#[test]
fn saving_from_the_discard_dialog_takes_the_step_once_the_save_has_worked() {
    let (_file, mut app) = app_with_an_edit();

    key(&mut app, KeyCode::Char('q'));
    key(&mut app, KeyCode::Char('s'));
    assert_eq!(app.mode(), Mode::Normal);
    assert!(!app.should_quit());

    app.finish_saving();
    assert!(app.should_quit());
}

#[test]
fn a_save_that_fails_forgets_the_step_waiting_on_it() {
    let (file, mut app) = app_with_an_edit();
    std::fs::write(&file.path, b"replaced by something else").unwrap();

    type_keys(&mut app, "ZZ");
    app.finish_saving();
    assert!(!app.should_quit());
    assert_eq!(app.next_step(), None);

    std::fs::write(&file.path, single_sheet(&sheet(&numbers(3)), &[])).unwrap();
    key(&mut app, KeyCode::F(5));
    assert_eq!(app.mode(), Mode::ConfirmDiscard, "still has its edits");
}
