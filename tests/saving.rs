//! Saving a workbook in the background, with the app still usable while the file is written.

mod common;

use std::time::Duration;

use common::*;
use tuicalc::address::CellRef;
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
