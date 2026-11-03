use tuicalc::address::{CellRef, MAX_COLS, MAX_ROWS};
use tuicalc::model::{Cell, CellValue, Sheet};
use tuicalc::motion::{Motion, Spot};

const ROWS_ON_SCREEN: u32 = 10;

fn a1(text: &str) -> CellRef {
    CellRef::parse(text).unwrap()
}

/// A sheet with these cells filled in.
fn sheet_with(cells: &[&str]) -> Sheet {
    let mut sheet = Sheet::new("Sheet1");
    for at in cells {
        sheet.load_cell(a1(at), Cell::value(CellValue::Number(1.0)));
    }
    sheet
}

/// Where a motion from `from` lands the cursor.
fn land(motion: Motion, count: Option<u32>, sheet: &Sheet, from: &str) -> CellRef {
    let spot = Spot {
        cursor: a1(from),
        scroll: a1("A1"),
    };
    motion.land(count, sheet, spot, ROWS_ON_SCREEN).cursor
}

#[test]
fn arrows_move_by_the_count_and_stop_at_the_edges_of_the_sheet() {
    let sheet = Sheet::new("Sheet1");

    assert_eq!(land(Motion::Right, None, &sheet, "A1"), a1("B1"));
    assert_eq!(land(Motion::Down, Some(3), &sheet, "A1"), a1("A4"));
    assert_eq!(land(Motion::Left, None, &sheet, "A1"), a1("A1"));
    assert_eq!(land(Motion::Up, Some(9), &sheet, "A5"), a1("A1"));
    assert_eq!(land(Motion::Right, None, &sheet, "XFD1"), a1("XFD1"));
    let bottom = CellRef {
        col: 0,
        row: MAX_ROWS - 1,
    };
    let spot = Spot {
        cursor: bottom,
        scroll: a1("A1"),
    };
    assert_eq!(
        Motion::Down.land(None, &sheet, spot, ROWS_ON_SCREEN).cursor,
        bottom
    );
}

#[test]
fn run_motions_jump_between_filled_and_empty_cells_along_the_row() {
    let sheet = sheet_with(&["A1", "B1", "D1", "E1", "G1"]);

    assert_eq!(land(Motion::NextRunStart, None, &sheet, "A1"), a1("D1"));
    assert_eq!(land(Motion::NextRunStart, Some(2), &sheet, "A1"), a1("G1"));
    assert_eq!(land(Motion::NextRunStart, None, &sheet, "G1"), a1("G1"));
    assert_eq!(land(Motion::NextRunEnd, None, &sheet, "A1"), a1("B1"));
    assert_eq!(land(Motion::NextRunEnd, None, &sheet, "B1"), a1("E1"));
    assert_eq!(land(Motion::PreviousRunStart, None, &sheet, "E1"), a1("D1"));
    assert_eq!(land(Motion::PreviousRunStart, None, &sheet, "D1"), a1("A1"));
    assert_eq!(land(Motion::PreviousRunStart, None, &sheet, "A1"), a1("A1"));
}

#[test]
fn gap_motions_jump_to_the_empty_cells_around_runs_in_the_column() {
    let sheet = sheet_with(&["A1", "A2", "A5", "A6"]);

    assert_eq!(land(Motion::NextGap, None, &sheet, "A1"), a1("A3"));
    assert_eq!(land(Motion::NextGap, None, &sheet, "A3"), a1("A7"));
    assert_eq!(land(Motion::NextGap, None, &sheet, "A7"), a1("A7"));
    assert_eq!(land(Motion::PreviousGap, None, &sheet, "A6"), a1("A4"));
    assert_eq!(land(Motion::PreviousGap, None, &sheet, "A4"), a1("A1"));
    assert_eq!(land(Motion::PreviousGap, None, &sheet, "A1"), a1("A1"));
}

