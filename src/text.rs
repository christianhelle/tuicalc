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
            if reads_as_text { text.clone() } else { format!("'{text}") }
        }
    }
}
