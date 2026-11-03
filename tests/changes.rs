use tuicalc::address::{CellRange, CellRef, MAX_COLS};
use tuicalc::changes::{History, Register};
use tuicalc::model::{Cell, CellValue, Sheet};

fn a1(text: &str) -> CellRef {
    CellRef::parse(text).unwrap()
}

fn number(value: f64) -> Cell {
    Cell::value(CellValue::Number(value))
}

fn book() -> Vec<Sheet> {
    vec![Sheet::new("One"), Sheet::new("Two")]
}

#[test]
fn undo_puts_back_what_a_change_replaced_and_redo_brings_it_again() {
    let mut sheets = book();
    let mut history = History::default();
    sheets[1].load_cell(a1("A1"), number(1.0));

    history
        .apply(
            &mut sheets,
            1,
            vec![(a1("A1"), number(2.0)), (a1("B1"), number(3.0))],
        )
        .unwrap();

    let undone = history.undo(&mut sheets).unwrap();
    assert_eq!(undone.sheet, 1);
    assert_eq!(undone.first, Some(a1("A1")));
    assert_eq!(sheets[1].cell(a1("A1")), number(1.0));
    assert_eq!(sheets[1].cell(a1("B1")), Cell::default());

    let redone = history.redo(&mut sheets).unwrap();
    assert_eq!(redone.sheet, 1);
    assert_eq!(sheets[1].cell(a1("A1")), number(2.0));
    assert_eq!(sheets[1].cell(a1("B1")), number(3.0));
}

#[test]
fn undo_and_redo_with_nothing_to_do_say_so() {
    let mut sheets = book();
    let mut history = History::default();

    assert!(history.undo(&mut sheets).is_none());
    assert!(history.redo(&mut sheets).is_none());
}

#[test]
fn a_new_change_forgets_what_could_be_redone() {
    let mut sheets = book();
    let mut history = History::default();
    history
        .apply(&mut sheets, 0, vec![(a1("A1"), number(1.0))])
        .unwrap();
    history.undo(&mut sheets).unwrap();

    history
        .apply(&mut sheets, 0, vec![(a1("A2"), number(2.0))])
        .unwrap();

    assert!(history.redo(&mut sheets).is_none());
}

#[test]
fn a_change_that_alters_nothing_leaves_nothing_to_undo() {
    let mut sheets = book();
    let mut history = History::default();
    sheets[0].load_cell(a1("A1"), number(1.0));

    history
        .apply(&mut sheets, 0, vec![(a1("A1"), number(1.0))])
        .unwrap();

    assert!(history.undo(&mut sheets).is_none());
    assert!(!sheets[0].is_dirty());
}

#[test]
fn a_locked_cell_refuses_the_whole_change() {
    let mut sheets = book();
    let mut history = History::default();
    sheets[0].lock_range(a1("B1"), a1("B2"));

    let refused = history
        .apply(
            &mut sheets,
            0,
            vec![(a1("A1"), number(1.0)), (a1("B2"), number(2.0))],
        )
        .unwrap_err();

    assert_eq!(refused.at, a1("B2"));
    assert_eq!(
        refused.to_string(),
        "B2 is part of an array formula and is read-only"
    );
    assert_eq!(sheets[0].cell(a1("A1")), Cell::default());
    assert!(history.undo(&mut sheets).is_none());
}

fn yank(sheet: &Sheet, first: &str, last: &str) -> Register {
    Register::yank(sheet, CellRange::between(a1(first), a1(last)), false)
}

fn yank_rows(sheet: &Sheet, first: u32, last: u32) -> Register {
    let range = CellRange {
        first: CellRef { col: 0, row: first },
        last: CellRef {
            col: MAX_COLS - 1,
            row: last,
        },
    };
    Register::yank(sheet, range, true)
}

