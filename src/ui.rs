//! Draws the application state.

use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use unicode_width::UnicodeWidthChar;

use crate::address::{CellRef, column_name};
use crate::app::{App, Mode};
use crate::text::{display_text, edit_text, is_right_aligned};

/// The header bars and the cursor, as a nod to Lotus 1-2-3. The color is exact because a terminal
/// theme can make its own "cyan" any shade, and the text isn't bold because some terminals show
/// bold black as grey.
const FRAME_STYLE: Style = Style::new()
    .fg(Color::Black)
    .bg(Color::Rgb(0x00, 0xA8, 0xA8));
const ROW_HEADER_WIDTH: u16 = 8;
const COLUMN_WIDTH: u16 = 12;
/// Column width less the gap separating neighbouring cells.
const CONTENT_WIDTH: usize = COLUMN_WIDTH as usize - 1;

pub fn draw(frame: &mut Frame, app: &mut App) {
    let [formula_bar, grid, tabs, status] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(frame.area());
    let visible_cols = u32::from(grid.width.saturating_sub(ROW_HEADER_WIDTH) / COLUMN_WIDTH);
    let visible_rows = u32::from(grid.height.saturating_sub(1));
    app.set_viewport(visible_cols, visible_rows);
    draw_grid(frame.buffer_mut(), grid, app, visible_cols, visible_rows);
    draw_formula_bar(frame, formula_bar, app);
    draw_tabs(frame.buffer_mut(), tabs, app);
    draw_status(frame.buffer_mut(), status, app);
    if app.mode() == Mode::Command && status.width > 0 && status.height > 0 {
        let chars: Vec<char> = app.buffer().chars().collect();
        let x = status.x + 1 + text_width(&chars[..app.caret()]) as u16;
        frame.set_cursor_position((x.min(status.right().saturating_sub(1)), status.y));
    }
}

/// Shows the cursor's address and contents, or the text being edited with its caret.
fn draw_formula_bar(frame: &mut Frame, area: Rect, app: &App) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let label = format!("{:<8}", app.cursor().to_string());
    let label_width = label.len() as u16;
    let editing = app.mode() == Mode::Insert;
    let content = if editing {
        app.buffer()
    } else {
        edit_text(
            &app.current_sheet().cell(app.cursor()),
            app.workbook.date1904,
        )
    };
    let content: Vec<char> = content
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let room = usize::from(area.width.saturating_sub(label_width + 1));
    let caret = if editing { app.caret() } else { 0 };

    // Drop leading characters until the caret fits in the visible room.
    let mut start = 0;
    while text_width(&content[start..caret]) > room && start < caret {
        start += 1;
    }
    let visible: String = content[start..].iter().collect();
    let buffer = frame.buffer_mut();
    let bold = Style::default().add_modifier(Modifier::BOLD);
    buffer.set_stringn(area.x, area.y, &label, usize::from(area.width), bold);
    if room > 0 {
        buffer.set_stringn(
            area.x + label_width,
            area.y,
            visible,
            room,
            Style::default(),
        );
    }
    if editing {
        let x = area.x + label_width + text_width(&content[start..caret]) as u16;
        frame.set_cursor_position((x.min(area.right().saturating_sub(1)), area.y));
    }
}

fn text_width(chars: &[char]) -> usize {
    chars.iter().map(|c| c.width().unwrap_or(0)).sum()
}

/// Shows the sheet names, starting late enough to keep the current one on screen.
fn draw_tabs(buffer: &mut Buffer, area: Rect, app: &App) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let names: Vec<String> = app
        .workbook
        .sheets
        .iter()
        .map(|sheet| format!(" {} ", sheet.name))
        .collect();
    let width_of = |name: &String| name.chars().map(|c| c.width().unwrap_or(0)).sum::<usize>();
    let current = app.sheet_index();
    let mut first = 0;
    while first < current
        && names[first..=current].iter().map(width_of).sum::<usize>() > usize::from(area.width)
    {
        first += 1;
    }
    let mut x = area.x;
    for (index, name) in names.iter().enumerate().skip(first) {
        if x >= area.right() {
            break;
        }
        let style = if index == current {
            Style::default().add_modifier(Modifier::REVERSED)
        } else {
            Style::default()
        };
        let room = usize::from(area.right() - x);
        buffer.set_stringn(x, area.y, name, room, style);
        x = x.saturating_add(width_of(name) as u16);
    }
}

const HELP: &str = "i insert | v visual | : command | u undo | gt sheet | :w save | :q quit";
const VISUAL_HELP: &str = "d delete | y yank | c change | o other corner | Esc back";
const CONFIRM_QUIT: &str = "Unsaved changes: [s]ave and quit, [d]iscard, Esc to stay";

