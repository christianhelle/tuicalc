use tuicalc::address::{CellRange, CellRef};

fn a1(text: &str) -> CellRef {
    CellRef::parse(text).unwrap()
}

#[test]
fn parses_a1_style_references() {
    assert_eq!(CellRef::parse("A1"), Some(CellRef { col: 0, row: 0 }));
    assert_eq!(CellRef::parse("Z10"), Some(CellRef { col: 25, row: 9 }));
    assert_eq!(CellRef::parse("AA1"), Some(CellRef { col: 26, row: 0 }));
    assert_eq!(
        CellRef::parse("XFD1048576"),
        Some(CellRef {
            col: 16383,
            row: 1048575
        })
    );
}

#[test]
fn rejects_malformed_and_out_of_range_references() {
    for bad in [
        "", "A", "1", "A0", "1A", "A1B", "XFE1", "A1048577", "$A$1", "A-1",
    ] {
        assert_eq!(CellRef::parse(bad), None, "{bad}");
    }
}

#[test]
fn formats_columns_and_references_as_a1() {
    assert_eq!(CellRef { col: 0, row: 0 }.to_string(), "A1");
    assert_eq!(CellRef { col: 27, row: 99 }.to_string(), "AB100");
    assert_eq!(tuicalc::address::column_name(16383), "XFD");
}

#[test]
fn a_range_between_two_cells_covers_the_rectangle_they_span() {
    let range = CellRange::between(a1("C4"), a1("B2"));

    assert_eq!(range.first, a1("B2"));
    assert_eq!(range.last, a1("C4"));
    assert!(range.contains(a1("B2")));
    assert!(range.contains(a1("C3")));
    assert!(range.contains(a1("C4")));
    assert!(!range.contains(a1("A3")));
    assert!(!range.contains(a1("D3")));
    assert!(!range.contains(a1("B5")));
    assert!(!range.contains(a1("B1")));
}
