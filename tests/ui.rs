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

    assert_eq!(
        screen[1].trim_end(),
        "             A           B           C"
    );
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
    assert_eq!(
        drawn_cell(row, 2).trim(),
        "2023-03-15",
        "the time does not fit the column"
    );
}

#[test]
fn long_and_wide_text_stays_inside_its_column() {
    let (_file, mut app) = app_with(
        r#"<row r="1"><c r="A1" t="inlineStr"><is><t>far too long for the column</t></is></c><c r="B1"><v>1</v></c></row><row r="2"><c r="A2" t="inlineStr"><is><t>日本語日本語日本語</t></is></c><c r="B2"><v>2</v></c></row><row r="3"><c r="A3" t="inlineStr"><is><t xml:space="preserve">two
lines</t></is></c></row>"#,
    );

    let screen = render(&mut app, 40, 10);

    assert_eq!(drawn_cell(grid_line(&screen, 1), 0), "far too lon");
    assert_eq!(drawn_cell(grid_line(&screen, 1), 1).trim(), "1");
    let row2 = grid_line(&screen, 2);
    assert!(row2.contains("日本語日本"), "{row2}");
    assert!(!row2.contains("日本語日本語"), "{row2}");
    assert!(row2.trim_end().ends_with('2'), "{row2}");
    assert_eq!(drawn_cell(grid_line(&screen, 3), 0), "two lines  ");
}

