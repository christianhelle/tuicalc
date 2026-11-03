mod common;

use common::*;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use tuicalc::app::App;
use tuicalc::help::{HELP_KEYS, HELP_ROWS, HELP_WIDTH, INSERT_HELP, NORMAL_HELP, VISUAL_HELP};
use tuicalc::xlsx::Workbook;
use unicode_width::UnicodeWidthStr;

#[test]
fn every_table_fits_the_dialog() {
    for table in [NORMAL_HELP, INSERT_HELP, VISUAL_HELP] {
        assert!(table.len() <= HELP_ROWS, "{} rows", table.len());
        for (keys, action) in table {
            assert!(keys.width() <= HELP_KEYS, "{keys}");
            let room = usize::from(HELP_WIDTH) - 2 - HELP_KEYS;
            assert!(action.width() <= room, "{action}");
        }
    }
}

#[test]
fn every_table_says_how_to_close_it() {
    for table in [NORMAL_HELP, INSERT_HELP, VISUAL_HELP] {
        assert_eq!(table.last(), Some(&("", "Press any key to close")));
    }
}

/// What a key visibly did to the app, from a cell with filled neighbours.
fn reaction(code: KeyCode, modifiers: KeyModifiers) -> bool {
    let rows = r#"<row r="1"><c r="A1"><v>1</v></c><c r="B1"><v>2</v></c><c r="C1"><v>3</v></c></row><row r="2"><c r="A2"><v>4</v></c><c r="B2"><v>5</v></c><c r="C2"><v>6</v></c></row><row r="3"><c r="A3"><v>7</v></c><c r="B3"><v>8</v></c><c r="C3"><v>9</v></c></row>"#;
    let file = write_temp(&single_sheet(&sheet(rows), &[]));
    let mut app = App::new(Workbook::open(&file.path).unwrap());
    // Yanked cells give `p` something to do, and the middle cell gives motions somewhere to go.
    for c in "yyjl".chars() {
        app.handle_key(KeyEvent::from(KeyCode::Char(c)));
    }
    let before = (app.mode(), app.cursor(), app.sheet_index(), app.scroll());
    app.handle_key(KeyEvent::new(code, modifiers));
    let after = (app.mode(), app.cursor(), app.sheet_index(), app.scroll());
    before != after
        || app.should_quit()
        || !app.pending_keys().is_empty()
        || !app.status().is_empty()
        || app.document().workbook().is_dirty()
}

#[test]
fn every_letter_and_symbol_that_does_something_is_in_the_help() {
    let named: String = NORMAL_HELP.iter().map(|(keys, _)| *keys).collect();
    for byte in b'!'..=b'~' {
        let c = byte as char;
        // Digits are counts.
        if !reaction(KeyCode::Char(c), KeyModifiers::NONE) || c.is_ascii_digit() {
            continue;
        }
        assert!(named.contains(c), "{c} works but isn't in the help");
    }
}

#[test]
fn every_ctrl_key_that_does_something_is_in_the_help() {
    let named: String = NORMAL_HELP
        .iter()
        .map(|(keys, _)| *keys)
        .collect::<Vec<_>>()
        .join(" ");
    for c in 'a'..='z' {
        if !reaction(KeyCode::Char(c), KeyModifiers::CONTROL) {
            continue;
        }
        let upper = c.to_ascii_uppercase();
        let spelled_out = format!("Ctrl-{upper}");
        let grouped = named.split_whitespace().any(|keys| {
            keys.starts_with("Ctrl-") && keys[5..].split('/').any(|k| k == upper.to_string())
        });
        assert!(
            named.contains(&spelled_out) || grouped,
            "{spelled_out} works but isn't in the help"
        );
    }
}
