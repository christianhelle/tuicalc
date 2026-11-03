//! Where each cursor motion lands, on a sheet and a view of it.

use crate::address::{CellRef, MAX_COLS, MAX_ROWS};
use crate::model::Sheet;

/// A way of moving the cursor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Motion {
    Left,
    Right,
    Up,
    Down,
    /// Tab moves as `l` does, but an operator takes the cell it lands on.
    TabRight,
    TabLeft,
    /// `0`: the first column.
    RowStart,
    /// `|`: the column given by the count.
    Column,
    /// `^`: the first filled cell of the row.
    FirstFilled,
    /// `$`: the last filled cell of the row.
    LastFilled,
    /// End: the last used column.
    RowEnd,
    /// `gg`: the first row, or the row given by the count.
    FirstRow,
    /// `G`: the last used row, or the row given by the count.
    LastRow,
    /// `w`: the first cell of the next run of filled cells in this row.
    NextRunStart,
    /// `e`: the last cell of this run, or of the next one when already at the end.
    NextRunEnd,
    /// `b`: the first cell of this run, or of the previous one when already at the start.
    PreviousRunStart,
    /// `}`: the empty cell below the next run of filled cells in this column.
    NextGap,
    /// `{`: the empty cell above the previous run of filled cells, or the top of the sheet.
    PreviousGap,
    /// `H`: a row counted from the top of the view.
    ScreenTop,
    /// `M`: the middle of the view.
    ScreenMiddle,
    /// `L`: a row counted from the bottom of the view.
    ScreenBottom,
    PageDown,
    PageUp,
    /// `Ctrl-F`: a page down, with the view.
    ViewDown,
    /// `Ctrl-B`: a page up, with the view.
    ViewUp,
    /// `Ctrl-D`: half a page down, with the view.
    HalfViewDown,
    /// `Ctrl-U`: half a page up, with the view.
    HalfViewUp,
    /// Ctrl-Home: the first cell.
    Origin,
    /// Ctrl-End: the bottom-right corner of the used area.
    LastCell,
}

/// Where the cursor and the view are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Spot {
    pub cursor: CellRef,
    /// The cell shown in the grid's top-left corner.
    pub scroll: CellRef,
}

impl Motion {
    /// Whether an operator stops short of where this lands, so `dw` keeps the next run and `dl`
    /// takes one cell.
    pub fn is_exclusive(self) -> bool {
        matches!(
            self,
            Self::Left | Self::Right | Self::NextRunStart | Self::PreviousRunStart
        )
    }

    /// Whether an operator with this motion does nothing if the motion doesn't move.
    pub fn is_backward(self) -> bool {
        matches!(self, Self::Left | Self::PreviousRunStart)
    }

    /// Where this lands from `spot`, on a view of `rows` rows.
    pub fn land(self, count: Option<u32>, sheet: &Sheet, spot: Spot, rows: u32) -> Spot {
        let n = i64::from(count.unwrap_or(1));
        let page = i64::from(rows);
        let half = i64::from(rows / 2);
        let Spot { cursor, scroll } = spot;
        let with_cursor = |at: CellRef| Spot { cursor: at, scroll };
        let in_col = |col: u32| with_cursor(CellRef { col, ..cursor });
        let in_row = |row: u32| with_cursor(CellRef { row, ..cursor });
        let from_top = count.unwrap_or(1).saturating_sub(1).min(rows - 1);
        match self {
            Self::Left | Self::TabLeft => with_cursor(moved(cursor, -n, 0)),
            Self::Right | Self::TabRight => with_cursor(moved(cursor, n, 0)),
            Self::Up => with_cursor(moved(cursor, 0, -n)),
            Self::Down => with_cursor(moved(cursor, 0, n)),
            Self::RowStart => in_col(0),
            Self::Column => in_col(count.unwrap_or(1).saturating_sub(1).min(MAX_COLS - 1)),
            Self::FirstFilled => in_col(first_filled_column(sheet, cursor.row).unwrap_or(0)),
            Self::LastFilled => in_col(last_filled_column(sheet, cursor.row).unwrap_or(cursor.col)),
            Self::RowEnd => in_col(sheet.extent().map_or(cursor.col, |extent| extent.col)),
            Self::FirstRow => in_row(count.unwrap_or(1).saturating_sub(1).min(MAX_ROWS - 1)),
            Self::LastRow => {
                let last = sheet.extent().map_or(0, |extent| extent.row);
                in_row(count.map_or(last, |row| row.saturating_sub(1)))
            }
            Self::NextRunStart => with_cursor(repeated(n, cursor, sheet, next_run_start)),
            Self::NextRunEnd => with_cursor(repeated(n, cursor, sheet, next_run_end)),
            Self::PreviousRunStart => with_cursor(repeated(n, cursor, sheet, previous_run_start)),
            Self::NextGap => with_cursor(repeated(n, cursor, sheet, next_gap)),
            Self::PreviousGap => with_cursor(repeated(n, cursor, sheet, previous_gap)),
            Self::ScreenTop => in_row(scroll.row + from_top),
            Self::ScreenMiddle => in_row(scroll.row + (rows - 1) / 2),
            Self::ScreenBottom => in_row(scroll.row + rows - 1 - from_top),
            Self::PageDown => with_cursor(moved(cursor, 0, n * page)),
            Self::PageUp => with_cursor(moved(cursor, 0, -n * page)),
            Self::ViewDown => with_view(spot, n * page),
            Self::ViewUp => with_view(spot, -n * page),
            Self::HalfViewDown => with_view(spot, n * half),
            Self::HalfViewUp => with_view(spot, -n * half),
            Self::Origin => with_cursor(CellRef { col: 0, row: 0 }),
            Self::LastCell => with_cursor(sheet.extent().unwrap_or(cursor)),
        }
    }
}

