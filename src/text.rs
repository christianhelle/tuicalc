//! Conversions between cell contents and the text a user sees and types.

use crate::date;
use crate::model::{Cell, CellValue};

const ERROR_LITERALS: [&str; 7] = [
    "#N/A", "#DIV/0!", "#REF!", "#VALUE!", "#NAME?", "#NUM!", "#NULL!",
];

/// Interprets what the user typed into a cell.
pub fn parse_input(text: &str, was_date: bool, date1904: bool) -> Cell {
    if text.is_empty() {
        return Cell::default();
    }
    if let Some(literal) = text.strip_prefix('\'') {
        return Cell::value(CellValue::Text(literal.to_string()));
    }
    if let Some(formula) = text.strip_prefix('=').filter(|formula| !formula.is_empty()) {
        return Cell::formula(formula);
    }
    if was_date && let Some(serial) = date::parse_serial(text, date1904) {
        return Cell {
            is_date: true,
            ..Cell::value(CellValue::Number(serial))
        };
    }
    if let Some(number) = text.parse::<f64>().ok().filter(|number| number.is_finite()) {
        return Cell {
            is_date: was_date,
            ..Cell::value(CellValue::Number(number))
        };
    }
    if text.eq_ignore_ascii_case("true") {
        return Cell::value(CellValue::Bool(true));
    }
    if text.eq_ignore_ascii_case("false") {
        return Cell::value(CellValue::Bool(false));
    }
    if ERROR_LITERALS.contains(&text) {
        return Cell::value(CellValue::Error(text.to_string()));
    }
    Cell::value(CellValue::Text(text.to_string()))
}

/// The text that reproduces a cell when edited and confirmed.
pub fn edit_text(cell: &Cell, date1904: bool) -> String {
    if let Some(formula) = &cell.formula {
        return format!("={formula}");
    }
    match &cell.value {
        CellValue::Empty => String::new(),
        CellValue::Number(number) if cell.is_date => date::format_serial(*number, date1904),
        CellValue::Number(number) => number.to_string(),
        CellValue::Bool(flag) => if *flag { "TRUE" } else { "FALSE" }.to_string(),
        CellValue::Error(text) => text.clone(),
        CellValue::Text(text) => {
            let reads_as_text =
                parse_input(text, false, date1904) == Cell::value(CellValue::Text(text.clone()));
            if reads_as_text {
                text.clone()
            } else {
                format!("'{text}")
            }
        }
    }
}

/// The text shown in the grid for a cell.
pub fn display_text(cell: &Cell, date1904: bool, width: usize) -> String {
    match &cell.value {
        CellValue::Empty => match &cell.formula {
            // A newly typed formula has no result until a spreadsheet engine computes one.
            Some(formula) => format!("={formula}"),
            None => String::new(),
        },
        CellValue::Number(number) if cell.is_date => date_text(*number, date1904, width),
        CellValue::Number(number) => number_text(*number, width),
        CellValue::Text(text) => text.clone(),
        CellValue::Bool(flag) => if *flag { "TRUE" } else { "FALSE" }.to_string(),
        CellValue::Error(text) => text.clone(),
    }
}

/// Room for a cell's whole text when searching, unlike the narrow columns on screen.
const SEARCH_WIDTH: usize = 64;

/// Whether a cell shows, or its formula holds, text already in lower case. A large sheet has millions
/// of cells to try, so text is compared where it lies and other values are only formatted if needed.
pub fn cell_contains(cell: &Cell, lowercase_needle: &str, date1904: bool) -> bool {
    let in_formula = cell
        .formula
        .as_ref()
        .is_some_and(|formula| contains_ignoring_case(&format!("={formula}"), lowercase_needle));
    in_formula
        || match &cell.value {
            CellValue::Empty => false,
            CellValue::Text(text) | CellValue::Error(text) => {
                contains_ignoring_case(text, lowercase_needle)
            }
            CellValue::Number(number)
                if number.is_finite() && !could_be_in_a_number(lowercase_needle) =>
            {
                false
            }
            CellValue::Number(_) => {
                contains_ignoring_case(
                    &display_text(cell, date1904, SEARCH_WIDTH),
                    lowercase_needle,
                ) || contains_ignoring_case(&edit_text(cell, date1904), lowercase_needle)
            }
            CellValue::Bool(_) => contains_ignoring_case(
                &display_text(cell, date1904, SEARCH_WIDTH),
                lowercase_needle,
            ),
        }
}

/// Whether text could appear in a formatted number or date, which saves formatting millions of them
/// to look for a word.
fn could_be_in_a_number(lowercase_needle: &str) -> bool {
    lowercase_needle
        .bytes()
        .all(|byte| byte.is_ascii_digit() || b"-+.:e #".contains(&byte))
}

fn contains_ignoring_case(text: &str, lowercase_needle: &str) -> bool {
    if text.is_ascii() && lowercase_needle.is_ascii() {
        let needle = lowercase_needle.as_bytes();
        needle.is_empty()
            || text
                .as_bytes()
                .windows(needle.len())
                .any(|window| window.eq_ignore_ascii_case(needle))
    } else {
        text.to_lowercase().contains(lowercase_needle)
    }
}

/// Numbers sit against the right edge of their column, as in Excel.
pub fn is_right_aligned(cell: &Cell) -> bool {
    matches!(cell.value, CellValue::Number(_))
}

/// The longest significant-digit count Excel shows in a General cell.
const GENERAL_PRECISION: usize = 11;

fn date_text(serial: f64, date1904: bool, width: usize) -> String {
    let full = date::format_serial(serial, date1904);
    if full.len() <= width {
        return full;
    }
    // The date matters more than the time when both cannot fit.
    let date = full.split(' ').next().unwrap_or(&full);
    if date.len() <= width {
        date.to_string()
    } else {
        "#".repeat(width)
    }
}

/// Formats a number with as many significant digits as fit, like Excel's General format.
fn number_text(number: f64, width: usize) -> String {
    (1..=GENERAL_PRECISION)
        .rev()
        .map(|precision| general_format(number, precision))
        .find(|text| text.len() <= width)
        .unwrap_or_else(|| "#".repeat(width))
}

fn general_format(number: f64, precision: usize) -> String {
    if number == 0.0 {
        return "0".to_string();
    }
    let magnitude = number.abs();
    if !(1e-4..1e11).contains(&magnitude) {
        let text = format!("{:.*e}", precision - 1, number);
        let (mantissa, exponent) = text.split_once('e').unwrap_or((&text, "0"));
        return format!("{}e{exponent}", trim_fraction(mantissa));
    }
    let integer_digits = magnitude.log10().floor() as i32 + 1;
    let decimals = (precision as i32 - integer_digits).max(0) as usize;
    trim_fraction(&format!("{number:.decimals$}")).to_string()
}

fn trim_fraction(text: &str) -> &str {
    if text.contains('.') {
        text.trim_end_matches('0').trim_end_matches('.')
    } else {
        text
    }
}
