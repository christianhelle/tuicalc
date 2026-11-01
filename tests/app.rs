mod common;

use common::*;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use tuicalc::address::CellRef;
use tuicalc::app::{App, Mode};
use tuicalc::model::{Cell, CellValue};
use tuicalc::xlsx::Workbook;

fn a1(text: &str) -> CellRef {
    CellRef::parse(text).unwrap()
}

fn app_with(rows: &str) -> (TempXlsx, App) {
    app_from(&single_sheet(&sheet(rows), &[]))
}

fn app_from(bytes: &[u8]) -> (TempXlsx, App) {
    let file = write_temp(bytes);
    let workbook = Workbook::open(&file.path).unwrap();
    (file, App::new(workbook))
}

fn press(app: &mut App, code: KeyCode) {
    app.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
}

fn ctrl(app: &mut App, code: KeyCode) {
    app.handle_key(KeyEvent::new(code, KeyModifiers::CONTROL));
}

fn type_text(app: &mut App, text: &str) {
    for c in text.chars() {
        press(app, KeyCode::Char(c));
    }
}

/// Types `text` into the cell under the cursor and confirms it.
fn enter_value(app: &mut App, text: &str) -> Cell {
    let at = app.cursor();
    type_text(app, text);
    press(app, KeyCode::Enter);
    app.current_sheet().cell(at)
}

#[test]
fn arrows_move_the_cursor_and_stop_at_the_sheet_edge() {
    let (_file, mut app) = app_with("");
    assert_eq!(app.cursor(), a1("A1"));

    press(&mut app, KeyCode::Left);
    press(&mut app, KeyCode::Up);
    assert_eq!(app.cursor(), a1("A1"));

    press(&mut app, KeyCode::Right);
    press(&mut app, KeyCode::Down);
    assert_eq!(app.cursor(), a1("B2"));
}

#[test]
fn key_releases_do_nothing() {
    let (_file, mut app) = app_with("");
    let release = KeyEvent::new_with_kind(KeyCode::Right, KeyModifiers::NONE, KeyEventKind::Release);

    app.handle_key(release);

    assert_eq!(app.cursor(), a1("A1"));
}

#[test]
fn tab_and_enter_move_the_cursor() {
    let (_file, mut app) = app_with("");
    press(&mut app, KeyCode::Tab);
    press(&mut app, KeyCode::Tab);
    press(&mut app, KeyCode::BackTab);
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.cursor(), a1("B2"));
}

#[test]
fn paging_and_jumping() {
    let (_file, mut app) = app_with(r#"<row r="5"><c r="D5"><v>1</v></c></row>"#);
    app.set_viewport(5, 10);

    press(&mut app, KeyCode::PageDown);
    assert_eq!(app.cursor(), a1("A11"));
    press(&mut app, KeyCode::PageUp);
    assert_eq!(app.cursor(), a1("A1"));

    ctrl(&mut app, KeyCode::End);
    assert_eq!(app.cursor(), a1("D5"));
    press(&mut app, KeyCode::Home);
    assert_eq!(app.cursor(), a1("A5"));
    press(&mut app, KeyCode::End);
    assert_eq!(app.cursor(), a1("D5"));
    ctrl(&mut app, KeyCode::Home);
    assert_eq!(app.cursor(), a1("A1"));
}

#[test]
fn typing_replaces_the_cell_and_enter_confirms_and_moves_down() {
    let (_file, mut app) = app_with(r#"<row r="1"><c r="A1"><v>1</v></c></row>"#);

    type_text(&mut app, "42");
    assert_eq!(app.mode(), Mode::Edit);
    assert_eq!(app.buffer(), "42");
    assert_eq!(app.current_sheet().cell(a1("A1")).value, CellValue::Number(1.0), "not until confirmed");

    press(&mut app, KeyCode::Enter);

    assert_eq!(app.mode(), Mode::Navigate);
    assert_eq!(app.current_sheet().cell(a1("A1")).value, CellValue::Number(42.0));
    assert_eq!(app.cursor(), a1("A2"));
}

#[test]
fn control_shortcuts_do_not_type_but_altgr_characters_do() {
    let (_file, mut app) = app_with("");
    app.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL));
    assert_eq!(app.mode(), Mode::Navigate);

    let altgr = KeyModifiers::CONTROL | KeyModifiers::ALT;
    app.handle_key(KeyEvent::new(KeyCode::Char('@'), altgr));
    assert_eq!(app.buffer(), "@");
}

