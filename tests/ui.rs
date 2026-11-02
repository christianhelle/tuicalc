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

/// The cursor and highlighted dialog lines follow the terminal's theme, as reversed video.
fn is_selector(cell: &ratatui::buffer::Cell) -> bool {
    cell.modifier.contains(Modifier::REVERSED)
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

/// A line of the grid without the box drawn around it.
fn inside_box(line: &str) -> &str {
    let line = line.strip_prefix(['┃', '│']).unwrap_or(line);
    line.strip_suffix(['┃', '│']).unwrap_or(line)
}

/// The grid line holding the given visible row, 1-based, without the box around it.
fn grid_line(screen: &[String], row: usize) -> &str {
    inside_box(&screen[cell_position(0, 0).1 as usize + row - 1])
}

fn header_y() -> u16 {
    cell_position(0, 0).1 - 2
}

/// Where a one-digit row number is drawn: at the right of its seven cells.
fn row_number_x() -> u16 {
    7
}

/// A grid line's row number.
fn row_label(line: &str) -> String {
    line.chars().take(7).collect::<String>().trim().to_string()
}

fn formula_line(screen: &[String]) -> &str {
    inside_box(&screen[1])
}

fn header_line(screen: &[String]) -> &str {
    inside_box(&screen[header_y() as usize])
}

fn tabs_line(screen: &[String]) -> &str {
    &screen[screen.len() - 2]
}

fn status_line(screen: &[String]) -> &str {
    &screen[screen.len() - 1]
}

/// Where a cell's text starts on screen, for a visible column and row counted from zero.
fn cell_position(col: u16, row: u16) -> (u16, u16) {
    (9 + col * 12, 6 + row)
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

    let screen = render(&mut app, 46, 14);

    assert_eq!(
        header_line(&screen),
        "       │     A     │     B     │     C     │"
    );
    assert_eq!(
        inside_box(&screen[header_y() as usize + 1]),
        "───────┼───────────┼───────────┼───────────┤"
    );
    assert_eq!(
        grid_line(&screen, 1),
        "      1│           │           │           │"
    );
    assert_eq!(
        grid_line(&screen, 2),
        "      2│name       │         42│           │"
    );
}

#[test]
fn numbers_show_as_many_significant_digits_as_fit() {
    let (_file, mut app) = app_with(
        r#"<row r="1"><c r="A1"><v>0.30000000000000004</v></c><c r="B1"><v>-0.333333333333333</v></c><c r="C1"><v>1E+20</v></c></row><row r="2"><c r="A2"><v>123456</v></c><c r="B2"><v>0.00001</v></c><c r="C2"><v>12345678901234</v></c></row>"#,
    );

    let screen = render(&mut app, 46, 14);

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

    let screen = render(&mut app, 46, 14);

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

    let screen = render(&mut app, 40, 14);

    assert_eq!(drawn_cell(grid_line(&screen, 1), 0), "far too lon");
    assert_eq!(drawn_cell(grid_line(&screen, 1), 1).trim(), "1");
    let row2 = grid_line(&screen, 2);
    assert!(row2.contains("日本語日本"), "{row2}");
    assert!(!row2.contains("日本語日本語"), "{row2}");
    assert_eq!(row2.split('│').nth(2).unwrap().trim(), "2", "{row2}");
    assert_eq!(drawn_cell(grid_line(&screen, 3), 0), "two lines  ");
}

#[test]
fn highlights_the_cursor_cell_and_scrolls_with_it() {
    let (_file, mut app) = app_with(r#"<row r="1"><c r="A1"><v>1</v></c></row>"#);

    let terminal = terminal_for(&mut app, 40, 15);
    let buffer = terminal.backend().buffer();
    assert!(is_selector(&buffer[cell_position(0, 0)]));
    assert!(!is_selector(&buffer[cell_position(1, 0)]));

    // Six rows fit between the headers and the bottom two lines.
    for _ in 0..9 {
        press(&mut app, KeyCode::Down);
    }
    let screen = render(&mut app, 40, 15);
    assert_eq!(row_label(grid_line(&screen, 1)), "5");
    assert_eq!(row_label(grid_line(&screen, 6)), "10");
    let terminal = terminal_for(&mut app, 40, 15);
    assert!(is_selector(
        &terminal.backend().buffer()[cell_position(0, 5)]
    ));

    for _ in 0..4 {
        press(&mut app, KeyCode::Right);
    }
    let screen = render(&mut app, 40, 15);
    assert_eq!(
        header_line(&screen).trim_end(),
        "       │     D     │     E     │"
    );
}

#[test]
fn the_formula_bar_shows_the_address_and_full_contents() {
    let (_file, mut app) = app_with(
        r#"<row r="1"><c r="A1"><f>SUM(B1:B9)</f><v>3</v></c><c r="B1" t="inlineStr"><is><t>far too long for the column</t></is></c></row>"#,
    );

    let screen = render(&mut app, 50, 14);
    assert!(screen[0].starts_with("┌ A1 ─"), "{}", screen[0]);
    assert_eq!(formula_line(&screen).trim_end(), "=SUM(B1:B9)");
    assert!(screen[2].starts_with('└'), "{}", screen[2]);

    press(&mut app, KeyCode::Right);
    let screen = render(&mut app, 50, 14);
    assert!(screen[0].starts_with("┌ B1 ─"), "{}", screen[0]);
    assert_eq!(
        formula_line(&screen).trim_end(),
        "far too long for the column"
    );
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

    assert!(terminal.backend().buffer()[(1, 1)].symbol() == "日");
    assert_eq!(
        terminal.get_cursor_position().unwrap().x,
        1 + 4,
        "wide characters take two cells"
    );
    assert_eq!(terminal.get_cursor_position().unwrap().y, 1);
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
    assert!(position.x < 29, "caret stays in the box, at {}", position.x);
    let buffer = terminal.backend().buffer();
    let line: String = (1..29)
        .map(|x| buffer[(x, 1)].symbol().to_string())
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

    let screen = render(&mut app, 30, 14);
    assert!(
        tabs_line(&screen).starts_with(" Sheet0  Sheet1 "),
        "{}",
        tabs_line(&screen)
    );

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

    let screen = render(&mut app, 80, 14);
    assert!(
        status_line(&screen).starts_with(" NORMAL  i edit · "),
        "{}",
        status_line(&screen)
    );

    press(&mut app, KeyCode::Char('s'));
    press(&mut app, KeyCode::Char('1'));
    let screen = render(&mut app, 80, 14);
    assert!(
        status_line(&screen).starts_with(" INSERT  Esc done · "),
        "{}",
        status_line(&screen)
    );

    press(&mut app, KeyCode::Enter);
    let screen = render(&mut app, 80, 14);
    assert!(
        status_line(&screen).starts_with(" NORMAL "),
        "{}",
        status_line(&screen)
    );

    app.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL));
    let screen = render(&mut app, 80, 14);
    assert!(
        status_line(&screen).starts_with(" NORMAL  Saved book.xlsx"),
        "{}",
        status_line(&screen)
    );

    press(&mut app, KeyCode::Up);
    press(&mut app, KeyCode::Char('s'));
    press(&mut app, KeyCode::Char('2'));
    press(&mut app, KeyCode::Enter);
    app.handle_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::CONTROL));
    let screen = render(&mut app, 80, 14);
    assert!(
        status_line(&screen).contains("[s]ave and quit"),
        "{}",
        status_line(&screen)
    );
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

