mod common;

use std::time::Duration;

use common::*;
use tuicalc::address::CellRef;
use tuicalc::document::{Document, LoadKind, OpenError, Outcome, Refusal, SaveError};
use tuicalc::model::{Cell, CellValue};
use tuicalc::xlsx::{Workbook, WorkbookError};

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

#[test]
fn nothing_changes_while_a_save_is_writing() {
    let (_file, mut document) = document(&two_sheets());
    document.edit(vec![(a1("A1"), number(5.0))]).unwrap();

    document.save().unwrap();

    assert!(document.is_saving());
    let refused = Err(Refusal::StillSaving);
    assert_eq!(document.edit(vec![(a1("A2"), number(6.0))]), refused);
    assert_eq!(document.undo(), Err(Refusal::StillSaving));
    assert_eq!(document.redo(), Err(Refusal::StillSaving));
    assert_eq!(Refusal::StillSaving.to_string(), "Still saving");
    assert_eq!(
        document.workbook().sheets[0].cell(a1("A2")),
        Cell::default()
    );
    assert!(matches!(
        document.save(),
        Err(SaveError::Refused(Refusal::StillSaving))
    ));
}

#[test]
fn a_finished_save_marks_the_document_saved_and_allows_changes_again() {
    let (file, mut document) = document(&two_sheets());
    document.edit(vec![(a1("A1"), number(5.0))]).unwrap();
    document.save().unwrap();

    assert!(matches!(document.finish_saving(), Some(Outcome::Saved)));

    assert!(!document.is_saving());
    assert!(!document.workbook().is_dirty());
    assert_eq!(document.edit(vec![(a1("A2"), number(6.0))]), Ok(()));
    let saved = Workbook::open(&file.path).unwrap();
    assert_eq!(saved.sheets[0].cell(a1("A1")).value, CellValue::Number(5.0));
}

#[test]
fn a_failed_save_keeps_the_edits_unsaved() {
    let (file, mut document) = document(&two_sheets());
    document.edit(vec![(a1("A1"), number(5.0))]).unwrap();
    std::fs::write(&file.path, b"someone else's file").unwrap();
    document.save().unwrap();

    let outcome = document.finish_saving();

    assert!(matches!(
        outcome,
        Some(Outcome::SaveFailed(WorkbookError::ChangedOnDisk))
    ));
    assert!(document.workbook().is_dirty());
    assert!(!document.is_saving());
}

#[test]
fn a_blank_document_has_no_file_to_save_to_until_it_is_given_one() {
    let mut document = Document::new(Workbook::blank().unwrap());
    document.edit(vec![(a1("A1"), number(5.0))]).unwrap();
    assert!(matches!(document.save(), Err(SaveError::NoFile)));

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("new.xlsx");
    document.save_as(&path).unwrap();
    assert!(matches!(document.finish_saving(), Some(Outcome::Saved)));

    assert_eq!(document.workbook().path(), Some(path.as_path()));
    assert!(!document.workbook().is_dirty());
}

#[test]
fn saving_over_a_file_replaces_it() {
    let (other, _) = document(&two_sheets());
    let mut document = Document::new(Workbook::blank().unwrap());
    document.edit(vec![(a1("A1"), number(5.0))]).unwrap();

    document.save_over(&other.path).unwrap();
    assert!(matches!(document.finish_saving(), Some(Outcome::Saved)));

    let saved = Workbook::open(&other.path).unwrap();
    assert_eq!(saved.sheets[0].cell(a1("A1")).value, CellValue::Number(5.0));
}

#[test]
fn news_of_a_save_arrives_through_absorbing_too() {
    let (_file, mut document) = document(&two_sheets());
    document.edit(vec![(a1("A1"), number(5.0))]).unwrap();
    document.save().unwrap();

    let outcome = document.absorb(Duration::from_secs(30));

    assert!(matches!(outcome, Some(Outcome::Saved)));
    assert!(document.absorb(Duration::ZERO).is_none(), "nothing more");
}

