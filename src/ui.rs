//! Draws the application state.

use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use unicode_width::UnicodeWidthChar;

use crate::address::{CellRef, column_name};
use crate::app::App;
use crate::text::{display_text, is_right_aligned};

const ROW_HEADER_WIDTH: u16 = 8;
const COLUMN_WIDTH: u16 = 12;
/// Column width less the gap separating neighbouring cells.
const CONTENT_WIDTH: usize = COLUMN_WIDTH as usize - 1;

pub fn draw(frame: &mut Frame, app: &mut App) {
    let [_formula_bar, grid, _tabs, _status] = Layout::vertical([
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
}

fn draw_grid(buffer: &mut Buffer, area: Rect, app: &App, cols: u32, rows: u32) {
    if area.height == 0 || area.width == 0 {
        return;
    }
    let scroll = app.scroll();
    let header_style = Style::default().add_modifier(Modifier::BOLD);
    for offset in 0..cols {
        let x = area.x + ROW_HEADER_WIDTH + offset as u16 * COLUMN_WIDTH;
        let name = column_name(scroll.col + offset);
        buffer.set_stringn(x, area.y, center(&name), CONTENT_WIDTH, header_style);
    }
    let sheet = app.current_sheet();
    let date1904 = app.workbook.date1904;
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
            let at = CellRef { col: scroll.col + col_offset, row };
            let cell = sheet.cell(at);
            let text = display_text(&cell, date1904, CONTENT_WIDTH);
            let padded = pad(&text, is_right_aligned(&cell));
            buffer.set_stringn(x, y, padded, CONTENT_WIDTH, Style::default());
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
    if right_aligned { format!("{fill}{fitted}") } else { format!("{fitted}{fill}") }
}