#[test]
fn a_register_counts_the_cells_it_took() {
    let mut sheet = Sheet::new("Sheet1");
    sheet.load_cell(a1("A1"), number(1.0));
    sheet.load_cell(a1("B2"), number(2.0));
    sheet.load_cell(a1("C3"), number(3.0));

    assert_eq!(yank(&sheet, "A1", "B2").len(), 2);
}

#[test]
fn a_block_lands_with_its_corner_at_the_cursor_and_keeps_its_shape() {
    let mut sheet = Sheet::new("Sheet1");
    sheet.load_cell(a1("A1"), number(1.0));
    sheet.load_cell(a1("B2"), number(2.0));

    let paste = yank(&sheet, "A1", "B2").paste(&sheet, a1("D5"), false);

    assert_eq!(paste.cursor, a1("D5"));
    assert_eq!(
        paste.cells,
        vec![(a1("D5"), number(1.0)), (a1("E6"), number(2.0))]
    );
}

#[test]
fn a_block_replaces_what_it_lands_on_even_with_its_empty_cells() {
    let mut sheet = Sheet::new("Sheet1");
    sheet.load_cell(a1("A1"), number(1.0));
    sheet.load_cell(a1("E5"), number(9.0));

    let paste = yank(&sheet, "A1", "B1").paste(&sheet, a1("D5"), false);

    assert_eq!(
        paste.cells,
        vec![(a1("D5"), number(1.0)), (a1("E5"), Cell::default())]
    );
}

#[test]
fn a_pasted_formula_shifts_its_references_and_forgets_its_cached_result() {
    let mut sheet = Sheet::new("Sheet1");
    sheet.load_cell(
        a1("A1"),
        Cell {
            value: CellValue::Number(3.0),
            ..Cell::formula("B1+1")
        },
    );

    let paste = yank(&sheet, "A1", "A1").paste(&sheet, a1("C3"), false);

    assert_eq!(paste.cells, vec![(a1("C3"), Cell::formula("D3+1"))]);
}

#[test]
fn a_pasted_number_takes_the_date_format_of_the_cell_it_lands_on() {
    let mut sheet = Sheet::new("Sheet1");
    sheet.load_cell(a1("A1"), number(1.0));
    let date = Cell {
        is_date: true,
        ..number(45000.0)
    };
    sheet.load_cell(a1("C1"), date);

    let paste = yank(&sheet, "A1", "A1").paste(&sheet, a1("C1"), false);

    let expected = Cell {
        is_date: true,
        ..number(1.0)
    };
    assert_eq!(paste.cells, vec![(a1("C1"), expected)]);
}

#[test]
fn a_block_too_big_for_the_sheet_loses_what_falls_off_the_edge() {
    let mut sheet = Sheet::new("Sheet1");
    sheet.load_cell(a1("A1"), number(1.0));
    sheet.load_cell(a1("B1"), number(2.0));

    let paste = yank(&sheet, "A1", "B1").paste(&sheet, a1("XFD1"), false);

    assert_eq!(paste.cells, vec![(a1("XFD1"), number(1.0))]);
}

#[test]
fn whole_rows_go_below_the_cursor_and_keep_its_column() {
    let mut sheet = Sheet::new("Sheet1");
    sheet.load_cell(a1("A1"), number(1.0));

    let paste = yank_rows(&sheet, 0, 0).paste(&sheet, a1("C4"), false);

    assert_eq!(paste.cursor, a1("C5"));
    assert_eq!(paste.cells, vec![(a1("A5"), number(1.0))]);
}

#[test]
fn whole_rows_pasted_before_take_the_cursors_row() {
    let mut sheet = Sheet::new("Sheet1");
    sheet.load_cell(a1("A1"), number(1.0));

    let paste = yank_rows(&sheet, 0, 0).paste(&sheet, a1("C4"), true);

    assert_eq!(paste.cursor, a1("C4"));
    assert_eq!(paste.cells, vec![(a1("A4"), number(1.0))]);
}
