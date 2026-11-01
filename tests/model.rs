use tuicalc::address::CellRef;
use tuicalc::model::{Cell, CellValue, Sheet};

fn a1(text: &str) -> CellRef {
    CellRef::parse(text).unwrap()
}

#[test]
fn unset_cells_read_as_empty_and_set_cells_read_back() {
    let mut sheet = Sheet::new("Data");
    assert_eq!(sheet.cell(a1("B2")), Cell::default());

    sheet.set_cell(a1("B2"), Cell::value(CellValue::Number(4.5)));

    assert_eq!(sheet.cell(a1("B2")).value, CellValue::Number(4.5));
    assert_eq!(sheet.name, "Data");
}

#[test]
fn edits_are_tracked_until_marked_saved() {
    let mut sheet = Sheet::new("Data");
    sheet.load_cell(a1("A1"), Cell::value(CellValue::Number(1.0)));
    assert!(!sheet.is_dirty());

    sheet.set_cell(a1("B1"), Cell::value(CellValue::Bool(true)));
    assert!(sheet.is_dirty());
    assert_eq!(sheet.dirty_cells().copied().collect::<Vec<_>>(), vec![a1("B1")]);

    sheet.mark_saved();
    assert!(!sheet.is_dirty());
}

#[test]
fn setting_an_empty_cell_removes_its_contents() {
    let mut sheet = Sheet::new("Data");
    sheet.load_cell(a1("A1"), Cell::value(CellValue::Number(1.0)));

    sheet.set_cell(a1("A1"), Cell::default());

    assert_eq!(sheet.cell(a1("A1")), Cell::default());
    assert!(sheet.is_dirty());
}

#[test]
fn used_extent_covers_populated_cells() {
    let mut sheet = Sheet::new("Data");
    assert_eq!(sheet.extent(), None);
    sheet.load_cell(a1("C2"), Cell::value(CellValue::Number(1.0)));
    sheet.load_cell(a1("A5"), Cell::value(CellValue::Number(1.0)));
    assert_eq!(sheet.extent(), Some(a1("C5")));
}