#[test]
fn the_status_line_names_the_mode() {
    let (_file, mut app) = app_with("");
    press(&mut app, KeyCode::Char('v'));
    let screen = render(&mut app, 80, 14);
    assert!(
        status_line(&screen).starts_with(" VISUAL  d delete · "),
        "{}",
        status_line(&screen)
    );

    press(&mut app, KeyCode::Char('V'));
    let screen = render(&mut app, 80, 14);
    assert!(
        status_line(&screen).starts_with(" V-LINE "),
        "{}",
        status_line(&screen)
    );
}

#[test]
fn the_command_line_takes_over_the_status_line() {
    let (_file, mut app) = app_with("");
    press(&mut app, KeyCode::Char(':'));
    press(&mut app, KeyCode::Char('w'));
    press(&mut app, KeyCode::Char('q'));

    let mut terminal = terminal_for(&mut app, 40, 10);

    let buffer = terminal.backend().buffer();
    let line: String = (0..40)
        .map(|x| buffer[(x, 9)].symbol().to_string())
        .collect();
    assert_eq!(line.trim_end(), ":wq");
    let position = terminal.get_cursor_position().unwrap();
    assert_eq!((position.x, position.y), (3, 9));
}

#[test]
fn the_selection_is_highlighted_with_the_cursor_marked_inside_it() {
    let (_file, mut app) = app_with("");
    press(&mut app, KeyCode::Char('v'));
    press(&mut app, KeyCode::Char('j'));
    press(&mut app, KeyCode::Char('l'));

    let terminal = terminal_for(&mut app, 44, 12);
    let buffer = terminal.backend().buffer();

    let at = |col: u16, row: u16| &buffer[cell_position(col, row)];
    assert!(at(0, 0).modifier.contains(Modifier::REVERSED));
    assert!(at(1, 0).modifier.contains(Modifier::REVERSED));
    assert!(at(0, 1).modifier.contains(Modifier::REVERSED));
    assert!(at(1, 1).modifier.contains(Modifier::REVERSED), "the cursor");
    assert!(at(1, 1).modifier.contains(Modifier::BOLD), "the cursor");
    assert_eq!(at(1, 1).fg, ratatui::style::Color::Yellow, "the cursor");
    assert_ne!(at(0, 0).fg, ratatui::style::Color::Yellow);
    assert!(!at(2, 0).modifier.contains(Modifier::REVERSED));
    assert!(!at(0, 2).modifier.contains(Modifier::REVERSED));
}

