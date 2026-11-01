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

#[test]
fn numbers_show_as_many_significant_digits_as_fit() {
    let (_file, mut app) = app_with(
        r#"<row r="1"><c r="A1"><v>0.30000000000000004</v></c><c r="B1"><v>-0.333333333333333</v></c><c r="C1"><v>1E+20</v></c></row><row r="2"><c r="A2"><v>123456</v></c><c r="B2"><v>0.00001</v></c><c r="C2"><v>12345678901234</v></c></row>"#,
    );

    let screen = render(&mut app, 44, 10);

    let row = grid_line(&screen, 1);
    assert_eq!(drawn_cell(row, 0).trim(), "0.3");
    assert_eq!(drawn_cell(row, 1).trim(), "-0.33333333");
    assert_eq!(drawn_cell(row, 2).trim(), "1e20");
    let row = grid_line(&screen, 2);
    assert_eq!(drawn_cell(row, 0).trim(), "123456");
    assert_eq!(drawn_cell(row, 1).trim(), "1e-5");
    assert_eq!(drawn_cell(row, 2).trim(), "1.234568e13");
}

#[test]
fn shows_booleans_errors_formula_results_and_dates() {
    let styles = r#"<styleSheet><cellXfs count="2"><xf numFmtId="0"/><xf numFmtId="22"/></cellXfs></styleSheet>"#;
    let rows = r##"<row r="1"><c r="A1" t="b"><v>1</v></c><c r="B1" t="e"><v>#REF!</v></c><c r="C1"><f>B2*2</f><v>8</v></c></row><row r="2"><c r="A2"><f>NEW()</f></c><c r="B2" s="1"><v>45000</v></c><c r="C2" s="1"><v>45000.5</v></c></row>"##;
    let (_file, mut app) = app_from(&single_sheet(&sheet(rows), &[("xl/styles.xml", styles)]));

    let screen = render(&mut app, 44, 10);

    let row = grid_line(&screen, 1);
    assert_eq!(drawn_cell(row, 0).trim(), "TRUE");
    assert_eq!(drawn_cell(row, 1).trim(), "#REF!");
    assert_eq!(drawn_cell(row, 2).trim(), "8");
    let row = grid_line(&screen, 2);
    assert_eq!(drawn_cell(row, 0).trim(), "=NEW()");
    assert_eq!(drawn_cell(row, 1).trim(), "2023-03-15");
    assert_eq!(drawn_cell(row, 2).trim(), "2023-03-15", "the time does not fit the column");
}
