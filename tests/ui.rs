mod common;

use common::*;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use tuicalc::address::CellRef;
use tuicalc::app::{App, Mode};
use tuicalc::ui::{draw, fit, handle_mouse};
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
    fit(app, Rect::new(0, 0, width, height));
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
        status_line(&screen).starts_with(" NORMAL  ? help · "),
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
    app.finish_saving();
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
}

/// A workbook with unsaved edits, with the given key pressed in normal mode.
fn app_asking_about_edits(file: &TempXlsx, key: KeyEvent) -> App {
    let mut app = App::new(Workbook::open(&file.path).unwrap());
    press(&mut app, KeyCode::Char('s'));
    press(&mut app, KeyCode::Char('2'));
    press(&mut app, KeyCode::Enter);
    app.handle_key(key);
    app
}

#[test]
fn unsaved_edits_are_asked_about_in_a_red_dialog() {
    use ratatui::style::Color;

    let file = write_temp(&single_sheet(&sheet(""), &[]));
    let mut app = app_asking_about_edits(&file, KeyEvent::from(KeyCode::Char('q')));

    let screen = render(&mut app, 80, 14);
    let text = screen.join(
        "
",
    );
    assert!(text.contains("Unsaved changes"), "{text}");
    assert!(text.contains("book.xlsx has unsaved changes"), "{text}");
    assert!(text.contains("s save and quit"), "{text}");
    assert!(text.contains("d discard and quit"), "{text}");
    assert!(text.contains("Esc stay"), "{text}");

    let top = screen
        .iter()
        .position(|line| line.contains("Unsaved"))
        .unwrap();
    let left = screen[top].chars().position(|c| c == '┌').unwrap() as u16;
    let terminal = terminal_for(&mut app, 80, 14);
    assert_eq!(
        terminal.backend().buffer()[(left, top as u16)].fg,
        Color::Red
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
    app.finish_saving();
    assert_ne!(message_colour(&mut app), Color::Red, "a save that worked");

    press(&mut app, KeyCode::Char('s'));
    press(&mut app, KeyCode::Char('1'));
    press(&mut app, KeyCode::Enter);
    std::fs::write(&file.path, b"changed by something else").unwrap();
    app.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL));
    app.finish_saving();
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
    // A long path keeps only its end, as it does in the macOS temporary folder.
    let folder = dir.path().file_name().unwrap().to_string_lossy();
    assert!(text.contains(folder.as_ref()), "{text}");
    assert!(text.contains("  .."), "{text}");
    assert!(text.contains("  reports"), "{text}");
    assert!(text.contains("  old.xlsx"), "{text}");
    assert!(text.contains("Name: b"), "{text}");
    assert!(text.contains("Enter save"), "{text}");
}

#[test]
fn the_file_dialog_has_a_yellow_border_and_coloured_key_hints() {
    use ratatui::style::Color;

    let dir = tempfile::tempdir().unwrap();
    let mut app = app_in_save_dialog(dir.path());

    let screen = render(&mut app, 100, 30);
    let top = screen
        .iter()
        .position(|line| line.contains("Save as"))
        .unwrap();
    let left = screen[top].chars().position(|c| c == '┌').unwrap() as u16;
    let hint = screen
        .iter()
        .position(|line| line.contains("Enter save"))
        .unwrap();
    let enter = screen[hint].chars().position(|c| c == 'E').unwrap() as u16;
    let esc = screen[hint].find("Esc").unwrap();
    let esc = screen[hint][..esc].chars().count() as u16;

    let terminal = terminal_for(&mut app, 100, 30);
    let buffer = terminal.backend().buffer();
    assert_eq!(buffer[(left, top as u16)].fg, Color::Yellow);
    assert_eq!(buffer[(enter, hint as u16)].fg, Color::Yellow);
    assert_eq!(buffer[(esc, hint as u16)].fg, Color::Cyan);
}