#[test]
fn unfinished_commands_show_at_the_right_of_the_status_line() {
    let (_file, mut app) = app_with("");
    for c in "2d3".chars() {
        press(&mut app, KeyCode::Char(c));
    }
    let screen = render(&mut app, 100, 14);
    assert!(
        status_line(&screen).trim_end().ends_with(" 2d3"),
        "{}",
        status_line(&screen)
    );

    press(&mut app, KeyCode::Char('g'));
    let screen = render(&mut app, 100, 14);
    assert!(
        status_line(&screen).trim_end().ends_with(" 6dg"),
        "{}",
        status_line(&screen)
    );

    press(&mut app, KeyCode::Esc);
    let screen = render(&mut app, 100, 14);
    assert!(
        !status_line(&screen).trim_end().ends_with('g'),
        "{}",
        status_line(&screen)
    );
}

#[test]
fn column_and_row_headers_are_bold_cyan_on_the_terminal_background() {
    use ratatui::style::Color;

    let (_file, mut app) = app_with(r#"<row r="1"><c r="B1"><v>1</v></c></row>"#);
    press(&mut app, KeyCode::Char('j'));
    press(&mut app, KeyCode::Char('l'));
    press(&mut app, KeyCode::Char('l'));

    let terminal = terminal_for(&mut app, 50, 16);
    let buffer = terminal.backend().buffer();
    let column_letter = &buffer[(cell_position(0, 0).0 + 5, header_y())];
    let row_number = &buffer[(row_number_x(), cell_position(0, 0).1)];

    assert_eq!(column_letter.symbol(), "A");
    assert_eq!(column_letter.fg, Color::Cyan);
    assert_eq!(column_letter.bg, Color::Reset);
    assert_eq!(row_number.symbol(), "1");
    assert_eq!(row_number.fg, Color::Cyan);
    assert_eq!(row_number.bg, Color::Reset);
}

#[test]
fn the_grid_sits_in_a_box_titled_with_the_file_and_sheet() {
    let (_file, mut app) = app_with("");

    let screen = render(&mut app, 46, 14);

    assert!(
        screen[3].starts_with("┏ book.xlsx — Sheet1 ━"),
        "{}",
        screen[3]
    );
    assert!(screen[3].ends_with('┓'), "{}", screen[3]);
    assert!(screen[11].starts_with('┗'), "{}", screen[11]);
    assert!(screen[11].ends_with('┛'), "{}", screen[11]);
    assert!(screen[5].starts_with('┃'), "{}", screen[5]);
    assert!(screen[5].ends_with('┃'), "{}", screen[5]);
}

#[test]
fn the_grid_title_marks_unsaved_changes_and_unnamed_workbooks() {
    let (_file, mut app) = app_with("");
    press(&mut app, KeyCode::Char('s'));
    press(&mut app, KeyCode::Char('1'));
    press(&mut app, KeyCode::Enter);
    let screen = render(&mut app, 46, 14);
    assert!(screen[3].contains(" book.xlsx* — Sheet1 "), "{}", screen[3]);

    let mut blank = App::new(Workbook::blank().unwrap());
    let screen = render(&mut blank, 46, 14);
    assert!(screen[3].contains(" [No Name] — "), "{}", screen[3]);
}

#[test]
fn the_thick_border_moves_to_the_formula_bar_while_editing() {
    let (_file, mut app) = app_with("");

    let screen = render(&mut app, 46, 14);
    assert!(screen[0].starts_with('┌'), "{}", screen[0]);
    assert!(screen[3].starts_with('┏'), "{}", screen[3]);

    press(&mut app, KeyCode::Char('s'));
    let screen = render(&mut app, 46, 14);
    assert!(screen[0].starts_with('┏'), "{}", screen[0]);
    assert!(screen[3].starts_with('┌'), "{}", screen[3]);
}

#[test]
fn the_cursors_column_and_row_headers_stand_out_in_yellow() {
    use ratatui::style::Color;

    let (_file, mut app) = app_with("");
    press(&mut app, KeyCode::Char('l'));
    press(&mut app, KeyCode::Char('j'));

    let terminal = terminal_for(&mut app, 50, 16);
    let buffer = terminal.backend().buffer();
    let letter_x = |col: u16| cell_position(col, 0).0 + 5;
    let number_y = |row: u16| cell_position(0, row).1;

    assert_eq!(buffer[(letter_x(1), header_y())].fg, Color::Yellow);
    assert_eq!(buffer[(letter_x(0), header_y())].fg, Color::Cyan);
    assert_eq!(buffer[(row_number_x(), number_y(1))].fg, Color::Yellow);
    assert_eq!(buffer[(row_number_x(), number_y(0))].fg, Color::Cyan);
}

#[test]
fn errors_on_the_status_line_are_red_and_other_messages_are_not() {
    use ratatui::style::Color;

    let (file, mut app) = app_with("");
    let message_colour = |app: &mut App| {
        let terminal = terminal_for(app, 60, 14);
        terminal.backend().buffer()[(10, 13)].fg
    };

    app.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL));
    assert_ne!(message_colour(&mut app), Color::Red, "a save that worked");

    press(&mut app, KeyCode::Char('s'));
    press(&mut app, KeyCode::Char('1'));
    press(&mut app, KeyCode::Enter);
    std::fs::write(&file.path, b"changed by something else").unwrap();
    app.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL));
    assert_eq!(message_colour(&mut app), Color::Red, "a save that failed");

    press(&mut app, KeyCode::Char('j'));
    assert_ne!(message_colour(&mut app), Color::Red, "the hints again");
}

