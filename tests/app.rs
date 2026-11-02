mod common;

use std::path::Path;

use common::*;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use tuicalc::address::CellRef;
use tuicalc::app::{App, Mode};
use tuicalc::file_dialog::DialogKind;
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
    press(app, KeyCode::Char('s'));
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
    let release =
        KeyEvent::new_with_kind(KeyCode::Right, KeyModifiers::NONE, KeyEventKind::Release);

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

    press(&mut app, KeyCode::Char('s'));
    type_text(&mut app, "42");
    assert_eq!(app.mode(), Mode::Insert);
    assert_eq!(app.buffer(), "42");
    assert_eq!(
        app.current_sheet().cell(a1("A1")).value,
        CellValue::Number(1.0),
        "not until confirmed"
    );

    press(&mut app, KeyCode::Enter);

    assert_eq!(app.mode(), Mode::Normal);
    assert_eq!(
        app.current_sheet().cell(a1("A1")).value,
        CellValue::Number(42.0)
    );
    assert_eq!(app.cursor(), a1("A2"));
}

#[test]
fn control_shortcuts_do_not_type_but_altgr_characters_do() {
    let (_file, mut app) = app_with("");
    app.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL));
    assert_eq!(app.mode(), Mode::Normal);

    press(&mut app, KeyCode::Char('s'));
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

    assert_eq!(
        enter_value(&mut app, "=SUM(A1:A2)").formula.as_deref(),
        Some("SUM(A1:A2)")
    );
}