#[test]
fn row_motions_know_the_filled_cells_of_the_row() {
    let sheet = sheet_with(&["B1", "D1", "F9"]);

    assert_eq!(land(Motion::RowStart, None, &sheet, "C1"), a1("A1"));
    assert_eq!(land(Motion::FirstFilled, None, &sheet, "A1"), a1("B1"));
    assert_eq!(land(Motion::LastFilled, None, &sheet, "A1"), a1("D1"));
    assert_eq!(land(Motion::FirstFilled, None, &sheet, "C5"), a1("A5"));
    assert_eq!(land(Motion::LastFilled, None, &sheet, "C5"), a1("C5"));
    assert_eq!(land(Motion::Column, Some(4), &sheet, "A1"), a1("D1"));
    assert_eq!(land(Motion::Column, None, &sheet, "D1"), a1("A1"));
    let far = CellRef {
        col: MAX_COLS - 1,
        row: 0,
    };
    assert_eq!(land(Motion::Column, Some(u32::MAX), &sheet, "A1"), far);
}

#[test]
fn sheet_motions_know_the_used_area() {
    let sheet = sheet_with(&["B1", "F9"]);

    assert_eq!(land(Motion::FirstRow, None, &sheet, "C5"), a1("C1"));
    assert_eq!(land(Motion::FirstRow, Some(3), &sheet, "C5"), a1("C3"));
    assert_eq!(land(Motion::LastRow, None, &sheet, "C5"), a1("C9"));
    assert_eq!(land(Motion::LastRow, Some(2), &sheet, "C5"), a1("C2"));
    assert_eq!(land(Motion::Origin, None, &sheet, "C5"), a1("A1"));
    assert_eq!(land(Motion::LastCell, None, &sheet, "C5"), a1("F9"));
    assert_eq!(land(Motion::RowEnd, None, &sheet, "C5"), a1("F5"));
    let empty = Sheet::new("Sheet1");
    assert_eq!(land(Motion::LastCell, None, &empty, "C5"), a1("C5"));
}

#[test]
fn screen_motions_count_from_the_top_of_the_view() {
    let sheet = Sheet::new("Sheet1");
    let spot = Spot {
        cursor: a1("B30"),
        scroll: a1("A21"),
    };
    let land = |motion: Motion, count| motion.land(count, &sheet, spot, ROWS_ON_SCREEN).cursor;

    assert_eq!(land(Motion::ScreenTop, None), a1("B21"));
    assert_eq!(land(Motion::ScreenTop, Some(3)), a1("B23"));
    assert_eq!(land(Motion::ScreenMiddle, None), a1("B25"));
    assert_eq!(land(Motion::ScreenBottom, None), a1("B30"));
    assert_eq!(land(Motion::ScreenBottom, Some(3)), a1("B28"));
    assert_eq!(land(Motion::ScreenTop, Some(99)), a1("B30"));
}

#[test]
fn paging_moves_by_the_screen_and_the_scrolling_kinds_take_the_view_along() {
    let sheet = Sheet::new("Sheet1");
    let spot = Spot {
        cursor: a1("A30"),
        scroll: a1("A25"),
    };
    let land = |motion: Motion, count| motion.land(count, &sheet, spot, ROWS_ON_SCREEN);

    assert_eq!(land(Motion::PageDown, None).cursor, a1("A40"));
    assert_eq!(land(Motion::PageDown, None).scroll, a1("A25"));
    assert_eq!(land(Motion::PageUp, Some(2)).cursor, a1("A10"));

    assert_eq!(land(Motion::ViewDown, None).cursor, a1("A40"));
    assert_eq!(land(Motion::ViewDown, None).scroll, a1("A35"));
    assert_eq!(land(Motion::ViewUp, None).scroll, a1("A15"));
    assert_eq!(land(Motion::HalfViewDown, None).cursor, a1("A35"));
    assert_eq!(land(Motion::HalfViewDown, None).scroll, a1("A30"));
    assert_eq!(land(Motion::HalfViewUp, Some(2)).cursor, a1("A20"));
    assert_eq!(land(Motion::HalfViewUp, Some(2)).scroll, a1("A15"));
}

#[test]
fn only_some_motions_stop_short_or_go_backward() {
    assert!(Motion::Left.is_exclusive());
    assert!(Motion::Right.is_exclusive());
    assert!(Motion::NextRunStart.is_exclusive());
    assert!(Motion::PreviousRunStart.is_exclusive());
    assert!(!Motion::NextRunEnd.is_exclusive());
    assert!(!Motion::Down.is_exclusive());
    assert!(!Motion::TabRight.is_exclusive());

    assert!(Motion::Left.is_backward());
    assert!(Motion::PreviousRunStart.is_backward());
    assert!(!Motion::Up.is_backward());
    assert!(!Motion::TabLeft.is_backward());
}