#[test]
fn the_status_line_badge_is_black_on_the_modes_colour() {
    use ratatui::style::Color;

    let (_file, mut app) = app_with("");
    let badge_of = |app: &mut App| {
        let terminal = terminal_for(app, 60, 14);
        let cell = &terminal.backend().buffer()[(1, 13)];
        (cell.fg, cell.bg)
    };

    assert_eq!(badge_of(&mut app), (Color::Black, Color::Blue));
    press(&mut app, KeyCode::Char('s'));
    assert_eq!(badge_of(&mut app), (Color::Black, Color::Green));
    press(&mut app, KeyCode::Esc);
    press(&mut app, KeyCode::Char('v'));
    assert_eq!(badge_of(&mut app), (Color::Black, Color::Magenta));
    press(&mut app, KeyCode::Char('V'));
    assert_eq!(badge_of(&mut app), (Color::Black, Color::Magenta));
}

/// A blank workbook with its save dialog open in `dir`.
fn app_in_save_dialog(dir: &std::path::Path) -> App {
    let mut app = App::new(Workbook::blank().unwrap());
    app.set_dialog_directory(dir);
    app.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL));
    app
}

#[test]
fn the_save_dialog_shows_the_folder_list_and_name() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("reports")).unwrap();
    std::fs::write(dir.path().join("old.xlsx"), b"x").unwrap();
    let mut app = app_in_save_dialog(dir.path());
    press(&mut app, KeyCode::Char('b'));

    let text = render(&mut app, 100, 20).join("\n");

    assert!(text.contains("Save as"), "{text}");
    assert!(text.contains(&dir.path().display().to_string()), "{text}");
    assert!(text.contains("  .."), "{text}");
    assert!(text.contains("  reports"), "{text}");
    assert!(text.contains("  old.xlsx"), "{text}");
    assert!(text.contains("Name: b"), "{text}");
    assert!(text.contains("Enter save"), "{text}");
}

