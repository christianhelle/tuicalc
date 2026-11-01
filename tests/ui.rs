mod common;

use common::*;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::style::Modifier;
use tuicalc::app::App;
use tuicalc::ui::draw;
use tuicalc::xlsx::Workbook;
use unicode_width::UnicodeWidthStr;

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

fn terminal_for(app: &mut App, width: u16, height: u16) -> Terminal<TestBackend> {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|frame| draw(frame, app)).unwrap();
    terminal
}

/// The screen as lines of text.
fn render(app: &mut App, width: u16, height: u16) -> Vec<String> {
    let terminal = terminal_for(app, width, height);
    let buffer = terminal.backend().buffer();
    (0..height)
        .map(|y| {
            let mut line = String::new();
            let mut x = 0;
            while x < width {
                let symbol = buffer[(x, y)].symbol();
                line.push_str(symbol);
                // Wide characters occupy the next cell too, which holds a filler.
                x += symbol.width().max(1) as u16;
            }
            line
        })
        .collect()
}

/// The screen line holding the given sheet row, 1-based, with the formula bar and headers above.
fn grid_line(screen: &[String], row: usize) -> &str {
    &screen[row + 1]
}

/// One cell's text as drawn in the grid, for a zero-based column.
fn drawn_cell(line: &str, col: usize) -> String {
    line.chars().skip(8 + col * 12).take(11).collect()
}

#[test]
fn draws_column_and_row_headers_with_aligned_values() {
    let (_file, mut app) = app_with(
        r#"<row r="2"><c r="A2" t="inlineStr"><is><t>name</t></is></c><c r="B2"><v>42</v></c></row>"#,
    );

    let screen = render(&mut app, 44, 10);

    assert_eq!(screen[1].trim_end(), "             A           B           C");
    assert_eq!(grid_line(&screen, 1).trim_end(), "1");
    let row = grid_line(&screen, 2);
    assert!(row.starts_with("2       "), "{row}");
    assert_eq!(drawn_cell(row, 0), "name       ");
    assert_eq!(drawn_cell(row, 1), "         42");
}
