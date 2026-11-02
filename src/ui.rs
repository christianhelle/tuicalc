//! Draws the application state.

use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, Paragraph};
use unicode_width::UnicodeWidthChar;

use crate::address::{CellRef, column_name};
use crate::app::{App, Mode, NextStep};
use crate::file_dialog::{DialogKind, FileDialog};
use crate::text::{display_text, edit_text, is_right_aligned};

/// The cursor and highlighted dialog lines. Reversed video follows the terminal's theme.
const SELECTED_STYLE: Style = Style::new().add_modifier(Modifier::REVERSED);
/// The cursor inside a selection, which reversed video alone couldn't tell apart from it.
const SELECTED_CURSOR_STYLE: Style = SELECTED_STYLE
    .fg(Color::Yellow)
    .add_modifier(Modifier::BOLD);
const HEADER_STYLE: Style = Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD);
const LINE_STYLE: Style = Style::new().fg(Color::DarkGray);
const CURSOR_HEADER_STYLE: Style = Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD);
const ROW_HEADER_WIDTH: u16 = 8;
const COLUMN_WIDTH: u16 = 12;
/// Column width less the line separating neighbouring cells.
const CONTENT_WIDTH: usize = COLUMN_WIDTH as usize - 1;

/// Where each part of the screen goes. Drawing and mouse clicks must agree, so both ask here.
fn layout(area: Rect) -> [Rect; 4] {
    Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(0),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(area)
}

pub fn draw(frame: &mut Frame, app: &mut App) {
    let [formula_bar, grid, tabs, status] = layout(frame.area());
    let editing = app.mode() == Mode::Insert;
    let marker = if app.workbook.is_dirty() { "*" } else { "" };
    let title = format!("{}{marker} — {}", app.file_name(), app.current_sheet().name);
    let grid_block = section_block(!editing, &title);
    let grid_inner = grid_block.inner(grid);
    frame.render_widget(grid_block, grid);
    let visible_cols = u32::from(grid_inner.width.saturating_sub(ROW_HEADER_WIDTH) / COLUMN_WIDTH);
    let visible_rows = u32::from(grid_inner.height.saturating_sub(2));
    app.set_viewport(visible_cols, visible_rows);
    draw_grid(
        frame.buffer_mut(),
        grid_inner,
        app,
        visible_cols,
        visible_rows,
    );
    let formula_block = section_block(editing, &app.cursor().to_string());
    let formula_inner = formula_block.inner(formula_bar);
    frame.render_widget(formula_block, formula_bar);
    draw_formula_bar(frame, formula_inner, app);
    draw_tabs(frame.buffer_mut(), tabs, app);
    draw_status(frame.buffer_mut(), status, app);
    if app.mode() == Mode::Command && status.width > 0 && status.height > 0 {
        let chars: Vec<char> = app.buffer().chars().collect();
        let x = status.x + 1 + text_width(&chars[..app.caret()]) as u16;
        frame.set_cursor_position((x.min(status.right().saturating_sub(1)), status.y));
    }
    if let Some(dialog) = app.file_dialog() {
        draw_file_dialog(frame, dialog);
    }
    if app.mode() == Mode::ConfirmDiscard {
        draw_discard_dialog(frame, app);
    }
    if app.mode() == Mode::GoTo {
        draw_go_to_dialog(frame, app);
    }
}

const GO_TO_DIALOG_WIDTH: u16 = 52;
const GO_TO_LABEL: &str = "Cell or row: ";