#[test]
fn the_save_dialog_marks_the_highlighted_line() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("old.xlsx"), b"x").unwrap();
    let mut app = app_in_save_dialog(dir.path());
    press(&mut app, KeyCode::Up);

    let screen = render(&mut app, 100, 20);
    let y = screen
        .iter()
        .position(|line| line.contains("> old.xlsx"))
        .unwrap();
    let x = screen[y].find("> old.xlsx").unwrap() as u16;

    let terminal = terminal_for(&mut app, 100, 20);
    assert!(is_selector(&terminal.backend().buffer()[(x, y as u16)]));
}

#[test]
fn the_save_dialog_list_scrolls_to_keep_the_highlight_visible() {
    let dir = tempfile::tempdir().unwrap();
    for number in 0..40 {
        std::fs::write(dir.path().join(format!("file{number:02}.xlsx")), b"x").unwrap();
    }
    let mut app = app_in_save_dialog(dir.path());
    press(&mut app, KeyCode::Up);

    let text = render(&mut app, 100, 30).join("\n");

    assert!(text.contains("> file39.xlsx"), "{text}");
    assert!(!text.contains("file00.xlsx"), "{text}");
}

#[test]
fn the_save_dialog_shows_messages_and_the_overwrite_question() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("old.xlsx"), b"x").unwrap();
    let mut app = app_in_save_dialog(dir.path());

    press(&mut app, KeyCode::Enter);
    let text = render(&mut app, 100, 20).join("\n");
    assert!(text.contains("Enter a file name"), "{text}");

    press(&mut app, KeyCode::Up);
    press(&mut app, KeyCode::Enter);
    let text = render(&mut app, 100, 20).join("\n");
    assert!(text.contains("old.xlsx exists. Overwrite? [y/n]"), "{text}");
}

#[test]
fn the_save_dialog_keeps_the_end_of_a_long_folder_path() {
    let dir = tempfile::tempdir().unwrap();
    let deep = dir.path().join("a".repeat(70)).join("the-last-folder");
    std::fs::create_dir_all(&deep).unwrap();
    let mut app = app_in_save_dialog(&deep);

    let text = render(&mut app, 60, 20).join("\n");

    assert!(text.contains("the-last-folder"), "{text}");
}

#[test]
fn the_save_dialog_name_stays_visible_in_a_short_terminal() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = app_in_save_dialog(dir.path());
    press(&mut app, KeyCode::Char('b'));

    let text = render(&mut app, 40, 3).join("\n");

    assert!(text.contains("Name: b"), "{text}");
}

#[test]
fn the_save_dialog_survives_any_terminal_size() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("old.xlsx"), b"x").unwrap();
    let mut app = app_in_save_dialog(dir.path());
    press(&mut app, KeyCode::Up);
    for width in 1..30 {
        for height in 1..12 {
            render(&mut app, width, height);
        }
    }
}

#[test]
fn column_and_row_headers_are_bold_but_the_cursor_is_not() {
    let (_file, mut app) = app_with(r#"<row r="1"><c r="B1"><v>1</v></c></row>"#);

    let terminal = terminal_for(&mut app, 50, 16);
    let buffer = terminal.backend().buffer();
    let is_bold = |x: u16, y: u16| buffer[(x, y)].modifier.contains(Modifier::BOLD);

    let letter_x = cell_position(0, 0).0 + 5;
    let number_y = cell_position(0, 0).1;
    assert_eq!(buffer[(letter_x, header_y())].symbol(), "A");
    assert!(is_bold(letter_x, header_y()), "a column letter");
    assert_eq!(buffer[(row_number_x(), number_y)].symbol(), "1");
    assert!(is_bold(row_number_x(), number_y), "a row number");
    assert!(is_selector(&buffer[cell_position(0, 0)]));
    let (x, y) = cell_position(0, 0);
    assert!(!is_bold(x, y), "the cursor");
    let (x, y) = cell_position(1, 0);
    assert!(!is_bold(x + 10, y), "cell contents");
}

/// A blank workbook with the open dialog on `dir`.
fn app_in_open_dialog(dir: &std::path::Path) -> App {
    let mut app = App::new(Workbook::blank().unwrap());
    app.set_dialog_directory(dir);
    app.handle_key(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::CONTROL));
    app
}

#[test]
fn the_open_dialog_is_labelled_for_opening() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = app_in_open_dialog(dir.path());

    let screen = render(&mut app, 100, 20);
    let text = screen.join("\n");

    assert!(text.contains(" Open "), "{text}");
    assert!(!text.contains("Save as"), "{text}");
    assert!(text.contains("Enter open"), "{text}");
    assert!(
        status_line(&screen).starts_with(" OPEN "),
        "{}",
        status_line(&screen)
    );
}