#[test]
fn highlights_the_cursor_cell_and_scrolls_with_it() {
    let (_file, mut app) = app_with(r#"<row r="1"><c r="A1"><v>1</v></c></row>"#);

    let terminal = terminal_for(&mut app, 40, 10);
    let buffer = terminal.backend().buffer();
    assert!(buffer[(8, 2)].modifier.contains(Modifier::REVERSED));
    assert!(!buffer[(20, 2)].modifier.contains(Modifier::REVERSED));

    // Six rows fit between the headers and the bottom two lines.
    for _ in 0..9 {
        press(&mut app, KeyCode::Down);
    }
    let screen = render(&mut app, 40, 10);
    assert!(
        grid_line(&screen, 1).starts_with("5 "),
        "{}",
        grid_line(&screen, 1)
    );
    assert!(
        grid_line(&screen, 6).starts_with("10 "),
        "{}",
        grid_line(&screen, 6)
    );
    let terminal = terminal_for(&mut app, 40, 10);
    assert!(
        terminal.backend().buffer()[(8, 7)]
            .modifier
            .contains(Modifier::REVERSED)
    );

    for _ in 0..4 {
        press(&mut app, KeyCode::Right);
    }
    let screen = render(&mut app, 40, 10);
    assert_eq!(screen[1].trim_end(), "             D           E");
}

#[test]
fn the_formula_bar_shows_the_address_and_full_contents() {
    let (_file, mut app) = app_with(
        r#"<row r="1"><c r="A1"><f>SUM(B1:B9)</f><v>3</v></c><c r="B1" t="inlineStr"><is><t>far too long for the column</t></is></c></row>"#,
    );

    let screen = render(&mut app, 50, 10);
    assert_eq!(screen[0].trim_end(), "A1      =SUM(B1:B9)");

    press(&mut app, KeyCode::Right);
    let screen = render(&mut app, 50, 10);
    assert_eq!(screen[0].trim_end(), "B1      far too long for the column");
}

#[test]
fn editing_shows_the_buffer_with_the_terminal_cursor_at_the_caret() {
    let (_file, mut app) = app_with("");
    press(&mut app, KeyCode::Char('s'));
    for c in "日本x".chars() {
        press(&mut app, KeyCode::Char(c));
    }
    press(&mut app, KeyCode::Left);

    let mut terminal = terminal_for(&mut app, 50, 10);

    assert!(terminal.backend().buffer()[(8, 0)].symbol() == "日");
    assert_eq!(
        terminal.get_cursor_position().unwrap().x,
        8 + 4,
        "wide characters take two cells"
    );
    assert_eq!(terminal.get_cursor_position().unwrap().y, 0);
}

#[test]
fn long_edits_scroll_to_keep_the_caret_visible() {
    let (_file, mut app) = app_with("");
    let text: String = ('a'..='z').cycle().take(60).collect();
    press(&mut app, KeyCode::Char('s'));
    for c in text.chars() {
        press(&mut app, KeyCode::Char(c));
    }

    let mut terminal = terminal_for(&mut app, 30, 10);

    let position = terminal.get_cursor_position().unwrap();
    assert!(position.x < 30, "caret stays on screen, at {}", position.x);
    let buffer = terminal.backend().buffer();
    let line: String = (8..30)
        .map(|x| buffer[(x, 0)].symbol().to_string())
        .collect();
    assert!(text.ends_with(line.trim_end()), "{line}");
}

fn many_sheets(count: usize) -> Vec<u8> {
    let tags: String = (0..count)
        .map(|i| format!(r#"<sheet name="Sheet{i}" sheetId="{i}" r:id="rId{i}"/>"#))
        .collect();
    let workbook = format!(r#"<workbook xmlns:r="x"><sheets>{tags}</sheets></workbook>"#);
    let rels: String = (0..count)
        .map(|i| {
            format!(
                r#"<Relationship Id="rId{i}" Type="x/worksheet" Target="worksheets/s{i}.xml"/>"#
            )
        })
        .collect();
    let rels = format!("<Relationships>{rels}</Relationships>");
    let sheet_xml = sheet("");
    let names: Vec<String> = (0..count)
        .map(|i| format!("xl/worksheets/s{i}.xml"))
        .collect();
    let mut parts = vec![
        ("_rels/.rels", ROOT_RELS),
        ("xl/workbook.xml", workbook.as_str()),
        ("xl/_rels/workbook.xml.rels", rels.as_str()),
    ];
    parts.extend(names.iter().map(|name| (name.as_str(), sheet_xml.as_str())));
    package(&parts)
}

#[test]
fn tabs_mark_the_current_sheet_and_keep_it_on_screen() {
    let (_file, mut app) = app_from(&many_sheets(10));

    let screen = render(&mut app, 30, 10);
    assert!(screen[8].starts_with(" Sheet0  Sheet1 "), "{}", screen[8]);

    for _ in 0..9 {
        press(&mut app, KeyCode::F(6));
    }
    let terminal = terminal_for(&mut app, 30, 10);
    let buffer = terminal.backend().buffer();
    let line: String = (0..30)
        .map(|x| buffer[(x, 8)].symbol().to_string())
        .collect();
    assert!(line.contains(" Sheet9 "), "{line}");
    let start = line.find(" Sheet9 ").unwrap() as u16;
    assert!(buffer[(start + 1, 8)].modifier.contains(Modifier::REVERSED));
}

#[test]
fn the_status_line_shows_file_state_help_messages_and_prompts() {
    let (_file, mut app) = app_with("");

    let screen = render(&mut app, 80, 10);
    assert!(
        screen[9].starts_with("book.xlsx | F2 edit"),
        "{}",
        screen[9]
    );

    press(&mut app, KeyCode::Char('s'));
    press(&mut app, KeyCode::Char('1'));
    let screen = render(&mut app, 80, 10);
    assert!(screen[9].contains("Enter confirm"), "{}", screen[9]);

    press(&mut app, KeyCode::Enter);
    let screen = render(&mut app, 80, 10);
    assert!(screen[9].starts_with("book.xlsx* |"), "{}", screen[9]);

    app.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL));
    let screen = render(&mut app, 80, 10);
    assert!(
        screen[9].starts_with("book.xlsx | Saved book.xlsx"),
        "{}",
        screen[9]
    );

    press(&mut app, KeyCode::Up);
    press(&mut app, KeyCode::Char('s'));
    press(&mut app, KeyCode::Char('2'));
    press(&mut app, KeyCode::Enter);
    app.handle_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::CONTROL));
    let screen = render(&mut app, 80, 10);
    assert!(screen[9].contains("[s]ave and quit"), "{}", screen[9]);
}

#[test]
fn draws_at_any_terminal_size_without_panicking() {
    let (_file, mut app) = app_with(r#"<row r="1"><c r="A1"><v>1</v></c></row>"#);
    press(&mut app, KeyCode::Char('s'));
    for width in 1..30 {
        for height in 1..8 {
            render(&mut app, width, height);
        }
    }
}
