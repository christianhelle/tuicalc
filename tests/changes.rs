use tuicalc::address::CellRef;
use tuicalc::changes::History;
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