/// Asks where to jump, with the terminal cursor at the caret.
fn draw_go_to_dialog(frame: &mut Frame, app: &App) {
    let last_line = if app.status_is_error() {
        Line::styled(app.status().to_string(), Style::new().fg(Color::Red))
    } else {
        Line::from(vec![
            Span::styled("Enter", Style::new().fg(Color::Yellow)),
            Span::raw(" go   "),
            Span::styled("Esc", Style::new().fg(Color::Cyan)),
            Span::raw(" cancel"),
        ])
    };
    let lines = vec![
        Line::raw(format!("{GO_TO_LABEL}{}", app.buffer())),
        last_line,
    ];
    let area = frame.area().centered(
        Constraint::Length(GO_TO_DIALOG_WIDTH),
        Constraint::Length(lines.len() as u16 + 2),
    );
    let block = Block::bordered()
        .border_style(Style::new().fg(Color::Yellow))
        .title(" Go to ");
    let inner = block.inner(area);
    frame.render_widget(Clear, area);
    frame.render_widget(Paragraph::new(lines).block(block), area);
    if inner.width > 0 && inner.height > 0 {
        let chars: Vec<char> = app.buffer().chars().collect();
        let x = inner.x + (GO_TO_LABEL.len() + text_width(&chars[..app.caret()])) as u16;
        frame.set_cursor_position((x.min(inner.right().saturating_sub(1)), inner.y));
    }
}

const DISCARD_DIALOG_WIDTH: u16 = 60;

/// Asks what to do with unsaved edits, naming what happens next.
fn draw_discard_dialog(frame: &mut Frame, app: &App) {
    let next = match app.next_step() {
        Some(NextStep::OpenFile) => "open",
        Some(NextStep::Reload) => "reload",
        _ => "quit",
    };
    let lines = vec![
        Line::raw(format!("{} has unsaved changes.", app.file_name())),
        Line::from(vec![
            Span::styled("s", Style::new().fg(Color::Yellow)),
            Span::raw(format!(" save and {next}   ")),
            Span::styled("d", Style::new().fg(Color::Red)),
            Span::raw(format!(" discard and {next}   ")),
            Span::styled("Esc", Style::new().fg(Color::Cyan)),
            Span::raw(" stay"),
        ]),
    ];
    let area = frame.area().centered(
        Constraint::Length(DISCARD_DIALOG_WIDTH),
        Constraint::Length(lines.len() as u16 + 2),
    );
    let block = Block::bordered()
        .border_style(Style::new().fg(Color::Red))
        .title(" Unsaved changes ");
    frame.render_widget(Clear, area);
    frame.render_widget(Paragraph::new(lines).block(block), area);
}

/// A pane's box: a thick border marks the one in use.
fn section_block(focused: bool, title: &str) -> Block<'static> {
    let border = if focused {
        BorderType::Thick
    } else {
        BorderType::Plain
    };
    Block::bordered()
        .border_type(border)
        .title(format!(" {title} "))
}

const FILE_DIALOG_WIDTH: u16 = 60;
const FILE_DIALOG_LIST_ROWS: usize = 12;
const FILE_DIALOG_NAME_LABEL: &str = "Name: ";

/// Draws the dialog over the middle of the screen.
fn draw_file_dialog(frame: &mut Frame, dialog: &FileDialog) {
    let area = frame.area();
    let list_rows = dialog.entries().len().clamp(1, FILE_DIALOG_LIST_ROWS) as u16;
    let popup = area.centered(
        Constraint::Length(FILE_DIALOG_WIDTH),
        Constraint::Length(list_rows + 5),
    );
    let title = match dialog.kind() {
        DialogKind::Save => " Save as ",
        DialogKind::Open => " Open ",
    };
    let block = Block::bordered()
        .border_style(Style::new().fg(Color::Yellow))
        .title(title);
    let inner = block.inner(popup);
    frame.render_widget(Clear, popup);
    frame.render_widget(block, popup);
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let room = usize::from(inner.width);
    // A short terminal gives up the list first, then the path, then the hint.
    let list_rows = inner.height.saturating_sub(3);
    let show_path = inner.height >= 3;
    let show_hint = inner.height >= 2;
    let buffer = frame.buffer_mut();
    if show_path {
        let path = dialog.directory().display().to_string();
        buffer.set_stringn(
            inner.x,
            inner.y,
            keep_end(&path, room),
            room,
            Style::default(),
        );
    }
    draw_file_dialog_list(buffer, inner, dialog, list_rows);

    let name_y = inner.y + u16::from(show_path) + list_rows;
    let label_width = FILE_DIALOG_NAME_LABEL.len();
    // One cell stays free for the terminal cursor after the name.
    let name = keep_end(dialog.name(), room.saturating_sub(label_width + 1));
    let line = format!("{FILE_DIALOG_NAME_LABEL}{name}");
    buffer.set_stringn(inner.x, name_y, &line, room, Style::default());
    if show_hint {
        buffer.set_line(inner.x, name_y + 1, &file_dialog_hint(dialog), inner.width);
    }
    if dialog.pending_overwrite().is_none() {
        let chars: Vec<char> = line.chars().collect();
        let x = inner.x + text_width(&chars) as u16;
        frame.set_cursor_position((x.min(inner.right().saturating_sub(1)), name_y));
    }
}

