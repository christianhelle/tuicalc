//! Conversions between cell contents and the text a user sees and types.

use crate::date;
use crate::model::{Cell, CellValue};

const ERROR_LITERALS: [&str; 7] =
    ["#N/A", "#DIV/0!", "#REF!", "#VALUE!", "#NAME?", "#NUM!", "#NULL!"];

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
    if was_date {
        if let Some(serial) = date::parse_serial(text, date1904) {
            return Cell { is_date: true, ..Cell::value(CellValue::Number(serial)) };
        }
    }
    if let Some(number) = text.parse::<f64>().ok().filter(|number| number.is_finite()) {
        return Cell { is_date: was_date, ..Cell::value(CellValue::Number(number)) };
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