fn numbers(rows: u32) -> String {
    (1..=rows)
        .map(|row| format!(r#"<row r="{row}"><c r="A{row}"><v>{row}</v></c></row>"#))
        .collect()
}

fn broken_workbook() -> TempXlsx {
    let broken = format!(r#"<worksheet><sheetData>{}</row>"#, numbers(2));
    write_temp(&single_sheet(&broken, &[]))
}

#[test]
fn nothing_changes_and_nothing_saves_until_the_whole_workbook_has_arrived() {
    let (_file, mut document) = document(&two_sheets());
    let other = write_temp(&single_sheet(&sheet(&numbers(3)), &[]));

    document.open(&other.path).unwrap();

    assert!(document.is_loading());
    let refused = Err(Refusal::StillOpening);
    assert_eq!(document.edit(vec![(a1("Z9"), number(1.0))]), refused);
    assert_eq!(document.undo(), Err(Refusal::StillOpening));
    assert_eq!(
        Refusal::StillOpening.to_string(),
        "Still opening: Esc cancels"
    );
    assert!(matches!(
        document.save(),
        Err(SaveError::Refused(Refusal::StillOpening))
    ));
    assert!(matches!(
        document.finish_loading(),
        Some(Outcome::Loaded {
            kind: LoadKind::Open,
            replaced: true
        })
    ));
    assert!(!document.is_loading());
    assert_eq!(
        document.current_sheet().cell(a1("A3")).value,
        CellValue::Number(3.0)
    );
    assert_eq!(document.edit(vec![(a1("Z9"), number(1.0))]), Ok(()));
}

#[test]
fn a_document_opened_from_nothing_says_there_was_nothing_to_replace() {
    let file = write_temp(&single_sheet(&sheet(&numbers(3)), &[]));
    let (workbook, load) = Workbook::open_loading(&file.path, 0).unwrap();
    let mut document = Document::opening(workbook, load);

    assert!(matches!(
        document.finish_loading(),
        Some(Outcome::Loaded {
            kind: LoadKind::Open,
            replaced: false
        })
    ));
}

#[test]
fn a_new_workbook_starts_at_the_top_with_nothing_to_undo() {
    let (_file, mut document) = document(&two_sheets());
    document.edit(vec![(a1("A1"), number(5.0))]).unwrap();
    document.place.cursor = a1("C3");
    let other = write_temp(&single_sheet(&sheet(&numbers(3)), &[]));

    document.open(&other.path).unwrap();
    assert_eq!(document.place.cursor, a1("A1"));
    document.finish_loading();

    assert_eq!(document.undo(), Ok(false));
}

#[test]
fn a_workbook_that_fails_partway_gives_back_the_last_one_as_it_was() {
    let (first, mut document) = document(&two_sheets());
    document.switch_sheet(1);
    document.place.cursor = a1("C3");
    document.place.scroll = a1("B2");
    document.place.anchor = a1("A2");
    document.edit(vec![(a1("A1"), number(5.0))]).unwrap();
    let broken = broken_workbook();

    document.open(&broken.path).unwrap();
    let outcome = document.finish_loading();

    assert!(matches!(
        outcome,
        Some(Outcome::LoadFailed {
            kind: LoadKind::Open,
            went_back: true,
            ..
        })
    ));
    assert!(!document.is_loading());
    assert_eq!(document.workbook().path(), Some(first.path.as_path()));
    assert_eq!(document.sheet_index(), 1);
    assert_eq!(document.place.cursor, a1("C3"));
    assert_eq!(document.place.scroll, a1("B2"));
    assert_eq!(document.place.anchor, a1("A2"));
    assert_eq!(document.current_sheet().cell(a1("A1")), number(5.0));
    assert_eq!(document.undo(), Ok(true), "its history came back with it");
}

#[test]
fn a_first_workbook_that_fails_has_nothing_to_go_back_to() {
    let broken = broken_workbook();
    let (workbook, load) = Workbook::open_loading(&broken.path, 0).unwrap();
    let mut document = Document::opening(workbook, load);

    assert!(matches!(
        document.finish_loading(),
        Some(Outcome::LoadFailed {
            went_back: false,
            ..
        })
    ));
}

#[test]
fn stopping_a_load_goes_back_to_the_last_workbook_if_there_is_one() {
    let (first, mut document) = document(&two_sheets());
    document.place.cursor = a1("C3");
    let other = write_temp(&single_sheet(&sheet(&numbers(3)), &[]));
    document.open(&other.path).unwrap();

    assert!(document.cancel_load());

    assert!(!document.is_loading());
    assert_eq!(document.workbook().path(), Some(first.path.as_path()));
    assert_eq!(document.place.cursor, a1("C3"));

    let (workbook, load) = Workbook::open_loading(&other.path, 0).unwrap();
    let mut opening = Document::opening(workbook, load);
    assert!(!opening.cancel_load());
}

#[test]
fn reloading_stays_on_the_same_sheet_and_cell() {
    let (file, mut document) = document(&two_sheets());
    document.switch_sheet(1);
    document.place.cursor = a1("C3");
    document.place.scroll = a1("B2");
    document.edit(vec![(a1("A1"), number(5.0))]).unwrap();

    document.reload().unwrap();
    assert!(matches!(
        document.finish_loading(),
        Some(Outcome::Loaded {
            kind: LoadKind::Reload,
            replaced: true
        })
    ));

    assert_eq!(document.workbook().path(), Some(file.path.as_path()));
    assert_eq!(document.sheet_index(), 1);
    assert_eq!(document.place.cursor, a1("C3"));
    assert_eq!(document.place.scroll, a1("B2"));
    assert_eq!(
        document.current_sheet().cell(a1("A1")).value,
        CellValue::Number(2.0),
        "the edit is gone"
    );
}

#[test]
fn reloading_a_file_that_lost_the_sheet_starts_at_the_top() {
    let (file, mut document) = document(&two_sheets());
    document.switch_sheet(1);
    document.place.cursor = a1("C3");
    std::fs::write(&file.path, single_sheet(&sheet(&numbers(1)), &[])).unwrap();

    document.reload().unwrap();
    document.finish_loading();

    assert_eq!(document.sheet_index(), 0);
    assert_eq!(document.place.cursor, a1("A1"));
}

#[test]
fn a_file_that_is_no_workbook_is_refused_at_once_and_changes_nothing() {
    let (first, mut document) = document(&two_sheets());
    let broken = write_temp(b"not a workbook");

    assert!(document.open(&broken.path).is_err());

    assert!(!document.is_loading());
    assert_eq!(document.workbook().path(), Some(first.path.as_path()));
}

#[test]
fn no_other_workbook_loads_while_this_one_is_busy() {
    let (_file, mut document) = document(&two_sheets());
    let other = write_temp(&single_sheet(&sheet(&numbers(3)), &[]));
    document.save().unwrap();

    assert!(matches!(
        document.open(&other.path),
        Err(OpenError::Refused(Refusal::StillSaving))
    ));
    assert!(matches!(
        document.reload(),
        Err(OpenError::Refused(Refusal::StillSaving))
    ));
    document.finish_saving();

    document.open(&other.path).unwrap();
    assert!(matches!(
        document.open(&other.path),
        Err(OpenError::Refused(Refusal::StillOpening))
    ));
}