/// A cell the given distance away, stopping at the edges of the sheet.
pub fn moved(from: CellRef, cols: i64, rows: i64) -> CellRef {
    let col = i64::from(from.col) + cols;
    let row = i64::from(from.row) + rows;
    CellRef {
        col: col.clamp(0, i64::from(MAX_COLS) - 1) as u32,
        row: row.clamp(0, i64::from(MAX_ROWS) - 1) as u32,
    }
}

/// Moves the view and the cursor by the same number of rows.
fn with_view(spot: Spot, rows: i64) -> Spot {
    let top = i64::from(spot.scroll.row) + rows;
    Spot {
        cursor: moved(spot.cursor, 0, rows),
        scroll: CellRef {
            row: top.clamp(0, i64::from(MAX_ROWS) - 1) as u32,
            ..spot.scroll
        },
    }
}

fn repeated(
    times: i64,
    from: CellRef,
    sheet: &Sheet,
    step: fn(&Sheet, CellRef) -> Option<CellRef>,
) -> CellRef {
    let mut at = from;
    for _ in 0..times {
        match step(sheet, at) {
            Some(next) => at = next,
            None => break,
        }
    }
    at
}

/// The populated columns of a row, from left to right.
fn filled_columns(sheet: &Sheet, row: u32) -> impl Iterator<Item = u32> + '_ {
    let last = sheet.extent().map_or(0, |extent| extent.col);
    (0..=last).filter(move |&col| sheet.has_cell(CellRef { col, row }))
}

fn first_filled_column(sheet: &Sheet, row: u32) -> Option<u32> {
    filled_columns(sheet, row).next()
}

/// The last populated column of a row.
pub fn last_filled_column(sheet: &Sheet, row: u32) -> Option<u32> {
    filled_columns(sheet, row).last()
}

/// The last column worth searching on this sheet.
fn last_column(sheet: &Sheet) -> u32 {
    sheet.extent().map_or(0, |extent| extent.col)
}

fn next_run_start(sheet: &Sheet, from: CellRef) -> Option<CellRef> {
    let filled = |col| sheet.has_cell(CellRef { col, ..from });
    let last = last_column(sheet);
    let mut col = from.col;
    while col <= last && filled(col) {
        col += 1;
    }
    while col <= last && !filled(col) {
        col += 1;
    }
    (col <= last).then_some(CellRef { col, ..from })
}

fn next_run_end(sheet: &Sheet, from: CellRef) -> Option<CellRef> {
    let filled = |col| sheet.has_cell(CellRef { col, ..from });
    let last = last_column(sheet);
    let mut col = from.col + 1;
    while col <= last && !filled(col) {
        col += 1;
    }
    if col > last {
        return None;
    }
    while col < last && filled(col + 1) {
        col += 1;
    }
    Some(CellRef { col, ..from })
}

fn previous_run_start(sheet: &Sheet, from: CellRef) -> Option<CellRef> {
    let filled = |col| sheet.has_cell(CellRef { col, ..from });
    let mut col = from.col;
    while col > 0 && !filled(col - 1) {
        col -= 1;
    }
    if col == 0 {
        return None;
    }
    col -= 1;
    while col > 0 && filled(col - 1) {
        col -= 1;
    }
    Some(CellRef { col, ..from })
}

fn next_gap(sheet: &Sheet, from: CellRef) -> Option<CellRef> {
    let filled = |row| sheet.has_cell(CellRef { row, ..from });
    let last = sheet.extent()?.row;
    let mut row = from.row;
    while row <= last && !filled(row) {
        row += 1;
    }
    if row > last {
        return None;
    }
    while row <= last && filled(row) {
        row += 1;
    }
    Some(CellRef { row, ..from })
}

fn previous_gap(sheet: &Sheet, from: CellRef) -> Option<CellRef> {
    let filled = |row| sheet.has_cell(CellRef { row, ..from });
    let mut row = from.row;
    if row == 0 {
        return None;
    }
    while row > 0 && !filled(row) {
        row -= 1;
    }
    while row > 0 && filled(row) {
        row -= 1;
    }
    Some(CellRef { row, ..from })
}