#[test]
fn go_to_is_a_yellow_dialog_with_the_caret_after_the_text() {
    use ratatui::style::Color;

    let (_file, mut app) = app_with("");
    app.handle_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::CONTROL));
    press(&mut app, KeyCode::Char('C'));
    press(&mut app, KeyCode::Char('7'));

    let screen = render(&mut app, 80, 14);
    let top = screen
        .iter()
        .position(|line| line.contains(" Go to "))
        .unwrap();
    let left = screen[top].chars().position(|c| c == '┌').unwrap() as u16;
    let label = "Cell or row: C7";
    let row = top + 1;
    assert!(screen[row].contains(label), "{}", screen[row]);
    assert!(screen[top + 2].contains("Enter go"), "{}", screen[top + 2]);

    let mut terminal = terminal_for(&mut app, 80, 14);
    assert_eq!(
        terminal.backend().buffer()[(left, top as u16)].fg,
        Color::Yellow
    );
    let caret = terminal.get_cursor_position().unwrap();
    assert_eq!(
        (caret.x, caret.y),
        (left + 1 + label.len() as u16, row as u16)
    );
}

#[test]
fn go_to_shows_its_complaint_in_red_in_the_dialog() {
    use ratatui::style::Color;

    let (_file, mut app) = app_with("");
    app.handle_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::CONTROL));
    press(&mut app, KeyCode::Char('?'));
    press(&mut app, KeyCode::Enter);

    let screen = render(&mut app, 80, 14);
    let row = screen
        .iter()
        .position(|line| line.contains("isn't a cell"))
        .unwrap();
    assert!(!screen[row].contains("Enter go"));
    assert!(!status_line(&screen).contains("isn't a cell"), "shown once");
    let x = screen[row].find("?").unwrap() as u16;
    let terminal = terminal_for(&mut app, 80, 14);
    assert_eq!(terminal.backend().buffer()[(x, row as u16)].fg, Color::Red);
}

#[test]
fn the_search_prompt_takes_over_the_status_line_with_the_caret_after_the_text() {
    let (_file, mut app) = app_with("");
    press(&mut app, KeyCode::Char('/'));
    press(&mut app, KeyCode::Char('a'));
    press(&mut app, KeyCode::Char('b'));

    let screen = render(&mut app, 80, 14);
    assert_eq!(status_line(&screen).trim_end(), " NORMAL  /ab");

    let mut terminal = terminal_for(&mut app, 80, 14);
    let caret = terminal.get_cursor_position().unwrap();
    assert_eq!((caret.x, caret.y), (" NORMAL  /ab".len() as u16, 13));
}

#[test]
fn help_lists_keys_in_cyan_in_a_dialog_titled_keys() {
    use ratatui::style::Color;

    let (_file, mut app) = app_with("");
    press(&mut app, KeyCode::Char('?'));

    let screen = render(&mut app, 90, 24);
    let row = screen
        .iter()
        .position(|line| line.contains("Ctrl-G"))
        .unwrap();
    assert!(
        screen[row].contains("Go to a cell or row"),
        "{}",
        screen[row]
    );
    assert!(screen.join("").contains("Press any key to close"));
    assert!(screen[0].contains("Keys"), "{}", screen[0]);
    let x = screen[row].find("Ctrl-G").unwrap();
    let x = screen[row][..x].chars().count() as u16;

    let terminal = terminal_for(&mut app, 90, 24);
    assert_eq!(terminal.backend().buffer()[(x, row as u16)].fg, Color::Cyan);
}

#[test]
fn the_help_dialog_for_each_mode_looks_right() {
    let (_file, mut app) = app_with("");
    press(&mut app, KeyCode::Char('?'));
    insta::assert_snapshot!("help_in_normal_mode", render(&mut app, 90, 24).join("\n"));
    press(&mut app, KeyCode::Esc);

    press(&mut app, KeyCode::Char('v'));
    press(&mut app, KeyCode::Char('?'));
    insta::assert_snapshot!("help_while_selecting", render(&mut app, 90, 24).join("\n"));
    press(&mut app, KeyCode::Esc);
    press(&mut app, KeyCode::Esc);

    press(&mut app, KeyCode::Char('s'));
    press(&mut app, KeyCode::F(1));
    insta::assert_snapshot!("help_while_editing", render(&mut app, 90, 24).join("\n"));
}

#[test]
fn the_status_line_offers_help_in_every_mode() {
    let (_file, mut app) = app_with("");
    assert!(status_line(&render(&mut app, 100, 14)).contains("? help"));
    press(&mut app, KeyCode::Char('v'));
    assert!(status_line(&render(&mut app, 100, 14)).contains("? help"));
    press(&mut app, KeyCode::Esc);
    press(&mut app, KeyCode::Char('s'));
    assert!(status_line(&render(&mut app, 100, 14)).contains("F1 help"));
}

const MOUSE_SCREEN: (u16, u16) = (46, 16);

