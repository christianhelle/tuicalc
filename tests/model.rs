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
    assert_eq!(
        sheet.dirty_cells().copied().collect::<Vec<_>>(),
        vec![a1("B1")]
    );

    sheet.mark_saved();
    assert!(!sheet.is_dirty());
}

#[test]
fn the_extent_follows_cells_as_they_come_and_go() {
    let mut sheet = Sheet::new("Data");
    let number = Cell::value(CellValue::Number(1.0));
    sheet.load_cell(a1("A1"), number.clone());
    sheet.load_cell(a1("C2"), number.clone());
    sheet.set_cell(a1("B7"), number.clone());
    assert_eq!(sheet.extent(), Some(a1("C7")));

    // Replacing a cell doesn't count it twice, so clearing it once still empties its column.
    sheet.set_cell(a1("C2"), number.clone());
    sheet.set_cell(a1("C2"), Cell::default());
    assert_eq!(sheet.extent(), Some(a1("B7")), "the last column emptied");

    sheet.set_cell(a1("B7"), Cell::default());
    assert_eq!(sheet.extent(), Some(a1("A1")), "the last row emptied too");

    // Clearing an empty cell mustn't disturb the count of the cells around it.
    sheet.set_cell(a1("A5"), Cell::default());
    assert_eq!(sheet.extent(), Some(a1("A1")));

    sheet.load_cell(a1("A1"), Cell::default());
    assert_eq!(sheet.extent(), None);
}

#[test]
fn cells_around_a_position_come_in_reading_order_and_wrap() {
    let mut sheet = Sheet::new("Data");
    for at in ["A1", "C1", "B2", "A3"] {
        sheet.load_cell(a1(at), Cell::value(CellValue::Number(1.0)));
    }
    let around = |from: &str, forward: bool| -> Vec<String> {
        sheet
            .cells_around(a1(from), forward)
            .map(|(at, _)| at.to_string())
            .collect()
    };

    assert_eq!(around("B2", true), ["A3", "A1", "C1", "B2"]);
    assert_eq!(around("B2", false), ["C1", "A1", "A3", "B2"]);
    assert_eq!(
        around("D1", true),
        ["B2", "A3", "A1", "C1"],
        "from an empty cell"
    );
    assert_eq!(
        around("D1", false),
        ["C1", "A1", "A3", "B2"],
        "from an empty cell"
    );
    assert_eq!(around("A1", false), ["A3", "B2", "C1", "A1"]);
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

#[test]
fn a_copy_of_a_sheet_keeps_its_cells_when_the_original_changes() {
    let mut sheet = Sheet::new("Data");
    sheet.load_cell(a1("A1"), Cell::value(CellValue::Number(1.0)));
    let copy = sheet.clone();

    sheet.set_cell(a1("A1"), Cell::value(CellValue::Number(2.0)));
    sheet.set_cell(a1("B1"), Cell::value(CellValue::Number(3.0)));

    assert_eq!(copy.cell(a1("A1")).value, CellValue::Number(1.0));
    assert!(!copy.has_cell(a1("B1")));
    assert!(!copy.is_dirty());
    assert_eq!(sheet.cell(a1("A1")).value, CellValue::Number(2.0));
}