#[test]
fn input_is_interpreted_by_its_form() {
    let (_file, mut app) = app_with("");
    let text = |s: &str| CellValue::Text(s.to_string());
    for (input, expected) in [
        ("-1.5e3", CellValue::Number(-1500.0)),
        ("hello", text("hello")),
        ("true", CellValue::Bool(true)),
        ("FALSE", CellValue::Bool(false)),
        ("#N/A", CellValue::Error("#N/A".into())),
        ("'123", text("123")),
        ("'=A1", text("=A1")),
        ("=", text("=")),
        ("inf", text("inf")),
        ("NaN", text("NaN")),
        ("infinity", text("infinity")),
    ] {
        assert_eq!(enter_value(&mut app, input).value, expected, "{input}");
    }

    assert_eq!(enter_value(&mut app, "=SUM(A1:A2)").formula.as_deref(), Some("SUM(A1:A2)"));
}

#[test]
fn f2_edits_the_existing_content_at_the_caret() {
    let (_file, mut app) = app_with(r#"<row r="1"><c r="A1" t="inlineStr"><is><t>héllo</t></is></c></row>"#);

    press(&mut app, KeyCode::F(2));
    assert_eq!(app.buffer(), "héllo");
    assert_eq!(app.caret(), 5);

    press(&mut app, KeyCode::Home);
    press(&mut app, KeyCode::Right);
    press(&mut app, KeyCode::Delete);
    type_text(&mut app, "é");
    press(&mut app, KeyCode::End);
    press(&mut app, KeyCode::Backspace);
    press(&mut app, KeyCode::Left);
    press(&mut app, KeyCode::Left);
    press(&mut app, KeyCode::Right);
    type_text(&mut app, "!");
    assert_eq!(app.buffer(), "hél!l");
    assert_eq!(app.caret(), 4);

    press(&mut app, KeyCode::Enter);
    assert_eq!(app.current_sheet().cell(a1("A1")).value, CellValue::Text("hél!l".into()));
}

#[test]
fn escape_cancels_and_confirming_unchanged_text_changes_nothing() {
    let (_file, mut app) = app_with(r#"<row r="1"><c r="A1"><f>B1*2</f><v>4</v></c></row>"#);

    type_text(&mut app, "9");
    press(&mut app, KeyCode::Esc);
    assert_eq!(app.mode(), Mode::Navigate);
    assert_eq!(app.current_sheet().cell(a1("A1")).value, CellValue::Number(4.0));

    press(&mut app, KeyCode::F(2));
    press(&mut app, KeyCode::Enter);
    let cell = app.current_sheet().cell(a1("A1"));
    assert_eq!(cell.value, CellValue::Number(4.0), "the cached result is kept");
    assert!(!app.workbook.is_dirty());
}

#[test]
fn date_cells_edit_as_iso_text_and_stay_dates() {
    let styles = r#"<styleSheet><cellXfs count="2"><xf numFmtId="0"/><xf numFmtId="14"/></cellXfs></styleSheet>"#;
    let rows = r#"<row r="1"><c r="A1" s="1"><v>45000</v></c></row>"#;
    let (_file, mut app) = app_from(&single_sheet(&sheet(rows), &[("xl/styles.xml", styles)]));

    press(&mut app, KeyCode::F(2));
    assert_eq!(app.buffer(), "2023-03-15");
    press(&mut app, KeyCode::Enter);
    assert!(!app.workbook.is_dirty());

    press(&mut app, KeyCode::Up);
    let cell = enter_value(&mut app, "2023-03-16");
    assert_eq!(cell.value, CellValue::Number(45001.0));
    assert!(cell.is_date);
}

#[test]
fn f2_shows_each_kind_of_cell_as_text_that_recreates_it() {
    let (_file, mut app) = app_with(
        r#"<row r="1"><c r="A1"><f>B1*2</f><v>4</v></c><c r="B1"><v>2.5</v></c><c r="C1" t="b"><v>1</v></c><c r="D1" t="inlineStr"><is><t>007</t></is></c></row>"#,
    );
    for expected in ["=B1*2", "2.5", "TRUE", "'007"] {
        press(&mut app, KeyCode::F(2));
        assert_eq!(app.buffer(), expected);
        press(&mut app, KeyCode::Esc);
        press(&mut app, KeyCode::Right);
    }
}

#[test]
fn arrows_and_tab_confirm_the_edit_and_move() {
    let (_file, mut app) = app_with("");
    press(&mut app, KeyCode::Down);

    type_text(&mut app, "1");
    press(&mut app, KeyCode::Tab);
    assert_eq!(app.cursor(), a1("B2"));
    type_text(&mut app, "2");
    press(&mut app, KeyCode::Up);
    assert_eq!(app.cursor(), a1("B1"));
    type_text(&mut app, "3");
    press(&mut app, KeyCode::Down);
    type_text(&mut app, "4");
    press(&mut app, KeyCode::BackTab);
    assert_eq!(app.cursor(), a1("A2"));

    let value = |at| app.current_sheet().cell(a1(at)).value;
    assert_eq!(value("A2"), CellValue::Number(1.0));
    assert_eq!(value("B1"), CellValue::Number(3.0));
    assert_eq!(value("B2"), CellValue::Number(4.0));
}

#[test]
fn delete_clears_the_cell() {
    let (_file, mut app) = app_with(r#"<row r="1"><c r="A1"><v>1</v></c><c r="B1"><v>2</v></c></row>"#);

    press(&mut app, KeyCode::Delete);
    press(&mut app, KeyCode::Right);
    press(&mut app, KeyCode::Backspace);

    assert_eq!(app.current_sheet().cell(a1("A1")), Cell::default());
    assert_eq!(app.current_sheet().cell(a1("B1")), Cell::default());
    assert_eq!(app.mode(), Mode::Navigate);

    press(&mut app, KeyCode::Down);
    press(&mut app, KeyCode::Delete);
    assert_eq!(app.current_sheet().dirty_cells().count(), 2, "clearing an empty cell is no edit");
}

#[test]
fn array_formula_regions_cannot_be_edited() {
    let (_file, mut app) = app_with(
        r#"<row r="1"><c r="A1"><f t="array" ref="A1:A2">B1:B2*2</f><v>2</v></c></row><row r="2"><c r="A2"><v>4</v></c></row>"#,
    );

    for attempt in [KeyCode::Char('9'), KeyCode::F(2), KeyCode::Delete] {
        press(&mut app, attempt);
        assert_eq!(app.mode(), Mode::Navigate);
        assert!(app.status().contains("array formula"), "{}", app.status());
    }
    assert!(!app.workbook.is_dirty());

    press(&mut app, KeyCode::Right);
    assert_eq!(app.status(), "", "messages clear on the next key");
}

#[test]
fn undo_and_redo_restore_cells_and_the_cursor() {
    let (_file, mut app) = app_with(r#"<row r="1"><c r="A1"><f>B1</f><v>4</v></c></row>"#);
    press(&mut app, KeyCode::Delete);
    press(&mut app, KeyCode::Down);
    enter_value(&mut app, "7");

    ctrl(&mut app, KeyCode::Char('z'));
    assert_eq!(app.current_sheet().cell(a1("A2")), Cell::default());
    assert_eq!(app.cursor(), a1("A2"));

    ctrl(&mut app, KeyCode::Char('z'));
    let restored = app.current_sheet().cell(a1("A1"));
    assert_eq!(restored.formula.as_deref(), Some("B1"));
    assert_eq!(restored.value, CellValue::Number(4.0));
    assert_eq!(app.cursor(), a1("A1"));

    ctrl(&mut app, KeyCode::Char('z'));
    assert_eq!(app.status(), "Nothing to undo");

    ctrl(&mut app, KeyCode::Char('y'));
    assert_eq!(app.current_sheet().cell(a1("A1")), Cell::default());
    ctrl(&mut app, KeyCode::Char('y'));
    assert_eq!(app.current_sheet().cell(a1("A2")).value, CellValue::Number(7.0));
    ctrl(&mut app, KeyCode::Char('y'));
    assert_eq!(app.status(), "Nothing to redo");
}

#[test]
fn a_new_edit_discards_the_redo_history() {
    let (_file, mut app) = app_with("");
    enter_value(&mut app, "1");
    ctrl(&mut app, KeyCode::Char('z'));
    press(&mut app, KeyCode::Down);
    enter_value(&mut app, "2");

    ctrl(&mut app, KeyCode::Char('y'));

    assert_eq!(app.status(), "Nothing to redo");
    assert_eq!(app.current_sheet().cell(a1("A1")), Cell::default());
}

#[test]
fn ctrl_s_saves_and_reports_success_or_failure() {
    let (file, mut app) = app_with("");
    enter_value(&mut app, "5");

    ctrl(&mut app, KeyCode::Char('s'));

    assert_eq!(app.status(), "Saved book.xlsx");
    assert!(!app.workbook.is_dirty());
    let reloaded = Workbook::open(&file.path).unwrap();
    assert_eq!(reloaded.sheets[0].cell(a1("A1")).value, CellValue::Number(5.0));

    press(&mut app, KeyCode::Down);
    enter_value(&mut app, "6");
    std::fs::write(&file.path, b"replaced by something else").unwrap();
    ctrl(&mut app, KeyCode::Char('s'));

    assert!(app.status().starts_with("Save failed:"), "{}", app.status());
    assert!(app.workbook.is_dirty());
}

#[test]
fn quitting_a_clean_workbook_is_immediate() {
    let (_file, mut app) = app_with("");
    ctrl(&mut app, KeyCode::Char('q'));
    assert!(app.should_quit());

    let (_file, mut app) = app_with("");
    ctrl(&mut app, KeyCode::Char('c'));
    assert!(app.should_quit());
}

#[test]
fn quitting_with_unsaved_edits_asks_first() {
    let (file, mut app) = app_with("");
    enter_value(&mut app, "5");

    ctrl(&mut app, KeyCode::Char('q'));
    assert!(!app.should_quit());
    assert_eq!(app.mode(), Mode::ConfirmQuit);

    press(&mut app, KeyCode::Char('x'));
    assert_eq!(app.mode(), Mode::ConfirmQuit, "other keys keep asking");
    assert_eq!(app.current_sheet().cell(a1("A2")), Cell::default());

    press(&mut app, KeyCode::Esc);
    assert_eq!(app.mode(), Mode::Navigate);
    assert!(!app.should_quit());

    ctrl(&mut app, KeyCode::Char('q'));
    press(&mut app, KeyCode::Char('d'));
    assert!(app.should_quit());
    let on_disk = Workbook::open(&file.path).unwrap();
    assert_eq!(on_disk.sheets[0].cell(a1("A1")), Cell::default(), "discarded");
}

#[test]
fn save_and_quit_stays_open_if_the_save_fails() {
    let (file, mut app) = app_with("");
    enter_value(&mut app, "5");
    ctrl(&mut app, KeyCode::Char('q'));
    std::fs::write(&file.path, b"changed").unwrap();

    press(&mut app, KeyCode::Char('s'));

    assert!(!app.should_quit());
    assert!(app.status().starts_with("Save failed:"), "{}", app.status());

    std::fs::remove_file(&file.path).unwrap();
}