fn mouse(app: &mut App, kind: MouseEventKind, (column, row): (u16, u16)) {
    let (width, height) = MOUSE_SCREEN;
    let event = MouseEvent {
        kind,
        column,
        row,
        modifiers: KeyModifiers::NONE,
    };
    handle_mouse(app, Rect::new(0, 0, width, height), event);
}

fn click(app: &mut App, at: (u16, u16)) {
    mouse(app, MouseEventKind::Down(MouseButton::Left), at);
}

fn a_cell(col: u32, row: u32) -> CellRef {
    CellRef { col, row }
}

/// Lets the app learn the grid's size, as the first frame does.
fn draw_once(app: &mut App) {
    render(app, MOUSE_SCREEN.0, MOUSE_SCREEN.1);
}

#[test]
fn clicking_a_cell_moves_the_cursor_there() {
    let (_file, mut app) = app_with("");
    draw_once(&mut app);

    click(&mut app, cell_position(1, 2));

    assert_eq!(app.cursor(), a_cell(1, 2));
}

#[test]
fn clicking_counts_from_the_scrolled_position() {
    let (_file, mut app) = app_with("");
    draw_once(&mut app);
    for _ in 0..30 {
        press(&mut app, KeyCode::Down);
    }
    draw_once(&mut app);
    let top = app.scroll().row;
    assert!(top > 0);

    click(&mut app, cell_position(0, 1));

    assert_eq!(app.cursor(), a_cell(0, top + 1));
}

#[test]
fn clicking_outside_the_cells_does_nothing() {
    let (_file, mut app) = app_with("");
    draw_once(&mut app);
    let (x, y) = cell_position(0, 0);

    for at in [
        (0, y),
        (row_number_x(), y),
        (x, header_y()),
        (x, 1),
        (45, y),
    ] {
        click(&mut app, at);
        assert_eq!(app.cursor(), a_cell(0, 0), "at {at:?}");
    }
}

#[test]
fn clicking_a_tab_switches_sheet_even_when_the_tabs_have_scrolled() {
    let (_file, mut app) = app_from(&many_sheets(10));
    let tabs_y = MOUSE_SCREEN.1 - 2;
    draw_once(&mut app);

    click(&mut app, (10, tabs_y));
    assert_eq!(app.sheet_index(), 1, "the second tab");

    for _ in 0..8 {
        press(&mut app, KeyCode::F(6));
    }
    let screen = render(&mut app, MOUSE_SCREEN.0, MOUSE_SCREEN.1);
    let line = tabs_line(&screen);
    let x = line.find(" Sheet8 ").unwrap() as u16;
    click(&mut app, (x + 1, tabs_y));
    assert_eq!(app.sheet_index(), 8);
}

#[test]
fn clicks_do_nothing_while_editing_or_selecting() {
    let (_file, mut app) = app_with("");
    draw_once(&mut app);

    press(&mut app, KeyCode::Char('s'));
    click(&mut app, cell_position(1, 1));
    assert_eq!(app.mode(), Mode::Insert);
    assert_eq!(app.cursor(), a_cell(0, 0));
    press(&mut app, KeyCode::Esc);

    press(&mut app, KeyCode::Char('v'));
    click(&mut app, cell_position(1, 1));
    assert_eq!(app.mode(), Mode::Visual);
    assert_eq!(app.cursor(), a_cell(0, 0));
}

#[test]
fn a_click_forgets_a_half_typed_command() {
    let (_file, mut app) = app_with("");
    draw_once(&mut app);
    press(&mut app, KeyCode::Char('2'));
    press(&mut app, KeyCode::Char('d'));

    click(&mut app, cell_position(1, 1));

    assert_eq!(app.pending_keys(), "");
    press(&mut app, KeyCode::Char('j'));
    assert_eq!(app.cursor(), a_cell(1, 2), "a plain move, not a delete");
    assert!(!app.document().workbook().is_dirty());
}

#[test]
fn a_click_closes_the_help_without_moving_the_cursor() {
    let (_file, mut app) = app_with("");
    draw_once(&mut app);
    press(&mut app, KeyCode::Char('?'));

    click(&mut app, cell_position(1, 1));

    assert_eq!(app.mode(), Mode::Normal);
    assert_eq!(app.cursor(), a_cell(0, 0));
}