fn draw_status(buffer: &mut Buffer, area: Rect, app: &App) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    if app.mode() == Mode::Command {
        let line = format!(":{}", app.buffer());
        buffer.set_stringn(
            area.x,
            area.y,
            line,
            usize::from(area.width),
            Style::default(),
        );
        return;
    }
    let marker = if app.workbook.is_dirty() { "*" } else { "" };
    let (name, hint) = match app.mode() {
        Mode::Normal => ("NORMAL", HELP),
        Mode::Insert => ("INSERT", "Esc done | Enter next | Ctrl+C cancel"),
        Mode::Visual => ("VISUAL", VISUAL_HELP),
        Mode::VisualRow => ("V-LINE", VISUAL_HELP),
        Mode::ConfirmQuit => ("NORMAL", CONFIRM_QUIT),
        Mode::Command => ("COMMAND", ""),
        Mode::SaveDialog => ("SAVE", ""),
    };
    let message = if app.status().is_empty() || app.mode() == Mode::ConfirmQuit {
        hint
    } else {
        app.status()
    };
    let pending = app.pending_keys();
    let line = format!("-- {name} -- {}{marker} | {message}", app.file_name());
    buffer.set_stringn(
        area.x,
        area.y,
        line,
        usize::from(area.width),
        Style::default(),
    );
    let width = pending.chars().count() as u16;
    if width > 0 && width < area.width {
        let x = area.right() - width;
        buffer.set_stringn(
            x - 1,
            area.y,
            format!(" {pending}"),
            usize::from(width) + 1,
            Style::default(),
        );
    }
}

fn draw_grid(buffer: &mut Buffer, area: Rect, app: &App, cols: u32, rows: u32) {
    if area.height == 0 || area.width == 0 {
        return;
    }
    let scroll = app.scroll();
    let header_style = FRAME_STYLE;
    buffer.set_style(Rect { height: 1, ..area }, header_style);
    buffer.set_style(
        Rect {
            y: area.y + 1,
            width: ROW_HEADER_WIDTH.min(area.width),
            height: area.height - 1,
            ..area
        },
        header_style,
    );
    for offset in 0..cols {
        let x = area.x + ROW_HEADER_WIDTH + offset as u16 * COLUMN_WIDTH;
        let name = column_name(scroll.col + offset);
        buffer.set_stringn(x, area.y, center(&name), CONTENT_WIDTH, header_style);
    }
    let sheet = app.current_sheet();
    let date1904 = app.workbook.date1904;
    let selection = app.selection();
    for row_offset in 0..rows {
        let y = area.y + 1 + row_offset as u16;
        let row = scroll.row + row_offset;
        buffer.set_stringn(
            area.x,
            y,
            (row + 1).to_string(),
            usize::from(ROW_HEADER_WIDTH) - 1,
            header_style,
        );
        for col_offset in 0..cols {
            let x = area.x + ROW_HEADER_WIDTH + col_offset as u16 * COLUMN_WIDTH;
            let at = CellRef {
                col: scroll.col + col_offset,
                row,
            };
            let cell = sheet.cell(at);
            let text = display_text(&cell, date1904, CONTENT_WIDTH);
            let padded = pad(&text, is_right_aligned(&cell));
            let selected = selection.is_some_and(|(first, last)| {
                (first.col..=last.col).contains(&at.col) && (first.row..=last.row).contains(&at.row)
            });
            let style = if at == app.cursor() {
                FRAME_STYLE
            } else if selected {
                Style::default().add_modifier(Modifier::REVERSED)
            } else {
                Style::default()
            };
            buffer.set_stringn(x, y, padded, CONTENT_WIDTH, style);
        }
    }
}

fn center(text: &str) -> String {
    let left = CONTENT_WIDTH.saturating_sub(text.len()) / 2;
    format!("{}{text}", " ".repeat(left))
}

/// Cuts text to the column's width in terminal cells and fills the remainder.
fn pad(text: &str, right_aligned: bool) -> String {
    let mut fitted = String::new();
    let mut width = 0;
    for c in text.chars().map(|c| if c.is_control() { ' ' } else { c }) {
        let char_width = c.width().unwrap_or(0);
        if width + char_width > CONTENT_WIDTH {
            break;
        }
        fitted.push(c);
        width += char_width;
    }
    let fill = " ".repeat(CONTENT_WIDTH - width);
    if right_aligned {
        format!("{fill}{fitted}")
    } else {
        format!("{fitted}{fill}")
    }
}
