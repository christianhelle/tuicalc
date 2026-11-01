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
