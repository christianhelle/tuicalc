use tuicalc::formula::translate;

fn shifted(formula: &str, cols: i64, rows: i64) -> String {
    translate(formula, cols, rows)
}

#[test]
fn shifts_relative_references() {
    assert_eq!(shifted("A1+B2", 1, 2), "B3+C4");
    assert_eq!(shifted("SUM(A1:A3)*2", 0, 1), "SUM(A2:A4)*2");
}

#[test]
fn keeps_absolute_parts_fixed() {
    assert_eq!(shifted("$A$1+$A1+A$1", 1, 1), "$A$1+$A2+B$1");
}

#[test]
fn shifts_whole_column_and_row_ranges() {
    assert_eq!(shifted("SUM(A:B)", 1, 5), "SUM(B:C)");
    assert_eq!(shifted("SUM($A:B)", 1, 5), "SUM($A:C)");
    assert_eq!(shifted("SUM(1:2)", 5, 1), "SUM(2:3)");
}

#[test]
fn leaves_non_references_alone() {
    for text in [
        r#""A1"&B"#,
        "LOG10(5)",
        "Sales2024*2",
        "TRUE",
        "Table1[A1]",
        "1.5E3",
        "XFE1",
    ] {
        assert_eq!(shifted(text, 1, 1), text, "{text}");
    }
}

#[test]
fn shifts_sheet_qualified_references_but_not_sheet_names() {
    assert_eq!(shifted("Q1!A1", 1, 0), "Q1!B1");
    assert_eq!(shifted("'A1 data'!A1", 0, 1), "'A1 data'!A2");
}

#[test]
fn references_shifted_off_the_sheet_become_ref_errors() {
    assert_eq!(shifted("A1", -1, 0), "#REF!");
}
