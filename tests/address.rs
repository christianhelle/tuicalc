use tuicalc::address::CellRef;

#[test]
fn parses_a1_style_references() {
    assert_eq!(CellRef::parse("A1"), Some(CellRef { col: 0, row: 0 }));
    assert_eq!(CellRef::parse("Z10"), Some(CellRef { col: 25, row: 9 }));
    assert_eq!(CellRef::parse("AA1"), Some(CellRef { col: 26, row: 0 }));
    assert_eq!(CellRef::parse("XFD1048576"), Some(CellRef { col: 16383, row: 1048575 }));
}

#[test]
fn rejects_malformed_and_out_of_range_references() {
    for bad in ["", "A", "1", "A0", "1A", "A1B", "XFE1", "A1048577", "$A$1", "A-1"] {
        assert_eq!(CellRef::parse(bad), None, "{bad}");
    }
}