/// Draws as much of the list as fits, starting late enough to show the highlight.
fn draw_file_dialog_list(buffer: &mut Buffer, inner: Rect, dialog: &FileDialog, rows: u16) {
    let room = usize::from(inner.width);
    let first = dialog.highlight().map_or(0, |highlight| {
        (highlight + 1).saturating_sub(usize::from(rows))
    });
    let visible = dialog.entries().iter().enumerate().skip(first);
    for (offset, (index, entry)) in visible.take(usize::from(rows)).enumerate() {
        let y = inner.y + 1 + offset as u16;
        let selected = dialog.highlight() == Some(index);
        let (marker, style) = if selected {
            (">", SELECTED_STYLE)
        } else {
            (" ", Style::default())
        };
        let line = format!("{marker} {:<room$}", entry.label());
        buffer.set_stringn(inner.x, y, line, room, style);
    }
}

fn file_dialog_hint(dialog: &FileDialog) -> Line<'static> {
    if let Some(path) = dialog.pending_overwrite() {
        let name = path
            .file_name()
            .unwrap_or(path.as_os_str())
            .to_string_lossy();
        Line::raw(format!("{name} exists. Overwrite? [y/n]"))
    } else if dialog.message().is_empty() {
        let action = match dialog.kind() {
            DialogKind::Save => " save   ",
            DialogKind::Open => " open   ",
        };
        Line::from(vec![
            Span::styled("Enter", Style::new().fg(Color::Yellow)),
            Span::raw(action),
            Span::styled("Esc", Style::new().fg(Color::Cyan)),
            Span::raw(" cancel"),
        ])
    } else {
        Line::styled(dialog.message().to_string(), Style::new().fg(Color::Red))
    }
}

/// Cuts text from the left to fit `room` terminal cells, so its end stays readable.
fn keep_end(text: &str, room: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    if text_width(&chars) <= room {
        return text.to_string();
    }
    let mut start = 0;
    while start < chars.len() && text_width(&chars[start..]) + 1 > room {
        start += 1;
    }
    let tail: String = chars[start..].iter().collect();
    format!("…{tail}")
}