#[test]
fn the_wheel_moves_the_cursor_three_rows_and_stops_at_the_top() {
    let (_file, mut app) = app_with("");
    draw_once(&mut app);

    mouse(&mut app, MouseEventKind::ScrollDown, (0, 0));
    assert_eq!(app.cursor(), a_cell(0, 3));
    mouse(&mut app, MouseEventKind::ScrollUp, (0, 0));
    mouse(&mut app, MouseEventKind::ScrollUp, (0, 0));
    assert_eq!(app.cursor(), a_cell(0, 0));
}

#[test]
fn the_wheel_extends_a_selection_but_leaves_edits_and_help_alone() {
    let (_file, mut app) = app_with("");
    draw_once(&mut app);

    press(&mut app, KeyCode::Char('v'));
    mouse(&mut app, MouseEventKind::ScrollDown, (0, 0));
    assert_eq!(
        app.selection(),
        Some(tuicalc::address::CellRange::between(
            a_cell(0, 0),
            a_cell(0, 3)
        ))
    );
    press(&mut app, KeyCode::Esc);

    press(&mut app, KeyCode::Char('s'));
    mouse(&mut app, MouseEventKind::ScrollDown, (0, 0));
    assert_eq!(app.cursor(), a_cell(0, 3));
    press(&mut app, KeyCode::Esc);

    press(&mut app, KeyCode::Char('?'));
    mouse(&mut app, MouseEventKind::ScrollDown, (0, 0));
    assert_eq!(app.mode(), Mode::Help);
    assert_eq!(app.cursor(), a_cell(0, 3));
}

#[test]
fn the_way_to_quit_fits_on_an_80_column_terminal() {
    let (_file, mut app) = app_with("");

    let screen = render(&mut app, 80, 14);

    assert!(
        status_line(&screen).contains("q quit"),
        "{}",
        status_line(&screen)
    );
}

/// A small sheet with some text and numbers to look at.
fn sample_rows() -> &'static str {
    r#"<row r="1"><c r="A1" t="inlineStr"><is><t>Item</t></is></c><c r="B1" t="inlineStr"><is><t>Price</t></is></c><c r="C1" t="inlineStr"><is><t>Qty</t></is></c></row><row r="2"><c r="A2" t="inlineStr"><is><t>Apples</t></is></c><c r="B2"><v>1.2</v></c><c r="C2"><v>4</v></c></row><row r="3"><c r="A3" t="inlineStr"><is><t>Pears</t></is></c><c r="B3"><v>0.9</v></c><c r="C3"><f>C2+3</f><v>7</v></c></row>"#
}

#[test]
fn the_whole_screen_looks_right_in_each_state() {
    let (_file, mut app) = app_with(sample_rows());
    let shot = |app: &mut App| render(app, 80, 20).join("\n");
    insta::assert_snapshot!("screen_in_normal_mode", shot(&mut app));

    for c in "jl".chars() {
        press(&mut app, KeyCode::Char(c));
    }
    press(&mut app, KeyCode::Char('A'));
    press(&mut app, KeyCode::Char('5'));
    insta::assert_snapshot!("screen_while_editing", shot(&mut app));
    press(&mut app, KeyCode::Esc);

    press(&mut app, KeyCode::Char('V'));
    press(&mut app, KeyCode::Char('j'));
    insta::assert_snapshot!("screen_while_selecting", shot(&mut app));
    press(&mut app, KeyCode::Esc);

    press(&mut app, KeyCode::Char('q'));
    insta::assert_snapshot!("screen_asking_about_unsaved_changes", shot(&mut app));
    press(&mut app, KeyCode::Esc);

    app.handle_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::CONTROL));
    press(&mut app, KeyCode::Char('C'));
    press(&mut app, KeyCode::Char('7'));
    insta::assert_snapshot!("screen_going_to_a_cell", shot(&mut app));
    press(&mut app, KeyCode::Esc);

    press(&mut app, KeyCode::Char('/'));
    press(&mut app, KeyCode::Char('p'));
    insta::assert_snapshot!("screen_searching", shot(&mut app));
}

