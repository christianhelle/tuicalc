use tuicalc::date::{format_serial, parse_serial};

#[test]
fn formats_serials_in_the_1900_system() {
    assert_eq!(format_serial(1.0, false), "1900-01-01");
    assert_eq!(format_serial(59.0, false), "1900-02-28");
    assert_eq!(format_serial(60.0, false), "1900-02-29");
    assert_eq!(format_serial(61.0, false), "1900-03-01");
    assert_eq!(format_serial(45000.0, false), "2023-03-15");
}

#[test]
fn formats_serials_in_the_1904_system() {
    assert_eq!(format_serial(0.0, true), "1904-01-01");
    assert_eq!(format_serial(43538.0, true), "2023-03-15");
}

#[test]
fn formats_times_of_day() {
    assert_eq!(format_serial(45000.5, false), "2023-03-15 12:00:00");
    assert_eq!(format_serial(0.25, false), "06:00:00");
}

#[test]
fn parses_what_it_formats() {
    assert_eq!(parse_serial("2023-03-15", false), Some(45000.0));
    assert_eq!(parse_serial("2023-03-15 12:00:00", false), Some(45000.5));
    assert_eq!(parse_serial("06:00:00", false), Some(0.25));
    assert_eq!(parse_serial("1900-02-29", false), Some(60.0));
    assert_eq!(parse_serial("1900-01-01", false), Some(1.0));
    assert_eq!(parse_serial("2023-03-15", true), Some(43538.0));
}

#[test]
fn rejects_text_that_is_not_a_date() {
    for bad in ["hello", "2023-13-01", "2023-02-30", "2023-03-15 25:00:00", "1899-12-31"] {
        assert_eq!(parse_serial(bad, false), None, "{bad}");
    }
}