/// Shows the cursor's contents, or the text being edited with its caret.
fn draw_formula_bar(frame: &mut Frame, area: Rect, app: &App) {
    if area.width == 0 || area.height == 0 {
        return;
    }
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
    // One cell stays free for the terminal cursor after the text.
    let room = usize::from(area.width.saturating_sub(1));
    let caret = if editing { app.caret() } else { 0 };

    // Drop leading characters until the caret fits in the visible room.
    let mut start = 0;
    while text_width(&content[start..caret]) > room && start < caret {
        start += 1;
    }
    let visible: String = content[start..].iter().collect();
    if room > 0 {
        frame
            .buffer_mut()
            .set_stringn(area.x, area.y, visible, room, Style::default());
    }
    if editing {
        let x = area.x + text_width(&content[start..caret]) as u16;
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

const NORMAL_HINTS: &str =
    " i edit · v select · u undo · gt sheet · : command · Ctrl-S save · Ctrl-Q quit";
const VISUAL_HINTS: &str = " d delete · y copy · c change · o other corner · Esc back";
const INSERT_HINTS: &str = " Esc done · Enter next · Ctrl-C cancel";

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
    let (name, color, hint) = match app.mode() {
        Mode::Normal => ("NORMAL", Color::Blue, NORMAL_HINTS),
        Mode::Insert => ("INSERT", Color::Green, INSERT_HINTS),
        Mode::Visual => ("VISUAL", Color::Magenta, VISUAL_HINTS),
        Mode::VisualRow => ("V-LINE", Color::Magenta, VISUAL_HINTS),
        Mode::ConfirmDiscard => ("NORMAL", Color::Blue, ""),
        Mode::Command => ("COMMAND", Color::Blue, ""),
        Mode::GoTo => ("NORMAL", Color::Blue, ""),
        Mode::FileDialog => match app.file_dialog().map(FileDialog::kind) {
            Some(DialogKind::Open) => ("OPEN", Color::Blue, " Choose a workbook"),
            _ => ("SAVE", Color::Blue, " Choose a folder and a name"),
        },
    };
    // The go-to dialog shows its own complaints.
    let message = if app.status().is_empty() || app.mode() == Mode::GoTo {
        hint.to_string()
    } else {
        format!(" {}", app.status())
    };
    let badge = format!(" {name} ");
    let badge_style = Style::new().fg(Color::Black).bg(color);
    let message_style = if app.status_is_error() {
        Style::new().fg(Color::Red)
    } else {
        Style::default()
    };
    let room = usize::from(area.width);
    buffer.set_stringn(area.x, area.y, &badge, room, badge_style);
    let badge_width = badge.len() as u16;
    if badge_width < area.width {
        buffer.set_stringn(
            area.x + badge_width,
            area.y,
            message,
            room - usize::from(badge_width),
            message_style,
        );
    }
    let pending = app.pending_keys();
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
    let header_style = |current: bool| {
        if current {
            CURSOR_HEADER_STYLE
        } else {
            HEADER_STYLE
        }
    };
    draw_grid_lines(buffer, area, cols, rows);
    for offset in 0..cols {
        let x = area.x + ROW_HEADER_WIDTH + offset as u16 * COLUMN_WIDTH;
        let name = column_name(scroll.col + offset);
        let style = header_style(scroll.col + offset == app.cursor().col);
        buffer.set_stringn(x, area.y, center(&name), CONTENT_WIDTH, style);
    }
    let sheet = app.current_sheet();
    let date1904 = app.workbook.date1904;
    let selection = app.selection();
    for row_offset in 0..rows {
        let y = area.y + 2 + row_offset as u16;
        let row = scroll.row + row_offset;
        let number_width = usize::from(ROW_HEADER_WIDTH) - 1;
        buffer.set_stringn(
            area.x,
            y,
            format!("{:>number_width$}", row + 1),
            number_width,
            header_style(row == app.cursor().row),
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
            let style = if at == app.cursor() && selection.is_some() {
                SELECTED_CURSOR_STYLE
            } else if at == app.cursor() || selected {
                SELECTED_STYLE
            } else {
                Style::default()
            };
            buffer.set_stringn(x, y, padded, CONTENT_WIDTH, style);
        }
    }
}

/// Draws the line under the headers and the lines between columns.
fn draw_grid_lines(buffer: &mut Buffer, area: Rect, cols: u32, rows: u32) {
    if area.width < ROW_HEADER_WIDTH {
        return;
    }
    // The header row is always there, the rule and the data rows only when they fit.
    let line_rows = std::iter::once(0).chain((0..rows).map(|row| row + 2));
    for y in line_rows {
        for col in 0..=cols {
            let x = ROW_HEADER_WIDTH - 1 + col as u16 * COLUMN_WIDTH;
            buffer.set_string(area.x + x, area.y + y as u16, "│", LINE_STYLE);
        }
    }
    if area.height >= 2 {
        let mut rule = "─".repeat(usize::from(ROW_HEADER_WIDTH) - 1);
        for _ in 0..cols {
            rule.push('┼');
            rule.push_str(&"─".repeat(CONTENT_WIDTH));
        }
        rule.push('┤');
        buffer.set_string(area.x, area.y + 1, rule, LINE_STYLE);
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