#[test]
fn the_status_line_shows_how_far_opening_has_got_and_how_to_stop() {
    let file = write_temp(&single_sheet(&sheet(""), &[]));
    let (workbook, load) = Workbook::open_loading(&file.path, 0).unwrap();
    let mut app = App::opening(workbook, load);

    let screen = render(&mut app, 80, 14);
    let status = status_line(&screen);
    assert!(
        status.starts_with(" NORMAL  Opening book.xlsx… "),
        "{status}"
    );
    assert!(status.contains("%"), "{status}");
    assert!(status.contains("Esc cancel"), "{status}");

    app.finish_loading();
    let screen = render(&mut app, 80, 14);
    assert!(status_line(&screen).contains("? help"));
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

#[test]
fn the_status_line_says_a_save_is_running_and_what_follows_it() {
    let (_file, mut app) = app_with("");
    press(&mut app, KeyCode::Char('s'));
    press(&mut app, KeyCode::Char('5'));
    press(&mut app, KeyCode::Enter);

    app.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL));
    let screen = render(&mut app, 80, 14);
    assert!(
        status_line(&screen).starts_with(" NORMAL  Saving book.xlsx…"),
        "{}",
        status_line(&screen)
    );

    press(&mut app, KeyCode::Char('x'));
    let screen = render(&mut app, 80, 14);
    assert!(
        status_line(&screen).starts_with(" NORMAL  Still saving"),
        "{}",
        status_line(&screen)
    );

    press(&mut app, KeyCode::Char('q'));
    let screen = render(&mut app, 80, 14);
    assert!(
        status_line(&screen).starts_with(" NORMAL  Saving book.xlsx… then quitting"),
        "{}",
        status_line(&screen)
    );
}

#[test]
fn the_save_dialog_says_it_is_saving() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = app_in_save_dialog(dir.path());
    for c in "budget".chars() {
        press(&mut app, KeyCode::Char(c));
    }

    press(&mut app, KeyCode::Enter);

    let screen = render(&mut app, 80, 24);
    assert!(
        screen.iter().any(|line| line.contains("Saving…")),
        "{screen:?}"
    );
}

#[test]
fn a_long_search_keeps_the_caret_on_the_status_line() {
    let (_file, mut app) = app_with("");
    let text: String = ('a'..='z').cycle().take(60).collect();
    press(&mut app, KeyCode::Char('/'));
    for c in text.chars() {
        press(&mut app, KeyCode::Char(c));
    }

    let mut terminal = terminal_for(&mut app, 40, 10);

    let position = terminal.get_cursor_position().unwrap();
    assert!(position.x < 40, "caret stays on screen, at {}", position.x);
    let buffer = terminal.backend().buffer();
    assert_eq!(buffer[(position.x - 1, position.y)].symbol(), "h");
}

#[test]
fn a_long_go_to_keeps_the_caret_in_the_dialog() {
    let (_file, mut app) = app_with("");
    let text: String = ('a'..='z').cycle().take(60).collect();
    app.handle_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::CONTROL));
    for c in text.chars() {
        press(&mut app, KeyCode::Char(c));
    }

    let mut terminal = terminal_for(&mut app, 80, 24);

    let position = terminal.get_cursor_position().unwrap();
    let buffer = terminal.backend().buffer();
    assert_eq!(buffer[(position.x - 1, position.y)].symbol(), "h");
}

#[test]
fn the_save_dialog_draws_the_terminal_cursor_at_the_names_caret() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = app_in_save_dialog(dir.path());
    press(&mut app, KeyCode::Char('a'));
    press(&mut app, KeyCode::Char('b'));
    press(&mut app, KeyCode::Left);

    let mut terminal = terminal_for(&mut app, 80, 24);

    let position = terminal.get_cursor_position().unwrap();
    let buffer = terminal.backend().buffer();
    assert_eq!(buffer[(position.x, position.y)].symbol(), "b");
}

#[test]
fn fitting_to_a_screen_gives_screen_motions_their_rows_without_drawing() {
    let (_file, mut app) = app_with("");

    fit(&mut app, Rect::new(0, 0, 80, 24));
    press(&mut app, KeyCode::Char('L'));

    assert_eq!(app.cursor().row, 14, "24 lines leave 15 rows for cells");
}

#[test]
fn drawing_leaves_the_app_as_it_was() {
    let (_file, mut app) = app_with("");
    fit(&mut app, Rect::new(0, 0, 80, 24));
    let shared: &App = &app;

    let mut terminal = Terminal::new(TestBackend::new(40, 10)).unwrap();
    terminal.draw(|frame| draw(frame, shared)).unwrap();

    assert_eq!(app.scroll().row, 0);
    press(&mut app, KeyCode::Char('L'));
    assert_eq!(
        app.cursor().row,
        14,
        "the size came from fitting, not drawing"
    );
}