#[test]
fn f2_edits_the_existing_content_at_the_caret() {
    let (_file, mut app) =
        app_with(r#"<row r="1"><c r="A1" t="inlineStr"><is><t>héllo</t></is></c></row>"#);

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
    assert_eq!(
        app.current_sheet().cell(a1("A1")).value,
        CellValue::Text("hél!l".into())
    );
}

#[test]
fn ctrl_c_cancels_and_confirming_unchanged_text_changes_nothing() {
    let (_file, mut app) = app_with(r#"<row r="1"><c r="A1"><f>B1*2</f><v>4</v></c></row>"#);

    press(&mut app, KeyCode::Char('s'));
    type_text(&mut app, "9");
    ctrl(&mut app, KeyCode::Char('c'));
    assert_eq!(app.mode(), Mode::Normal);
    assert_eq!(
        app.current_sheet().cell(a1("A1")).value,
        CellValue::Number(4.0)
    );

    press(&mut app, KeyCode::F(2));
    press(&mut app, KeyCode::Enter);
    let cell = app.current_sheet().cell(a1("A1"));
    assert_eq!(
        cell.value,
        CellValue::Number(4.0),
        "the cached result is kept"
    );
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

    press(&mut app, KeyCode::Char('s'));
    type_text(&mut app, "1");
    press(&mut app, KeyCode::Tab);
    assert_eq!(app.cursor(), a1("B2"));
    press(&mut app, KeyCode::Char('s'));
    type_text(&mut app, "2");
    press(&mut app, KeyCode::Up);
    assert_eq!(app.cursor(), a1("B1"));
    press(&mut app, KeyCode::Char('s'));
    type_text(&mut app, "3");
    press(&mut app, KeyCode::Down);
    press(&mut app, KeyCode::Char('s'));
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
    let (_file, mut app) =
        app_with(r#"<row r="1"><c r="A1"><v>1</v></c><c r="B1"><v>2</v></c></row>"#);

    press(&mut app, KeyCode::Delete);
    press(&mut app, KeyCode::Right);
    press(&mut app, KeyCode::Backspace);

    assert_eq!(app.current_sheet().cell(a1("A1")), Cell::default());
    assert_eq!(app.current_sheet().cell(a1("B1")), Cell::default());
    assert_eq!(app.mode(), Mode::Normal);

    press(&mut app, KeyCode::Down);
    press(&mut app, KeyCode::Delete);
    assert_eq!(
        app.current_sheet().dirty_cells().count(),
        2,
        "clearing an empty cell is no edit"
    );
}

#[test]
fn array_formula_regions_cannot_be_edited() {
    let (_file, mut app) = app_with(
        r#"<row r="1"><c r="A1"><f t="array" ref="A1:A2">B1:B2*2</f><v>2</v></c></row><row r="2"><c r="A2"><v>4</v></c></row>"#,
    );

    for attempt in [KeyCode::Char('s'), KeyCode::F(2), KeyCode::Delete] {
        press(&mut app, attempt);
        assert_eq!(app.mode(), Mode::Normal);
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
    assert_eq!(
        app.current_sheet().cell(a1("A2")).value,
        CellValue::Number(7.0)
    );
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
    assert_eq!(
        reloaded.sheets[0].cell(a1("A1")).value,
        CellValue::Number(5.0)
    );

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
fn q_quits_a_clean_workbook_and_asks_about_unsaved_edits() {
    let (_file, mut app) = app_with("");
    press(&mut app, KeyCode::Char('q'));
    assert!(app.should_quit());

    let (_file, mut app) = app_with("");
    enter_value(&mut app, "5");
    press(&mut app, KeyCode::Char('q'));
    assert!(!app.should_quit());
    assert_eq!(app.mode(), Mode::ConfirmDiscard);
}

#[test]
fn q_is_only_a_quit_key_in_normal_mode() {
    let (_file, mut app) = app_with("");
    press(&mut app, KeyCode::Char('s'));
    press(&mut app, KeyCode::Char('q'));
    assert!(!app.should_quit());
    assert_eq!(app.buffer(), "q");
    press(&mut app, KeyCode::Esc);

    press(&mut app, KeyCode::Char('v'));
    press(&mut app, KeyCode::Char('q'));
    assert!(!app.should_quit());

    press(&mut app, KeyCode::Esc);
    press(&mut app, KeyCode::Char('d'));
    press(&mut app, KeyCode::Char('q'));
    assert!(!app.should_quit(), "cancels the waiting operator");
}

#[test]
fn quitting_with_unsaved_edits_asks_first() {
    let (file, mut app) = app_with("");
    enter_value(&mut app, "5");

    ctrl(&mut app, KeyCode::Char('q'));
    assert!(!app.should_quit());
    assert_eq!(app.mode(), Mode::ConfirmDiscard);

    press(&mut app, KeyCode::Char('x'));
    assert_eq!(app.mode(), Mode::Normal, "other keys cancel, as in tuilook");
    assert!(!app.should_quit());
    assert_eq!(
        value(&app, "A1"),
        CellValue::Number(5.0),
        "and do nothing else"
    );

    ctrl(&mut app, KeyCode::Char('q'));
    ctrl(&mut app, KeyCode::Char('c'));
    assert_eq!(app.mode(), Mode::Normal);
    assert!(!app.should_quit());

    ctrl(&mut app, KeyCode::Char('q'));
    press(&mut app, KeyCode::Esc);
    assert_eq!(app.mode(), Mode::Normal);
    assert!(!app.should_quit());

    ctrl(&mut app, KeyCode::Char('q'));
    press(&mut app, KeyCode::Char('d'));
    assert!(app.should_quit());
    let on_disk = Workbook::open(&file.path).unwrap();
    assert_eq!(
        on_disk.sheets[0].cell(a1("A1")),
        Cell::default(),
        "discarded"
    );
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

fn two_sheets() -> Vec<u8> {
    let workbook = WORKBOOK.replace(
        "</sheets>",
        r#"<sheet name="Second" sheetId="2" r:id="rId4"/></sheets>"#,
    );
    let rels = WORKBOOK_RELS.replace(
        "</Relationships>",
        r#"<Relationship Id="rId4" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet2.xml"/></Relationships>"#,
    );
    package(&[
        ("[Content_Types].xml", CONTENT_TYPES),
        ("_rels/.rels", ROOT_RELS),
        ("xl/workbook.xml", &workbook),
        ("xl/_rels/workbook.xml.rels", &rels),
        ("xl/worksheets/sheet1.xml", &sheet("")),
        ("xl/worksheets/sheet2.xml", &sheet("")),
    ])
}

#[test]
fn sheets_switch_in_a_cycle_and_remember_their_cursors() {
    let (_file, mut app) = app_from(&two_sheets());
    press(&mut app, KeyCode::Down);

    ctrl(&mut app, KeyCode::PageDown);
    assert_eq!(app.current_sheet().name, "Second");
    assert_eq!(app.cursor(), a1("A1"));
    press(&mut app, KeyCode::Right);

    press(&mut app, KeyCode::F(6));
    assert_eq!(app.current_sheet().name, "Sheet1");
    assert_eq!(app.cursor(), a1("A2"));

    ctrl(&mut app, KeyCode::PageUp);
    assert_eq!(app.current_sheet().name, "Second");
    assert_eq!(app.cursor(), a1("B1"));

    app.handle_key(KeyEvent::new(KeyCode::F(6), KeyModifiers::SHIFT));
    assert_eq!(app.current_sheet().name, "Sheet1");
}

#[test]
fn undo_returns_to_the_sheet_of_the_edit_and_saves_only_edited_sheets() {
    let (file, mut app) = app_from(&two_sheets());
    enter_value(&mut app, "1");
    ctrl(&mut app, KeyCode::PageDown);
    enter_value(&mut app, "2");
    ctrl(&mut app, KeyCode::Char('s'));
    let reloaded = Workbook::open(&file.path).unwrap();
    assert_eq!(
        reloaded.sheets[0].cell(a1("A1")).value,
        CellValue::Number(1.0)
    );
    assert_eq!(
        reloaded.sheets[1].cell(a1("A1")).value,
        CellValue::Number(2.0)
    );

    ctrl(&mut app, KeyCode::Char('z'));
    ctrl(&mut app, KeyCode::Char('z'));
    assert_eq!(app.current_sheet().name, "Sheet1");
    assert_eq!(app.current_sheet().cell(a1("A1")), Cell::default());
}

#[test]
fn the_view_scrolls_just_enough_to_show_the_cursor() {
    let (_file, mut app) = app_with("");
    app.set_viewport(3, 4);

    for _ in 0..4 {
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Right);
    }
    assert_eq!(app.cursor(), a1("E5"));
    assert_eq!(app.scroll(), a1("C2"));

    press(&mut app, KeyCode::Up);
    assert_eq!(app.scroll(), a1("C2"), "still visible, so no scroll");

    ctrl(&mut app, KeyCode::Home);
    assert_eq!(app.scroll(), a1("A1"));
}

#[test]
fn shrinking_the_view_keeps_the_cursor_visible() {
    let (_file, mut app) = app_with("");
    app.set_viewport(10, 10);
    for _ in 0..8 {
        press(&mut app, KeyCode::Down);
    }
    assert_eq!(app.scroll(), a1("A1"));

    app.set_viewport(10, 3);

    assert_eq!(app.scroll(), a1("A7"));
}

/// Presses each character as its own key, as in vim's notation.
fn keys(app: &mut App, sequence: &str) {
    for c in sequence.chars() {
        press(app, KeyCode::Char(c));
    }
}

/// A1 B1 . D1 E1 . G1 across, and A1 A2 A3 . A5 A6 down.
const RUNS: &str = r#"<row r="1"><c r="A1"><v>1</v></c><c r="B1"><v>2</v></c><c r="D1"><v>4</v></c><c r="E1"><v>5</v></c><c r="G1"><v>7</v></c></row><row r="2"><c r="A2"><v>12</v></c></row><row r="3"><c r="A3"><v>13</v></c></row><row r="5"><c r="A5"><v>15</v></c></row><row r="6"><c r="A6"><v>16</v></c></row>"#;

fn value(app: &App, at: &str) -> CellValue {
    app.current_sheet().cell(a1(at)).value
}

#[test]
fn i_and_a_edit_the_cell_with_the_caret_at_either_end() {
    let (_file, mut app) =
        app_with(r#"<row r="1"><c r="A1" t="inlineStr"><is><t>word</t></is></c></row>"#);

    keys(&mut app, "i");
    assert_eq!(app.mode(), Mode::Insert);
    assert_eq!(app.buffer(), "word");
    assert_eq!(app.caret(), 0);
    press(&mut app, KeyCode::Esc);

    keys(&mut app, "a");
    assert_eq!(app.mode(), Mode::Insert);
    assert_eq!(app.caret(), 4);
}

#[test]
fn s_replaces_the_cell_and_equals_starts_a_formula() {
    let (_file, mut app) = app_with(r#"<row r="1"><c r="A1"><v>1</v></c></row>"#);

    keys(&mut app, "s");
    assert_eq!(app.buffer(), "");
    press(&mut app, KeyCode::Enter);
    assert_eq!(
        value(&app, "A1"),
        CellValue::Empty,
        "confirming nothing clears the cell"
    );

    keys(&mut app, "=");
    assert_eq!(app.buffer(), "=");
    assert_eq!(app.caret(), 1);
}

#[test]
fn letters_with_no_command_do_nothing_in_normal_mode() {
    let (_file, mut app) = app_with("");
    keys(&mut app, "Q");
    assert_eq!(app.mode(), Mode::Normal);
    assert_eq!(app.current_sheet().cell(a1("A1")), Cell::default());
}

#[test]
fn esc_keeps_the_text_and_stays_on_the_cell() {
    let (_file, mut app) = app_with("");
    keys(&mut app, "s42");
    press(&mut app, KeyCode::Esc);
    assert_eq!(app.mode(), Mode::Normal);
    assert_eq!(value(&app, "A1"), CellValue::Number(42.0));
    assert_eq!(app.cursor(), a1("A1"));
}

#[test]
fn ctrl_u_and_ctrl_w_delete_backwards_in_insert_mode() {
    let (_file, mut app) = app_with("");
    keys(&mut app, "sone two  ");
    ctrl(&mut app, KeyCode::Char('w'));
    assert_eq!(app.buffer(), "one ");
    keys(&mut app, "x");
    press(&mut app, KeyCode::Left);
    ctrl(&mut app, KeyCode::Char('u'));
    assert_eq!(app.buffer(), "x");
    assert_eq!(app.caret(), 0);
}

#[test]
fn hjkl_move_like_the_arrow_keys() {
    let (_file, mut app) = app_with("");
    keys(&mut app, "ljjlkh");
    assert_eq!(app.cursor(), a1("B2"));
    keys(&mut app, "hhhkkk");
    assert_eq!(app.cursor(), a1("A1"));
}

#[test]
fn a_count_repeats_the_motion_once() {
    let (_file, mut app) = app_with("");
    keys(&mut app, "5j");
    assert_eq!(app.cursor(), a1("A6"));
    keys(&mut app, "12l");
    assert_eq!(app.cursor(), a1("M6"));
    keys(&mut app, "h");
    assert_eq!(app.cursor(), a1("L6"), "the count does not linger");
    press(&mut app, KeyCode::Char('3'));
    press(&mut app, KeyCode::Down);
    assert_eq!(app.cursor(), a1("L9"), "arrows take counts too");
}

#[test]
fn enormous_counts_stop_at_the_sheet_edge() {
    let (_file, mut app) = app_with("");
    keys(&mut app, "99999999999999999999j");
    assert_eq!(app.cursor(), a1("A1048576"));
}

#[test]
fn line_motions_find_the_row_ends() {
    let (_file, mut app) = app_with(RUNS);
    keys(&mut app, "$");
    assert_eq!(app.cursor(), a1("G1"));
    keys(&mut app, "0");
    assert_eq!(app.cursor(), a1("A1"));
    keys(&mut app, "3l^");
    assert_eq!(app.cursor(), a1("A1"));
    keys(&mut app, "4|");
    assert_eq!(app.cursor(), a1("D1"));
    keys(&mut app, "10j$");
    assert_eq!(
        app.cursor(),
        a1("D11"),
        "an empty row has no last cell to go to"
    );
    keys(&mut app, "^");
    assert_eq!(app.cursor(), a1("A11"));
}

#[test]
fn gg_and_g_go_to_rows_and_keep_the_column() {
    let (_file, mut app) = app_with(RUNS);
    keys(&mut app, "lG");
    assert_eq!(app.cursor(), a1("B6"));
    keys(&mut app, "gg");
    assert_eq!(app.cursor(), a1("B1"));
    keys(&mut app, "12G");
    assert_eq!(app.cursor(), a1("B12"));
    keys(&mut app, "3gg");
    assert_eq!(app.cursor(), a1("B3"));
    keys(&mut app, "gq");
    assert_eq!(app.cursor(), a1("B3"), "an unknown second key does nothing");
    keys(&mut app, "j");
    assert_eq!(app.cursor(), a1("B4"), "and the prefix is gone");
}

#[test]
fn word_motions_treat_runs_of_filled_cells_as_words() {
    let (_file, mut app) = app_with(RUNS);
    keys(&mut app, "w");
    assert_eq!(app.cursor(), a1("D1"));
    keys(&mut app, "w");
    assert_eq!(app.cursor(), a1("G1"));
    keys(&mut app, "w");
    assert_eq!(app.cursor(), a1("G1"), "nothing further along");
    keys(&mut app, "b");
    assert_eq!(app.cursor(), a1("D1"));
    keys(&mut app, "b");
    assert_eq!(app.cursor(), a1("A1"));
    keys(&mut app, "b");
    assert_eq!(app.cursor(), a1("A1"));
    keys(&mut app, "e");
    assert_eq!(app.cursor(), a1("B1"));
    keys(&mut app, "e");
    assert_eq!(app.cursor(), a1("E1"));
    keys(&mut app, "0");
    keys(&mut app, "2w");
    assert_eq!(app.cursor(), a1("G1"));
}

#[test]
fn paragraph_motions_stop_at_empty_cells_in_the_column() {
    let (_file, mut app) = app_with(RUNS);
    keys(&mut app, "}");
    assert_eq!(app.cursor(), a1("A4"));
    keys(&mut app, "}");
    assert_eq!(app.cursor(), a1("A7"));
    keys(&mut app, "}");
    assert_eq!(app.cursor(), a1("A7"), "nothing further down");
    keys(&mut app, "{");
    assert_eq!(app.cursor(), a1("A4"));
    keys(&mut app, "{");
    assert_eq!(app.cursor(), a1("A1"));
}

#[test]
fn screen_motions_use_the_visible_rows() {
    let (_file, mut app) = app_with("");
    app.set_viewport(5, 10);
    keys(&mut app, "L");
    assert_eq!(app.cursor(), a1("A10"));
    keys(&mut app, "M");
    assert_eq!(app.cursor(), a1("A5"));
    keys(&mut app, "H");
    assert_eq!(app.cursor(), a1("A1"));
    keys(&mut app, "3H");
    assert_eq!(app.cursor(), a1("A3"));
    keys(&mut app, "2L");
    assert_eq!(app.cursor(), a1("A9"));
}

#[test]
fn ctrl_keys_scroll_the_view_along_with_the_cursor() {
    let (_file, mut app) = app_with("");
    app.set_viewport(5, 10);
    ctrl(&mut app, KeyCode::Char('d'));
    assert_eq!((app.cursor(), app.scroll()), (a1("A6"), a1("A6")));
    ctrl(&mut app, KeyCode::Char('u'));
    assert_eq!((app.cursor(), app.scroll()), (a1("A1"), a1("A1")));
    ctrl(&mut app, KeyCode::Char('f'));
    assert_eq!((app.cursor(), app.scroll()), (a1("A11"), a1("A11")));
    ctrl(&mut app, KeyCode::Char('b'));
    assert_eq!((app.cursor(), app.scroll()), (a1("A1"), a1("A1")));
    ctrl(&mut app, KeyCode::Char('b'));
    assert_eq!(
        (app.cursor(), app.scroll()),
        (a1("A1"), a1("A1")),
        "stops at the top"
    );
}

#[test]
fn z_commands_place_the_cursor_row_on_screen() {
    let (_file, mut app) = app_with("");
    app.set_viewport(5, 10);
    keys(&mut app, "30G");
    keys(&mut app, "zt");
    assert_eq!(app.scroll(), a1("A30"));
    keys(&mut app, "zb");
    assert_eq!(app.scroll(), a1("A21"));
    keys(&mut app, "zz");
    assert_eq!(app.scroll(), a1("A25"));
    assert_eq!(app.cursor(), a1("A30"));
}

#[test]
fn gt_and_g_shift_t_switch_sheets() {
    let (_file, mut app) = app_from(&two_sheets());
    keys(&mut app, "gt");
    assert_eq!(app.current_sheet().name, "Second");
    keys(&mut app, "gt");
    assert_eq!(app.current_sheet().name, "Sheet1");
    keys(&mut app, "gT");
    assert_eq!(app.current_sheet().name, "Second");
    keys(&mut app, "1gt");
    assert_eq!(app.current_sheet().name, "Sheet1");
    keys(&mut app, "9gt");
    assert_eq!(app.current_sheet().name, "Sheet1", "no such sheet");
}

#[test]
fn x_clears_cells_and_undo_restores_them_all_at_once() {
    let (_file, mut app) = app_with(RUNS);
    keys(&mut app, "5x");
    assert_eq!(value(&app, "A1"), CellValue::Empty);
    assert_eq!(value(&app, "D1"), CellValue::Empty);
    assert_eq!(value(&app, "E1"), CellValue::Empty);
    assert_eq!(value(&app, "G1"), CellValue::Number(7.0));
    assert_eq!(app.cursor(), a1("A1"));

    keys(&mut app, "u");
    assert_eq!(value(&app, "A1"), CellValue::Number(1.0));
    assert_eq!(value(&app, "E1"), CellValue::Number(5.0));

    ctrl(&mut app, KeyCode::Char('r'));
    assert_eq!(value(&app, "B1"), CellValue::Empty);
}

#[test]
fn capital_x_clears_to_the_left_and_d_to_the_end_of_the_row() {
    let (_file, mut app) = app_with(RUNS);
    keys(&mut app, "4|2X");
    assert_eq!(value(&app, "B1"), CellValue::Empty);
    assert_eq!(value(&app, "D1"), CellValue::Number(4.0));
    assert_eq!(app.cursor(), a1("B1"));

    keys(&mut app, "lllD");
    assert_eq!(value(&app, "E1"), CellValue::Empty);
    assert_eq!(value(&app, "G1"), CellValue::Empty);
    assert_eq!(value(&app, "D1"), CellValue::Number(4.0));
}

#[test]
fn a_clear_touching_a_read_only_cell_changes_nothing() {
    let (_file, mut app) = app_with(
        r#"<row r="1"><c r="A1"><v>1</v></c><c r="B1"><f t="array" ref="B1">C1*2</f><v>2</v></c></row>"#,
    );
    keys(&mut app, "2x");
    assert_eq!(value(&app, "A1"), CellValue::Number(1.0));
    assert!(
        app.status().contains("B1 is part of an array formula"),
        "{}",
        app.status()
    );
}

#[test]
fn visual_mode_selects_the_rectangle_between_start_and_cursor() {
    let (_file, mut app) = app_with(RUNS);
    assert_eq!(app.selection(), None);
    keys(&mut app, "lljv");
    assert_eq!(app.mode(), Mode::Visual);
    assert_eq!(app.selection(), Some((a1("C2"), a1("C2"))));

    keys(&mut app, "2jh");
    assert_eq!(app.selection(), Some((a1("B2"), a1("C4"))));
    keys(&mut app, "3k");
    assert_eq!(app.selection(), Some((a1("B1"), a1("C2"))));

    keys(&mut app, "o");
    assert_eq!(app.cursor(), a1("C2"));
    assert_eq!(
        app.selection(),
        Some((a1("B1"), a1("C2"))),
        "same corners, other end"
    );

    press(&mut app, KeyCode::Esc);
    assert_eq!(app.mode(), Mode::Normal);
    assert_eq!(app.selection(), None);
}

#[test]
fn visual_row_mode_selects_whole_rows_and_the_keys_switch_between_modes() {
    let (_file, mut app) = app_with(RUNS);
    keys(&mut app, "lVj");
    assert_eq!(app.mode(), Mode::VisualRow);
    assert_eq!(app.selection(), Some((a1("A1"), a1("XFD2"))));

    keys(&mut app, "v");
    assert_eq!(app.mode(), Mode::Visual);
    assert_eq!(app.selection(), Some((a1("B1"), a1("B2"))));

    keys(&mut app, "V");
    assert_eq!(app.mode(), Mode::VisualRow);
    keys(&mut app, "V");
    assert_eq!(app.mode(), Mode::Normal);

    keys(&mut app, "vv");
    assert_eq!(app.mode(), Mode::Normal);

    ctrl(&mut app, KeyCode::Char('v'));
    assert_eq!(app.mode(), Mode::Visual);
}

#[test]
fn every_motion_extends_a_visual_selection() {
    let (_file, mut app) = app_with(RUNS);
    keys(&mut app, "v$");
    assert_eq!(app.selection(), Some((a1("A1"), a1("G1"))));
    keys(&mut app, "G");
    assert_eq!(app.selection(), Some((a1("A1"), a1("G6"))));
    keys(&mut app, "gg");
    assert_eq!(app.selection(), Some((a1("A1"), a1("G1"))));
    keys(&mut app, "2h");
    assert_eq!(app.selection(), Some((a1("A1"), a1("E1"))));
}

#[test]
fn deleting_a_selection_clears_it_as_one_undo() {
    let (_file, mut app) = app_with(RUNS);
    keys(&mut app, "vjld");
    assert_eq!(app.mode(), Mode::Normal);
    assert_eq!(value(&app, "A1"), CellValue::Empty);
    assert_eq!(value(&app, "B1"), CellValue::Empty);
    assert_eq!(value(&app, "A2"), CellValue::Empty);
    assert_eq!(value(&app, "A3"), CellValue::Number(13.0));
    assert_eq!(app.cursor(), a1("A1"));

    keys(&mut app, "u");
    assert_eq!(value(&app, "A1"), CellValue::Number(1.0));
    assert_eq!(value(&app, "A2"), CellValue::Number(12.0));
}

#[test]
fn yanked_blocks_paste_at_the_cursor_and_replace_what_they_land_on() {
    let (_file, mut app) = app_with(RUNS);
    keys(&mut app, "vjly");
    assert_eq!(app.status(), "3 cells yanked");
    assert_eq!(app.mode(), Mode::Normal);
    assert_eq!(
        value(&app, "A1"),
        CellValue::Number(1.0),
        "yanking changes nothing"
    );

    keys(&mut app, "wp");
    assert_eq!(app.cursor(), a1("D1"));
    assert_eq!(value(&app, "D1"), CellValue::Number(1.0));
    assert_eq!(value(&app, "E1"), CellValue::Number(2.0));
    assert_eq!(value(&app, "D2"), CellValue::Number(12.0));
    assert_eq!(value(&app, "E2"), CellValue::Empty);

    keys(&mut app, "u");
    assert_eq!(value(&app, "D1"), CellValue::Number(4.0));
    assert_eq!(value(&app, "D2"), CellValue::Empty);
}

#[test]
fn an_empty_cell_in_a_block_overwrites_what_it_lands_on() {
    let (_file, mut app) = app_with(RUNS);
    keys(&mut app, "lvly");
    keys(&mut app, "lllp");
    assert_eq!(app.cursor(), a1("E1"));
    assert_eq!(value(&app, "E1"), CellValue::Number(2.0));
    assert_eq!(value(&app, "F1"), CellValue::Empty);
    assert_eq!(value(&app, "G1"), CellValue::Number(7.0));

    keys(&mut app, "lp");
    assert_eq!(value(&app, "F1"), CellValue::Number(2.0));
    assert_eq!(
        value(&app, "G1"),
        CellValue::Empty,
        "the empty C1 lands on it"
    );
}

#[test]
fn pasted_formulas_adjust_their_relative_references() {
    let (_file, mut app) =
        app_with(r#"<row r="1"><c r="A1"><v>1</v></c><c r="B1"><f>A1+$A$1</f><v>2</v></c></row>"#);
    keys(&mut app, "lvy");
    keys(&mut app, "jlp");
    let pasted = app.current_sheet().cell(a1("C2"));
    assert_eq!(pasted.formula.as_deref(), Some("B2+$A$1"));
    assert_eq!(
        pasted.value,
        CellValue::Empty,
        "the old result no longer applies"
    );
}

#[test]
fn deleted_cells_can_be_pasted_and_pasting_with_nothing_yanked_says_so() {
    let (_file, mut app) = app_with(RUNS);
    keys(&mut app, "p");
    assert_eq!(app.status(), "Nothing to paste");

    keys(&mut app, "vd");
    keys(&mut app, "5jp");
    assert_eq!(value(&app, "A6"), CellValue::Number(1.0));
}

#[test]
fn whole_rows_paste_below_with_p_and_at_the_cursor_with_capital_p() {
    let (_file, mut app) = app_with(RUNS);
    keys(&mut app, "lVy");
    keys(&mut app, "9jp");
    assert_eq!(value(&app, "A11"), CellValue::Number(1.0));
    assert_eq!(value(&app, "G11"), CellValue::Number(7.0));
    assert_eq!(app.cursor(), a1("B11"), "the column stays");

    keys(&mut app, "P");
    assert_eq!(value(&app, "D11"), CellValue::Number(4.0));
}

#[test]
fn changing_a_selection_clears_it_and_starts_typing_in_its_first_cell() {
    let (_file, mut app) = app_with(RUNS);
    keys(&mut app, "lvjc");
    assert_eq!(app.mode(), Mode::Insert);
    assert_eq!(app.cursor(), a1("B1"));
    assert_eq!(value(&app, "B1"), CellValue::Empty);
    keys(&mut app, "9");
    press(&mut app, KeyCode::Esc);
    assert_eq!(value(&app, "B1"), CellValue::Number(9.0));
}

#[test]
fn a_read_only_cell_in_a_selection_stops_the_delete() {
    let (_file, mut app) = app_with(
        r#"<row r="1"><c r="A1"><v>1</v></c><c r="B1"><f t="array" ref="B1">C1*2</f><v>2</v></c></row>"#,
    );
    keys(&mut app, "vld");
    assert_eq!(value(&app, "A1"), CellValue::Number(1.0));
    assert!(app.status().contains("read-only"), "{}", app.status());
}

#[test]
fn doubled_operators_act_on_whole_rows() {
    let (_file, mut app) = app_with(RUNS);
    keys(&mut app, "ldd");
    assert_eq!(value(&app, "A1"), CellValue::Empty);
    assert_eq!(value(&app, "G1"), CellValue::Empty);
    assert_eq!(value(&app, "A2"), CellValue::Number(12.0));
    assert_eq!(app.cursor(), a1("B1"), "the cursor stays");

    keys(&mut app, "u");
    keys(&mut app, "j2dd");
    assert_eq!(value(&app, "A2"), CellValue::Empty);
    assert_eq!(value(&app, "A3"), CellValue::Empty);
    assert_eq!(value(&app, "A1"), CellValue::Number(1.0));

    keys(&mut app, "u");
    keys(&mut app, "ggyy");
    assert_eq!(app.status(), "5 cells yanked");
}

#[test]
fn operators_take_the_cells_a_motion_covers() {
    let (_file, mut app) = app_with(RUNS);
    keys(&mut app, "d$");
    assert_eq!(value(&app, "A1"), CellValue::Empty);
    assert_eq!(value(&app, "G1"), CellValue::Empty);
    assert_eq!(value(&app, "A2"), CellValue::Number(12.0));

    keys(&mut app, "u");
    keys(&mut app, "dj");
    assert_eq!(value(&app, "A1"), CellValue::Empty);
    assert_eq!(value(&app, "A2"), CellValue::Empty);
    assert_eq!(value(&app, "B1"), CellValue::Number(2.0));

    keys(&mut app, "u");
    keys(&mut app, "dG");
    assert_eq!(value(&app, "A6"), CellValue::Empty);
    assert_eq!(value(&app, "B1"), CellValue::Number(2.0));

    keys(&mut app, "u");
    keys(&mut app, "jjdgg");
    assert_eq!(value(&app, "A1"), CellValue::Empty);
    assert_eq!(value(&app, "A3"), CellValue::Empty);
    assert_eq!(app.cursor(), a1("A1"));
}

#[test]
fn sideways_operators_leave_the_cell_the_motion_lands_on() {
    let (_file, mut app) = app_with(RUNS);
    keys(&mut app, "dw");
    assert_eq!(value(&app, "A1"), CellValue::Empty);
    assert_eq!(value(&app, "B1"), CellValue::Empty);
    assert_eq!(value(&app, "D1"), CellValue::Number(4.0), "where w landed");

    keys(&mut app, "u");
    keys(&mut app, "dl");
    assert_eq!(value(&app, "A1"), CellValue::Empty);
    assert_eq!(value(&app, "B1"), CellValue::Number(2.0));

    keys(&mut app, "u");
    keys(&mut app, "ldh");
    assert_eq!(value(&app, "A1"), CellValue::Empty);
    assert_eq!(
        value(&app, "B1"),
        CellValue::Number(2.0),
        "the cursor's cell stays"
    );
    assert_eq!(app.cursor(), a1("A1"));

    keys(&mut app, "u");
    keys(&mut app, "0de");
    assert_eq!(
        value(&app, "B1"),
        CellValue::Empty,
        "e lands on the cell it includes"
    );
}

#[test]
fn counts_multiply_across_an_operator_and_other_keys_cancel_it() {
    let (_file, mut app) = app_with(RUNS);
    keys(&mut app, "2d2j");
    assert_eq!(value(&app, "A5"), CellValue::Empty);
    assert_eq!(value(&app, "A6"), CellValue::Number(16.0));

    keys(&mut app, "u");
    keys(&mut app, "dQ");
    assert_eq!(value(&app, "A1"), CellValue::Number(1.0));
    keys(&mut app, "j");
    assert_eq!(
        value(&app, "A2"),
        CellValue::Number(12.0),
        "the operator did not wait"
    );
    press(&mut app, KeyCode::Char('d'));
    press(&mut app, KeyCode::Esc);
    keys(&mut app, "j");
    assert_eq!(value(&app, "A2"), CellValue::Number(12.0));
    assert_eq!(value(&app, "A3"), CellValue::Number(13.0));
}

#[test]
fn yank_with_a_motion_leaves_cells_alone_and_c_starts_typing() {
    let (_file, mut app) = app_with(RUNS);
    keys(&mut app, "y$");
    assert_eq!(app.status(), "5 cells yanked");
    assert_eq!(value(&app, "G1"), CellValue::Number(7.0));

    keys(&mut app, "cw");
    assert_eq!(app.mode(), Mode::Insert);
    assert_eq!(value(&app, "B1"), CellValue::Empty);
    keys(&mut app, "5");
    press(&mut app, KeyCode::Esc);
    assert_eq!(value(&app, "A1"), CellValue::Number(5.0));
}

#[test]
fn capital_c_and_s_clear_and_start_typing() {
    let (_file, mut app) = app_with(RUNS);
    keys(&mut app, "lC");
    assert_eq!(app.mode(), Mode::Insert);
    assert_eq!(value(&app, "A1"), CellValue::Number(1.0));
    assert_eq!(value(&app, "B1"), CellValue::Empty);
    assert_eq!(value(&app, "G1"), CellValue::Empty);
    press(&mut app, KeyCode::Esc);

    keys(&mut app, "jS");
    assert_eq!(app.mode(), Mode::Insert);
    assert_eq!(value(&app, "A2"), CellValue::Empty);
}

#[test]
fn o_and_capital_o_edit_the_cells_below_and_above() {
    let (_file, mut app) = app_with(RUNS);
    keys(&mut app, "o");
    assert_eq!(app.mode(), Mode::Insert);
    assert_eq!(app.cursor(), a1("A2"));
    assert_eq!(app.buffer(), "12");
    ctrl(&mut app, KeyCode::Char('c'));

    keys(&mut app, "O");
    assert_eq!(app.mode(), Mode::Insert);
    assert_eq!(app.cursor(), a1("A1"));
    assert_eq!(app.buffer(), "1");
}

fn run(app: &mut App, command: &str) {
    keys(app, ":");
    keys(app, command);
    press(app, KeyCode::Enter);
}

#[test]
fn w_saves_and_q_refuses_while_there_are_unsaved_edits() {
    let (file, mut app) = app_with("");
    enter_value(&mut app, "5");

    run(&mut app, "q");
    assert!(!app.should_quit());
    assert!(app.status().starts_with("E37"), "{}", app.status());

    run(&mut app, "w");
    assert_eq!(app.status(), "Saved book.xlsx");
    assert_eq!(app.mode(), Mode::Normal);
    let reloaded = Workbook::open(&file.path).unwrap();
    assert_eq!(
        reloaded.sheets[0].cell(a1("A1")).value,
        CellValue::Number(5.0)
    );

    run(&mut app, "q");
    assert!(app.should_quit());
}

#[test]
fn q_bang_discards_and_wq_saves_then_quits() {
    let (file, mut app) = app_with("");
    enter_value(&mut app, "5");
    run(&mut app, "q!");
    assert!(app.should_quit());
    let unchanged = Workbook::open(&file.path).unwrap();
    assert_eq!(unchanged.sheets[0].cell(a1("A1")), Cell::default());

    for command in ["wq", "x"] {
        let (file, mut app) = app_with("");
        enter_value(&mut app, "5");
        run(&mut app, command);
        assert!(app.should_quit(), "{command}");
        let saved = Workbook::open(&file.path).unwrap();
        assert_eq!(
            saved.sheets[0].cell(a1("A1")).value,
            CellValue::Number(5.0),
            "{command}"
        );
    }
}

#[test]
fn zz_saves_and_quits_and_zq_quits_without_saving() {
    let (file, mut app) = app_with("");
    enter_value(&mut app, "5");
    keys(&mut app, "ZZ");
    assert!(app.should_quit());
    let saved = Workbook::open(&file.path).unwrap();
    assert_eq!(saved.sheets[0].cell(a1("A1")).value, CellValue::Number(5.0));

    let (file, mut app) = app_with("");
    enter_value(&mut app, "5");
    keys(&mut app, "ZQ");
    assert!(app.should_quit());
    let unchanged = Workbook::open(&file.path).unwrap();
    assert_eq!(unchanged.sheets[0].cell(a1("A1")), Cell::default());
}

#[test]
fn a_failed_wq_stays_open() {
    let (file, mut app) = app_with("");
    enter_value(&mut app, "5");
    std::fs::write(&file.path, b"changed").unwrap();
    run(&mut app, "wq");
    assert!(!app.should_quit());
    assert!(app.status().starts_with("Save failed:"), "{}", app.status());
}

#[test]
fn numbers_and_addresses_jump_and_anything_else_is_an_error() {
    let (_file, mut app) = app_with("");
    run(&mut app, "25");
    assert_eq!(app.cursor(), a1("A25"));
    run(&mut app, "C7");
    assert_eq!(app.cursor(), a1("C7"));
    run(&mut app, "nonsense");
    assert_eq!(app.status(), "E492: Not an editor command: nonsense");
    assert_eq!(app.cursor(), a1("C7"));
}

#[test]
fn esc_and_backspace_leave_the_command_line() {
    let (_file, mut app) = app_with("");
    keys(&mut app, ":q");
    assert_eq!(app.mode(), Mode::Command);
    assert_eq!(app.buffer(), "q");
    press(&mut app, KeyCode::Esc);
    assert_eq!(app.mode(), Mode::Normal);
    assert!(!app.should_quit());

    keys(&mut app, ":q");
    press(&mut app, KeyCode::Backspace);
    assert_eq!(app.buffer(), "");
    assert_eq!(app.mode(), Mode::Command);
    press(&mut app, KeyCode::Backspace);
    assert_eq!(app.mode(), Mode::Normal);
}

#[test]
fn prefixed_keys_that_are_not_motions_cancel_a_waiting_operator() {
    let (_file, mut app) = app_with(RUNS);
    keys(&mut app, "dgQ");
    assert_eq!(value(&app, "A1"), CellValue::Number(1.0));
    keys(&mut app, "dg");
    press(&mut app, KeyCode::Esc);
    assert_eq!(value(&app, "A1"), CellValue::Number(1.0));
    keys(&mut app, "dzz");
    assert_eq!(value(&app, "A1"), CellValue::Number(1.0));
    keys(&mut app, "j");
    assert_eq!(
        value(&app, "A2"),
        CellValue::Number(12.0),
        "nothing is left waiting"
    );
}

#[test]
fn backward_operators_that_cannot_move_change_nothing() {
    let (_file, mut app) = app_with(RUNS);
    keys(&mut app, "dh");
    assert_eq!(value(&app, "A1"), CellValue::Number(1.0));
    keys(&mut app, "db");
    assert_eq!(value(&app, "A1"), CellValue::Number(1.0));
}

#[test]
fn dw_on_the_last_run_takes_the_cursor_cell() {
    let (_file, mut app) = app_with(RUNS);
    keys(&mut app, "$dw");
    assert_eq!(value(&app, "G1"), CellValue::Empty);
    assert_eq!(value(&app, "E1"), CellValue::Number(5.0));
}

fn blank_app() -> App {
    App::new(Workbook::blank().unwrap())
}

#[test]
fn w_with_a_name_saves_a_blank_workbook_to_that_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("new.xlsx");
    let mut app = blank_app();
    enter_value(&mut app, "5");

    run(&mut app, &format!("w {}", path.display()));

    assert_eq!(app.status(), "Saved new.xlsx");
    assert_eq!(app.file_name(), "new.xlsx");
    assert!(!app.workbook.is_dirty());
    assert_eq!(
        Workbook::open(&path).unwrap().sheets[0]
            .cell(a1("A1"))
            .value,
        CellValue::Number(5.0)
    );
}

#[test]
fn w_adds_the_xlsx_extension_to_a_name_without_one() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = blank_app();

    run(
        &mut app,
        &format!("w {}", dir.path().join("my book").display()),
    );

    assert_eq!(app.file_name(), "my book.xlsx");
    assert!(dir.path().join("my book.xlsx").exists());
}

#[test]
fn w_with_a_name_refuses_an_existing_file() {
    let file = write_temp(b"precious");
    let mut app = blank_app();

    run(&mut app, &format!("w {}", file.path.display()));

    assert!(app.status().starts_with("Save failed"), "{}", app.status());
    assert_eq!(app.file_name(), "[No Name]");
    assert_eq!(std::fs::read(&file.path).unwrap(), b"precious");
}

#[test]
fn wq_and_x_with_a_name_save_and_quit() {
    for verb in ["wq", "x"] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("new.xlsx");
        let mut app = blank_app();
        enter_value(&mut app, "5");

        run(&mut app, &format!("{verb} {}", path.display()));

        assert!(app.should_quit(), "{verb}");
        assert!(path.exists(), "{verb}");
    }
}

#[test]
fn wq_with_a_name_stays_open_if_the_save_fails() {
    let file = write_temp(b"precious");
    let mut app = blank_app();

    run(&mut app, &format!("wq {}", file.path.display()));

    assert!(!app.should_quit());
    assert!(app.status().starts_with("Save failed"), "{}", app.status());
}

#[test]
fn w_with_a_name_is_refused_on_an_opened_workbook() {
    let (file, mut app) = app_with("");
    enter_value(&mut app, "5");
    let other = file.path.with_file_name("other.xlsx");

    run(&mut app, &format!("w {}", other.display()));

    assert_eq!(app.status(), "Save As isn't supported yet");
    assert!(!other.exists());
    assert_eq!(app.file_name(), "book.xlsx");
    assert!(app.workbook.is_dirty());
}

/// A blank workbook whose save dialog starts in `dir`, with the dialog open.
fn app_in_save_dialog(dir: &Path) -> App {
    let mut app = blank_app();
    app.set_dialog_directory(dir);
    ctrl(&mut app, KeyCode::Char('s'));
    app
}

#[test]
fn saving_a_blank_workbook_without_a_name_opens_the_save_dialog() {
    let dir = tempfile::tempdir().unwrap();

    let app = app_in_save_dialog(dir.path());
    assert_eq!(app.mode(), Mode::FileDialog);
    assert_eq!(app.file_dialog().unwrap().directory(), dir.path());

    let mut app = blank_app();
    app.set_dialog_directory(dir.path());
    run(&mut app, "w");
    assert_eq!(app.mode(), Mode::FileDialog);
}

#[test]
fn saving_an_opened_workbook_never_opens_the_save_dialog() {
    let (_file, mut app) = app_with("");
    enter_value(&mut app, "5");
    ctrl(&mut app, KeyCode::Char('s'));
    assert_eq!(app.mode(), Mode::Normal);
    assert!(app.file_dialog().is_none());
}

/// A folder holding two subfolders, two workbooks and a file the dialog should hide.
fn save_folder() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("b-folder")).unwrap();
    std::fs::create_dir(dir.path().join("A-folder")).unwrap();
    std::fs::write(dir.path().join("z.xlsx"), b"z").unwrap();
    std::fs::write(dir.path().join("Y.XLSX"), b"y").unwrap();
    std::fs::write(dir.path().join("notes.txt"), b"n").unwrap();
    dir
}

fn listed(app: &App) -> Vec<String> {
    let dialog = app.file_dialog().unwrap();
    dialog.entries().iter().map(|entry| entry.label()).collect()
}

#[test]
fn the_save_dialog_lists_the_parent_then_folders_then_workbooks() {
    let dir = save_folder();
    let app = app_in_save_dialog(dir.path());
    let separator = std::path::MAIN_SEPARATOR;
    assert_eq!(
        listed(&app),
        [
            "..".to_string(),
            format!("A-folder{separator}"),
            format!("b-folder{separator}"),
            "Y.XLSX".to_string(),
            "z.xlsx".to_string(),
        ]
    );
}

fn highlighted(app: &App) -> Option<String> {
    let dialog = app.file_dialog().unwrap();
    dialog
        .highlight()
        .map(|index| dialog.entries()[index].label())
}

#[test]
fn up_and_down_move_the_highlight_and_a_file_fills_in_the_name() {
    let dir = save_folder();
    let mut app = app_in_save_dialog(dir.path());
    assert_eq!(highlighted(&app), None);

    press(&mut app, KeyCode::Up);
    assert_eq!(highlighted(&app).as_deref(), Some("z.xlsx"), "from the end");
    assert_eq!(app.file_dialog().unwrap().name(), "z.xlsx");

    press(&mut app, KeyCode::Down);
    assert_eq!(
        highlighted(&app).as_deref(),
        Some("z.xlsx"),
        "stops at the end"
    );

    press(&mut app, KeyCode::Up);
    assert_eq!(highlighted(&app).as_deref(), Some("Y.XLSX"));
    assert_eq!(app.file_dialog().unwrap().name(), "Y.XLSX");

    for _ in 0..5 {
        press(&mut app, KeyCode::Up);
    }
    assert_eq!(
        highlighted(&app).as_deref(),
        Some(".."),
        "stops at the start"
    );
    assert_eq!(
        app.file_dialog().unwrap().name(),
        "Y.XLSX",
        "folders leave the name alone"
    );
}

#[test]
fn enter_opens_a_highlighted_folder_or_the_parent() {
    let dir = save_folder();
    let mut app = app_in_save_dialog(dir.path());

    press(&mut app, KeyCode::Down);
    press(&mut app, KeyCode::Down);
    press(&mut app, KeyCode::Enter);
    assert_eq!(
        app.file_dialog().unwrap().directory(),
        dir.path().join("A-folder")
    );
    assert_eq!(listed(&app), [".."]);
    assert_eq!(highlighted(&app), None);

    press(&mut app, KeyCode::Down);
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.file_dialog().unwrap().directory(), dir.path());
    assert_eq!(listed(&app).len(), 5);
}

#[test]
fn typing_edits_the_name_and_removes_the_highlight() {
    let dir = save_folder();
    let mut app = app_in_save_dialog(dir.path());

    type_text(&mut app, "jk");
    assert_eq!(app.file_dialog().unwrap().name(), "jk", "letters are text");
    assert_eq!(highlighted(&app), None);

    press(&mut app, KeyCode::Down);
    press(&mut app, KeyCode::Backspace);
    assert_eq!(app.file_dialog().unwrap().name(), "j");
    assert_eq!(highlighted(&app), None);

    press(&mut app, KeyCode::Down);
    type_text(&mut app, "x");
    assert_eq!(app.file_dialog().unwrap().name(), "jx");
    assert_eq!(highlighted(&app), None);
}

#[test]
fn enter_saves_the_name_in_the_current_folder_and_closes_the_dialog() {
    let dir = save_folder();
    let mut app = blank_app();
    enter_value(&mut app, "5");
    app.set_dialog_directory(dir.path());
    ctrl(&mut app, KeyCode::Char('s'));

    type_text(&mut app, "budget");
    press(&mut app, KeyCode::Enter);

    assert_eq!(app.mode(), Mode::Normal);
    assert!(app.file_dialog().is_none());
    assert_eq!(app.status(), "Saved budget.xlsx");
    assert!(!app.workbook.is_dirty());
    assert!(!app.should_quit());
    assert_eq!(
        Workbook::open(dir.path().join("budget.xlsx"))
            .unwrap()
            .sheets[0]
            .cell(a1("A1"))
            .value,
        CellValue::Number(5.0)
    );
}

#[test]
fn a_typed_path_is_relative_to_the_current_folder() {
    let dir = save_folder();
    let mut app = app_in_save_dialog(dir.path());

    type_text(&mut app, "A-folder/inner.xlsx");
    press(&mut app, KeyCode::Enter);

    assert_eq!(app.mode(), Mode::Normal);
    assert!(dir.path().join("A-folder").join("inner.xlsx").exists());
}

#[test]
fn an_empty_name_is_refused_with_a_message() {
    let dir = save_folder();
    let mut app = app_in_save_dialog(dir.path());

    type_text(&mut app, "  ");
    press(&mut app, KeyCode::Enter);

    assert_eq!(app.mode(), Mode::FileDialog);
    assert_eq!(app.file_dialog().unwrap().message(), "Enter a file name");

    type_text(&mut app, "a");
    assert_eq!(
        app.file_dialog().unwrap().message(),
        "",
        "the next key clears it"
    );
}

#[test]
fn an_existing_file_asks_before_it_is_overwritten() {
    let dir = save_folder();
    let target = dir.path().join("z.xlsx");
    let mut app = app_in_save_dialog(dir.path());

    press(&mut app, KeyCode::Up);
    press(&mut app, KeyCode::Enter);
    let dialog = app.file_dialog().unwrap();
    assert_eq!(dialog.pending_overwrite(), Some(target.as_path()));
    assert_eq!(std::fs::read(&target).unwrap(), b"z", "nothing written yet");

    press(&mut app, KeyCode::Char('q'));
    assert!(
        app.file_dialog().unwrap().pending_overwrite().is_some(),
        "other keys keep asking"
    );
    assert_eq!(app.file_dialog().unwrap().name(), "z.xlsx");

    press(&mut app, KeyCode::Char('n'));
    assert_eq!(app.file_dialog().unwrap().pending_overwrite(), None);
    assert_eq!(app.mode(), Mode::FileDialog);

    press(&mut app, KeyCode::Enter);
    press(&mut app, KeyCode::Esc);
    assert_eq!(app.file_dialog().unwrap().pending_overwrite(), None);
    assert_eq!(app.mode(), Mode::FileDialog, "Esc goes back to the dialog");
    assert_eq!(std::fs::read(&target).unwrap(), b"z");

    press(&mut app, KeyCode::Enter);
    press(&mut app, KeyCode::Char('y'));
    assert_eq!(app.mode(), Mode::Normal);
    assert_eq!(app.status(), "Saved z.xlsx");
    assert!(Workbook::open(&target).is_ok(), "now a real workbook");
}

#[test]
fn esc_and_ctrl_c_close_the_save_dialog_and_keep_the_edits() {
    let dir = save_folder();
    for cancel in [
        KeyEvent::from(KeyCode::Esc),
        KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
    ] {
        let mut app = blank_app();
        enter_value(&mut app, "5");
        app.set_dialog_directory(dir.path());
        ctrl(&mut app, KeyCode::Char('s'));
        type_text(&mut app, "cancelled");

        app.handle_key(cancel);

        assert_eq!(app.mode(), Mode::Normal);
        assert!(app.file_dialog().is_none());
        assert!(app.workbook.is_dirty());
        assert_eq!(
            app.current_sheet().cell(a1("A1")).value,
            CellValue::Number(5.0)
        );
        assert!(!dir.path().join("cancelled.xlsx").exists());
    }
}

#[test]
fn a_folder_that_cannot_be_read_shows_an_error_and_stays_put() {
    let dir = save_folder();
    let mut app = app_in_save_dialog(dir.path());
    std::fs::remove_dir(dir.path().join("A-folder")).unwrap();

    press(&mut app, KeyCode::Down);
    press(&mut app, KeyCode::Down);
    press(&mut app, KeyCode::Enter);

    let dialog = app.file_dialog().unwrap();
    assert_eq!(dialog.directory(), dir.path());
    assert!(!dialog.message().is_empty());
}

#[test]
fn a_start_folder_that_cannot_be_read_shows_an_error_and_offers_the_parent() {
    let dir = tempfile::tempdir().unwrap();
    let app = app_in_save_dialog(&dir.path().join("missing"));

    assert!(!app.file_dialog().unwrap().message().is_empty());
    assert_eq!(listed(&app), [".."]);
}

#[test]
fn a_failed_save_shows_the_error_and_leaves_the_dialog_open() {
    let dir = save_folder();
    let mut app = app_in_save_dialog(dir.path());

    type_text(&mut app, "no-such-folder/book.xlsx");
    press(&mut app, KeyCode::Enter);

    assert_eq!(app.mode(), Mode::FileDialog);
    let message = app.file_dialog().unwrap().message();
    assert!(message.starts_with("Save failed"), "{message}");
    assert_eq!(app.file_name(), "[No Name]");
}

#[test]
fn wq_and_the_quit_prompt_quit_once_the_dialog_has_saved() {
    let dir = save_folder();
    let open_with_wq = |app: &mut App| run(app, "wq");
    let open_with_prompt = |app: &mut App| {
        ctrl(app, KeyCode::Char('q'));
        press(app, KeyCode::Char('s'));
    };
    let openers: [&dyn Fn(&mut App); 2] = [&open_with_wq, &open_with_prompt];
    for (index, open) in openers.into_iter().enumerate() {
        let mut app = blank_app();
        enter_value(&mut app, "5");
        app.set_dialog_directory(dir.path());

        open(&mut app);
        assert_eq!(app.mode(), Mode::FileDialog);
        assert!(!app.should_quit());

        type_text(&mut app, &format!("quit{index}"));
        press(&mut app, KeyCode::Enter);
        assert!(app.should_quit());
    }
}

#[test]
fn cancelling_the_dialog_cancels_the_quit_too() {
    let dir = save_folder();
    let mut app = blank_app();
    enter_value(&mut app, "5");
    app.set_dialog_directory(dir.path());

    run(&mut app, "wq");
    press(&mut app, KeyCode::Esc);
    assert!(!app.should_quit());

    ctrl(&mut app, KeyCode::Char('s'));
    type_text(&mut app, "later");
    press(&mut app, KeyCode::Enter);
    assert!(app.file_dialog().is_none());
    assert!(!app.should_quit(), "a plain save afterwards doesn't quit");
}

#[test]
fn a_failed_save_in_the_dialog_does_not_quit() {
    let dir = save_folder();
    let mut app = blank_app();
    app.set_dialog_directory(dir.path());
    run(&mut app, "wq");

    type_text(&mut app, "no-such-folder/book.xlsx");
    press(&mut app, KeyCode::Enter);

    assert!(!app.should_quit());
    assert_eq!(app.mode(), Mode::FileDialog);
}

#[test]
fn zz_and_x_quit_a_blank_workbook_with_no_edits() {
    let mut app = blank_app();
    keys(&mut app, "ZZ");
    assert!(app.should_quit());
    assert!(app.file_dialog().is_none());

    let mut app = blank_app();
    run(&mut app, "x");
    assert!(app.should_quit());
    assert!(app.file_dialog().is_none());
}

#[test]
fn zz_and_x_open_the_dialog_for_a_blank_workbook_with_edits() {
    let dir = save_folder();
    let mut app = blank_app();
    enter_value(&mut app, "5");
    app.set_dialog_directory(dir.path());

    keys(&mut app, "ZZ");
    assert_eq!(app.mode(), Mode::FileDialog);
    assert!(!app.should_quit());
    press(&mut app, KeyCode::Esc);

    run(&mut app, "x");
    assert_eq!(app.mode(), Mode::FileDialog);
    type_text(&mut app, "kept");
    press(&mut app, KeyCode::Enter);
    assert!(app.should_quit());
}

#[test]
fn wq_opens_the_dialog_for_a_blank_workbook_with_no_edits() {
    let dir = save_folder();
    let mut app = blank_app();
    app.set_dialog_directory(dir.path());

    run(&mut app, "wq");
    assert_eq!(app.mode(), Mode::FileDialog);
    assert!(!app.should_quit());
}

/// A workbook with 42 in A1, to open from another one.
fn other_workbook() -> TempXlsx {
    write_temp(&single_sheet(
        &sheet(r#"<row r="1"><c r="A1"><v>42</v></c></row>"#),
        &[],
    ))
}

#[test]
fn e_with_a_name_opens_that_file_at_the_top_of_its_first_sheet() {
    let (_first, mut app) = app_with("");
    let second = other_workbook();
    keys(&mut app, "50jl");

    run(&mut app, &format!("e {}", second.path.display()));

    assert_eq!(app.status(), "Opened book.xlsx");
    assert_eq!(app.workbook.path(), Some(second.path.as_path()));
    assert_eq!(app.cursor(), a1("A1"));
    assert_eq!(app.scroll(), a1("A1"));
    assert_eq!(value(&app, "A1"), CellValue::Number(42.0));
}

#[test]
fn e_adds_the_xlsx_extension_to_a_name_without_one() {
    let (_first, mut app) = app_with("");
    let second = other_workbook();
    let name = second.path.with_extension("");

    run(&mut app, &format!("e {}", name.display()));

    assert_eq!(app.workbook.path(), Some(second.path.as_path()));
}

#[test]
fn opening_a_file_returns_to_its_first_sheet() {
    let samples = Path::new(env!("CARGO_MANIFEST_DIR")).join("samples");
    let book = Workbook::open(samples.join("financial-model.xlsx")).unwrap();
    let mut app = App::new(book);
    keys(&mut app, "gt");
    assert_eq!(app.sheet_index(), 1);
    let second = other_workbook();

    run(&mut app, &format!("e {}", second.path.display()));

    assert_eq!(app.sheet_index(), 0);
    keys(&mut app, "gt");
    assert_eq!(app.sheet_index(), 0, "the new file has one sheet");
}

#[test]
fn undo_history_does_not_follow_into_another_file() {
    let (_first, mut app) = app_with("");
    enter_value(&mut app, "5");
    run(&mut app, "w");
    let second = other_workbook();

    run(&mut app, &format!("e {}", second.path.display()));
    press(&mut app, KeyCode::Char('u'));

    assert_eq!(app.status(), "Nothing to undo");
    assert_eq!(value(&app, "A1"), CellValue::Number(42.0));
}

#[test]
fn saving_after_opening_writes_to_the_opened_file() {
    let (first, mut app) = app_with("");
    let before = std::fs::read(&first.path).unwrap();
    let second = other_workbook();
    run(&mut app, &format!("e {}", second.path.display()));

    press(&mut app, KeyCode::Char('j'));
    enter_value(&mut app, "7");
    run(&mut app, "w");

    let saved = Workbook::open(&second.path).unwrap();
    assert_eq!(saved.sheets[0].cell(a1("A2")).value, CellValue::Number(7.0));
    assert_eq!(std::fs::read(&first.path).unwrap(), before);
}

#[test]
fn yanked_cells_can_be_pasted_into_another_file() {
    let (_first, mut app) = app_with("");
    enter_value(&mut app, "5");
    keys(&mut app, "kyy");
    run(&mut app, "w");
    let second = other_workbook();
    run(&mut app, &format!("e {}", second.path.display()));

    keys(&mut app, "p");

    assert_eq!(value(&app, "A2"), CellValue::Number(5.0), "the row below");
    assert_eq!(value(&app, "A1"), CellValue::Number(42.0));
}

#[test]
fn a_file_that_cannot_be_opened_keeps_the_current_workbook() {
    let (first, mut app) = app_with(r#"<row r="1"><c r="A1"><v>1</v></c></row>"#);
    keys(&mut app, "j");
    let broken = write_temp(b"not a workbook");

    for target in [
        broken.path.clone(),
        broken.path.with_file_name("missing.xlsx"),
    ] {
        run(&mut app, &format!("e {}", target.display()));

        assert!(app.status().starts_with("Open failed"), "{}", app.status());
        assert_eq!(app.workbook.path(), Some(first.path.as_path()));
        assert_eq!(app.cursor(), a1("A2"));
        assert_eq!(value(&app, "A1"), CellValue::Number(1.0));
    }
}

#[test]
fn e_with_a_name_refuses_while_there_are_unsaved_edits() {
    let (first, mut app) = app_with("");
    enter_value(&mut app, "5");
    let second = other_workbook();

    run(&mut app, &format!("e {}", second.path.display()));

    assert_eq!(
        app.status(),
        "E37: No write since last change (add ! to override)"
    );
    assert_eq!(app.workbook.path(), Some(first.path.as_path()));
    assert_eq!(value(&app, "A1"), CellValue::Number(5.0));
}

#[test]
fn e_bang_with_a_name_discards_unsaved_edits() {
    let (first, mut app) = app_with("");
    let before = std::fs::read(&first.path).unwrap();
    enter_value(&mut app, "5");
    let second = other_workbook();

    run(&mut app, &format!("e! {}", second.path.display()));

    assert_eq!(app.workbook.path(), Some(second.path.as_path()));
    assert!(!app.workbook.is_dirty());
    assert_eq!(std::fs::read(&first.path).unwrap(), before);
}

/// A folder with a workbook holding 42 in A1, and a file that isn't really a workbook.
fn open_folder() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let good = other_workbook();
    std::fs::copy(&good.path, dir.path().join("good.xlsx")).unwrap();
    std::fs::write(dir.path().join("broken.xlsx"), b"not a workbook").unwrap();
    dir
}

#[test]
fn e_and_ctrl_o_open_the_dialog_for_choosing_a_file() {
    let dir = open_folder();
    let open_with_e = |app: &mut App| run(app, "e");
    let open_with_ctrl_o = |app: &mut App| ctrl(app, KeyCode::Char('o'));
    let openers: [&dyn Fn(&mut App); 2] = [&open_with_e, &open_with_ctrl_o];
    for open in openers {
        let mut app = blank_app();
        app.set_dialog_directory(dir.path());
        open(&mut app);
        assert_eq!(app.mode(), Mode::FileDialog);
        assert_eq!(app.file_dialog().unwrap().kind(), DialogKind::Open);
    }
}

#[test]
fn saving_still_opens_the_dialog_for_saving() {
    let dir = open_folder();
    let app = app_in_save_dialog(dir.path());
    assert_eq!(app.file_dialog().unwrap().kind(), DialogKind::Save);
}

#[test]
fn f5_reloads_the_workbook_from_disk_keeping_the_cursor() {
    let (file, mut app) = app_with(r#"<row r="1"><c r="A1"><v>1</v></c></row>"#);
    press(&mut app, KeyCode::Char('j'));
    press(&mut app, KeyCode::Char('l'));
    let changed = single_sheet(&sheet(r#"<row r="1"><c r="A1"><v>2</v></c></row>"#), &[]);
    std::fs::write(&file.path, changed).unwrap();

    press(&mut app, KeyCode::F(5));

    assert_eq!(value(&app, "A1"), CellValue::Number(2.0));
    assert_eq!(app.cursor(), a1("B2"));
    assert_eq!(app.status(), "Reloaded book.xlsx");
}

#[test]
fn f5_asks_about_unsaved_edits_and_discarding_reloads() {
    let (_file, mut app) = app_with("");
    enter_value(&mut app, "5");

    press(&mut app, KeyCode::F(5));
    assert_eq!(app.mode(), Mode::ConfirmDiscard);
    press(&mut app, KeyCode::Esc);
    assert_eq!(value(&app, "A1"), CellValue::Number(5.0));

    press(&mut app, KeyCode::F(5));
    press(&mut app, KeyCode::Char('d'));
    assert_eq!(value(&app, "A1"), CellValue::Empty);
    assert!(!app.workbook.is_dirty());
}

#[test]
fn f5_says_so_when_there_is_no_file_to_reload() {
    let mut app = App::new(Workbook::blank().unwrap());
    enter_value(&mut app, "5");

    press(&mut app, KeyCode::F(5));

    assert_eq!(app.status(), "Nothing to reload");
    assert_eq!(app.mode(), Mode::Normal);
    assert_eq!(value(&app, "A1"), CellValue::Number(5.0));
}

#[test]
fn f5_keeps_the_loaded_workbook_if_the_file_cannot_be_read() {
    let (file, mut app) = app_with(r#"<row r="1"><c r="A1"><v>1</v></c></row>"#);
    std::fs::write(&file.path, b"not a workbook").unwrap();

    press(&mut app, KeyCode::F(5));

    assert!(
        app.status().starts_with("Reload failed:"),
        "{}",
        app.status()
    );
    assert!(app.status_is_error());
    assert_eq!(value(&app, "A1"), CellValue::Number(1.0));
}

#[test]
fn f5_starts_at_the_top_if_the_sheet_is_gone() {
    let (file, mut app) = app_from(&two_sheets());
    press(&mut app, KeyCode::F(6));
    press(&mut app, KeyCode::Char('j'));
    std::fs::write(&file.path, single_sheet(&sheet(""), &[])).unwrap();

    press(&mut app, KeyCode::F(5));

    assert_eq!(app.sheet_index(), 0);
    assert_eq!(app.cursor(), a1("A1"));
}

#[test]
fn ctrl_g_goes_to_a_cell_or_a_row() {
    let (_file, mut app) = app_with("");

    ctrl(&mut app, KeyCode::Char('g'));
    assert_eq!(app.mode(), Mode::GoTo);
    type_text(&mut app, "c7");
    assert_eq!(app.buffer(), "c7");
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.mode(), Mode::Normal);
    assert_eq!(app.cursor(), a1("C7"));

    ctrl(&mut app, KeyCode::Char('g'));
    type_text(&mut app, "25");
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.cursor(), a1("C25"), "the column stays");
}

#[test]
fn ctrl_g_starts_with_an_empty_box_after_editing() {
    let (_file, mut app) = app_with("");
    enter_value(&mut app, "old");

    ctrl(&mut app, KeyCode::Char('g'));

    assert_eq!(app.buffer(), "");
    assert_eq!(app.caret(), 0);
}

#[test]
fn ctrl_g_stays_open_on_nonsense_and_says_why() {
    let (_file, mut app) = app_with("");

    ctrl(&mut app, KeyCode::Char('g'));
    type_text(&mut app, "nope");
    press(&mut app, KeyCode::Enter);

    assert_eq!(app.mode(), Mode::GoTo);
    assert_eq!(app.status(), "nope isn't a cell or a row");
    assert!(app.status_is_error());
    assert_eq!(app.cursor(), a1("A1"));

    press(&mut app, KeyCode::Backspace);
    assert_eq!(app.buffer(), "nop");
    assert_eq!(app.status(), "", "typing clears the complaint");
}

#[test]
fn esc_ctrl_c_and_an_empty_enter_leave_go_to_where_you_were() {
    let (_file, mut app) = app_with("");

    ctrl(&mut app, KeyCode::Char('g'));
    type_text(&mut app, "C7");
    press(&mut app, KeyCode::Esc);
    assert_eq!(app.mode(), Mode::Normal);
    assert_eq!(app.cursor(), a1("A1"));

    ctrl(&mut app, KeyCode::Char('g'));
    ctrl(&mut app, KeyCode::Char('c'));
    assert_eq!(app.mode(), Mode::Normal);

    ctrl(&mut app, KeyCode::Char('g'));
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.mode(), Mode::Normal);
    assert_eq!(app.status(), "");
}

fn searchable() -> (TempXlsx, App) {
    app_with(
        r#"<row r="1"><c r="A1" t="inlineStr"><is><t>Apples</t></is></c><c r="B1"><v>1.5</v></c></row><row r="2"><c r="B2" t="inlineStr"><is><t>pears and APPLES</t></is></c></row><row r="3"><c r="A3"><f>SUM(B1:B2)</f><v>1.5</v></c></row>"#,
    )
}

fn search(app: &mut App, query: &str) {
    press(app, KeyCode::Char('/'));
    type_text(app, query);
    press(app, KeyCode::Enter);
}

#[test]
fn slash_finds_the_next_match_ignoring_case_and_wraps_around() {
    let (_file, mut app) = searchable();

    press(&mut app, KeyCode::Char('/'));
    assert_eq!(app.mode(), Mode::Search);
    assert_eq!(app.cursor(), a1("A1"), "nothing moves until Enter");
    type_text(&mut app, "apples");
    press(&mut app, KeyCode::Enter);

    assert_eq!(app.mode(), Mode::Normal);
    assert_eq!(app.cursor(), a1("B2"), "skips the cell it started on");

    press(&mut app, KeyCode::Char('n'));
    assert_eq!(app.cursor(), a1("A1"), "wraps to the top");
}

#[test]
fn capital_n_finds_the_previous_match_and_wraps_around() {
    let (_file, mut app) = searchable();
    search(&mut app, "apples");
    assert_eq!(app.cursor(), a1("B2"));

    press(&mut app, KeyCode::Char('N'));
    assert_eq!(app.cursor(), a1("A1"));
    press(&mut app, KeyCode::Char('N'));
    assert_eq!(app.cursor(), a1("B2"), "wraps to the bottom");
}

#[test]
fn search_looks_at_numbers_and_formula_text() {
    let (_file, mut app) = searchable();

    search(&mut app, "1.5");
    assert_eq!(app.cursor(), a1("B1"));

    search(&mut app, "sum(");
    assert_eq!(app.cursor(), a1("A3"));
}

#[test]
fn search_stays_on_the_current_sheet() {
    let (_file, mut app) = app_from(&two_sheets());
    let name = app.current_sheet().name.clone();

    search(&mut app, "no such text anywhere");

    assert_eq!(app.current_sheet().name, name);
    assert_eq!(app.sheet_index(), 0);
}

#[test]
fn a_search_with_no_match_says_so_in_red_and_stays_put() {
    let (_file, mut app) = searchable();
    press(&mut app, KeyCode::Char('j'));

    search(&mut app, "plums");

    assert_eq!(app.cursor(), a1("A2"));
    assert_eq!(app.status(), "No match for plums");
    assert!(app.status_is_error());
}

#[test]
fn n_without_a_search_says_so() {
    let (_file, mut app) = searchable();

    press(&mut app, KeyCode::Char('n'));
    assert_eq!(app.status(), "Nothing to search for");
    press(&mut app, KeyCode::Char('N'));
    assert_eq!(app.status(), "Nothing to search for");
    assert_eq!(app.cursor(), a1("A1"));
}

#[test]
fn leaving_the_search_prompt_changes_nothing() {
    let (_file, mut app) = searchable();
    search(&mut app, "apples");

    press(&mut app, KeyCode::Char('/'));
    type_text(&mut app, "pears");
    press(&mut app, KeyCode::Esc);
    assert_eq!(app.mode(), Mode::Normal);
    assert_eq!(app.cursor(), a1("B2"));

    press(&mut app, KeyCode::Char('/'));
    ctrl(&mut app, KeyCode::Char('c'));
    assert_eq!(app.mode(), Mode::Normal);

    press(&mut app, KeyCode::Char('n'));
    assert_eq!(app.cursor(), a1("A1"), "the last real search still applies");
}

#[test]
fn an_empty_search_repeats_the_last_one() {
    let (_file, mut app) = searchable();
    search(&mut app, "apples");
    assert_eq!(app.cursor(), a1("B2"));

    search(&mut app, "");

    assert_eq!(app.cursor(), a1("A1"));
}

#[test]
fn question_mark_opens_help_and_any_key_closes_it_without_doing_anything() {
    let (_file, mut app) = app_with("");

    press(&mut app, KeyCode::Char('?'));
    assert_eq!(app.mode(), Mode::Help);

    press(&mut app, KeyCode::Char('q'));
    assert_eq!(app.mode(), Mode::Normal);
    assert!(!app.should_quit(), "the key is swallowed");

    press(&mut app, KeyCode::F(1));
    assert_eq!(app.mode(), Mode::Help);
    ctrl(&mut app, KeyCode::Char('c'));
    assert_eq!(app.mode(), Mode::Normal);
    assert!(!app.should_quit());
}

#[test]
fn help_from_a_selection_returns_to_the_selection() {
    let (_file, mut app) = app_with("");
    press(&mut app, KeyCode::Char('V'));

    press(&mut app, KeyCode::Char('?'));
    assert_eq!(app.mode(), Mode::Help);
    assert_eq!(app.help_table(), tuicalc::help::VISUAL_HELP);
    press(&mut app, KeyCode::Char('x'));

    assert_eq!(app.mode(), Mode::VisualRow);
    assert!(app.selection().is_some());
}

#[test]
fn f1_helps_while_editing_and_question_mark_is_still_text() {
    let (_file, mut app) = app_with("");
    press(&mut app, KeyCode::Char('s'));
    press(&mut app, KeyCode::Char('?'));
    assert_eq!(app.buffer(), "?");

    press(&mut app, KeyCode::F(1));
    assert_eq!(app.mode(), Mode::Help);
    assert_eq!(app.help_table(), tuicalc::help::INSERT_HELP);

    press(&mut app, KeyCode::Char('x'));
    assert_eq!(app.mode(), Mode::Insert);
    assert_eq!(app.buffer(), "?", "the edit carries on untouched");
}

#[test]
fn e_refuses_while_there_are_unsaved_edits() {
    let (_file, mut app) = app_with("");
    enter_value(&mut app, "5");

    run(&mut app, "e");
    assert!(app.status().starts_with("E37"), "{}", app.status());
    assert_eq!(app.mode(), Mode::Normal);
}

#[test]
fn ctrl_o_asks_about_unsaved_edits() {
    let (_file, mut app) = app_with("");
    enter_value(&mut app, "5");

    ctrl(&mut app, KeyCode::Char('o'));
    assert_eq!(app.mode(), Mode::ConfirmDiscard);
    press(&mut app, KeyCode::Esc);
    assert_eq!(app.mode(), Mode::Normal);
    assert!(app.workbook.is_dirty());
}

#[test]
fn discarding_at_ctrl_o_keeps_the_edits_until_a_file_is_chosen() {
    let dir = open_folder();
    let (_file, mut app) = app_with("");
    app.set_dialog_directory(dir.path());
    enter_value(&mut app, "5");

    ctrl(&mut app, KeyCode::Char('o'));
    press(&mut app, KeyCode::Char('d'));
    assert_eq!(app.file_dialog().unwrap().kind(), DialogKind::Open);

    press(&mut app, KeyCode::Esc);
    assert_eq!(value(&app, "A1"), CellValue::Number(5.0));
    assert!(app.workbook.is_dirty());
}

#[test]
fn saving_at_ctrl_o_saves_and_then_opens_the_dialog() {
    let dir = open_folder();
    let (file, mut app) = app_with("");
    app.set_dialog_directory(dir.path());
    enter_value(&mut app, "5");

    ctrl(&mut app, KeyCode::Char('o'));
    press(&mut app, KeyCode::Char('s'));

    assert_eq!(app.file_dialog().unwrap().kind(), DialogKind::Open);
    assert!(!app.workbook.is_dirty());
    let on_disk = Workbook::open(&file.path).unwrap();
    assert_eq!(
        on_disk.sheets[0].cell(a1("A1")).value,
        CellValue::Number(5.0)
    );
}

#[test]
fn saving_an_unnamed_workbook_at_ctrl_o_names_it_and_then_opens_the_dialog() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = App::new(Workbook::blank().unwrap());
    app.set_dialog_directory(dir.path());
    enter_value(&mut app, "5");

    ctrl(&mut app, KeyCode::Char('o'));
    press(&mut app, KeyCode::Char('s'));
    assert_eq!(app.file_dialog().unwrap().kind(), DialogKind::Save);
    for c in "named".chars() {
        press(&mut app, KeyCode::Char(c));
    }
    press(&mut app, KeyCode::Enter);

    assert!(dir.path().join("named.xlsx").exists());
    assert_eq!(app.file_dialog().unwrap().kind(), DialogKind::Open);
}

#[test]
fn cancelling_the_save_dialog_at_ctrl_o_keeps_the_edits_and_does_not_open() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = App::new(Workbook::blank().unwrap());
    app.set_dialog_directory(dir.path());
    enter_value(&mut app, "5");

    ctrl(&mut app, KeyCode::Char('o'));
    press(&mut app, KeyCode::Char('s'));
    press(&mut app, KeyCode::Esc);

    assert_eq!(app.mode(), Mode::Normal);
    assert!(app.file_dialog().is_none());
    assert_eq!(value(&app, "A1"), CellValue::Number(5.0));
}

#[test]
fn a_failed_save_at_ctrl_o_stays_put_with_the_error() {
    let (file, mut app) = app_with("");
    enter_value(&mut app, "5");
    ctrl(&mut app, KeyCode::Char('o'));
    std::fs::write(&file.path, b"changed").unwrap();

    press(&mut app, KeyCode::Char('s'));

    assert_eq!(app.mode(), Mode::Normal);
    assert!(app.file_dialog().is_none());
    assert!(app.status().starts_with("Save failed:"), "{}", app.status());
    assert!(app.status_is_error());
    assert!(app.next_step().is_none());
}

#[test]
fn e_bang_opens_the_dialog_and_cancelling_keeps_the_edits() {
    let dir = open_folder();
    let (file, mut app) = app_with("");
    app.set_dialog_directory(dir.path());
    enter_value(&mut app, "5");

    run(&mut app, "e!");
    assert_eq!(app.mode(), Mode::FileDialog);

    press(&mut app, KeyCode::Esc);
    assert_eq!(app.mode(), Mode::Normal);
    assert!(app.workbook.is_dirty());
    assert_eq!(value(&app, "A1"), CellValue::Number(5.0));
    assert_eq!(app.workbook.path(), Some(file.path.as_path()));
}

/// A workbook with unsaved edits and the open dialog on `dir`, as `:e!` leaves it.
fn app_choosing_a_file_to_open(dir: &Path) -> (TempXlsx, App) {
    let (file, mut app) = app_with("");
    app.set_dialog_directory(dir);
    enter_value(&mut app, "5");
    run(&mut app, "e!");
    (file, app)
}

#[test]
fn choosing_a_file_in_the_dialog_opens_it() {
    let dir = open_folder();
    let (_file, mut app) = app_choosing_a_file_to_open(dir.path());

    press(&mut app, KeyCode::Up);
    assert_eq!(highlighted(&app).as_deref(), Some("good.xlsx"));
    press(&mut app, KeyCode::Enter);

    assert_eq!(app.mode(), Mode::Normal);
    assert!(app.file_dialog().is_none());
    assert_eq!(app.status(), "Opened good.xlsx");
    assert_eq!(value(&app, "A1"), CellValue::Number(42.0));
    assert!(!app.workbook.is_dirty());
}

#[test]
fn typing_a_name_in_the_dialog_opens_that_file() {
    let dir = open_folder();
    let (_file, mut app) = app_choosing_a_file_to_open(dir.path());

    type_text(&mut app, "good");
    press(&mut app, KeyCode::Enter);

    assert_eq!(app.file_name(), "good.xlsx");
    assert_eq!(
        app.file_dialog()
            .map(|dialog| dialog.pending_overwrite().is_some()),
        None,
        "opening never asks about overwriting"
    );
}

#[test]
fn a_file_that_cannot_be_opened_leaves_the_dialog_open_with_the_error() {
    let dir = open_folder();
    let (file, mut app) = app_choosing_a_file_to_open(dir.path());

    for name in ["broken.xlsx", "missing.xlsx"] {
        type_text(&mut app, name);
        press(&mut app, KeyCode::Enter);

        assert_eq!(app.mode(), Mode::FileDialog, "{name}");
        let dialog = app.file_dialog().unwrap();
        assert!(dialog.message().starts_with("Open failed"), "{name}");
        assert_eq!(app.workbook.path(), Some(file.path.as_path()));
        assert_eq!(value(&app, "A1"), CellValue::Number(5.0));
        for _ in 0..name.len() {
            press(&mut app, KeyCode::Backspace);
        }
    }
}
