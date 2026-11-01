//! Rewrites formula text when it is applied from a different cell.

use crate::address::{MAX_COLS, MAX_ROWS};

/// Shifts the relative references in `formula`, as Excel does for shared formulas.
pub fn translate(formula: &str, cols: i64, rows: i64) -> String {
    let chars: Vec<char> = formula.chars().collect();
    let mut out = String::with_capacity(formula.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            '"' | '\'' => i = copy_quoted(&chars, i, c, &mut out),
            '[' => i = copy_bracketed(&chars, i, &mut out),
            _ if is_token_char(c) => {
                let end = token_end(&chars, i);
                let token: String = chars[i..end].iter().collect();
                let next = chars.get(end).copied();
                i = end;
                if next == Some(':') && i + 1 < chars.len() {
                    let second_end = token_end(&chars, i + 1);
                    let second: String = chars[i + 1..second_end].iter().collect();
                    let after = chars.get(second_end).copied();
                    if let Some(range) = shift_line_range(&token, &second, cols, rows)
                        .filter(|_| !matches!(after, Some('(' | '!' | '[')))
                    {
                        out.push_str(&range);
                        i = second_end;
                        continue;
                    }
                }
                if matches!(next, Some('(' | '!' | '[')) {
                    out.push_str(&token);
                } else {
                    out.push_str(&shift_cell(&token, cols, rows).unwrap_or(token));
                }
            }
            _ => {
                out.push(c);
                i += 1;
            }
        }
    }
    out
}

fn is_token_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '$' | '_' | '.' | '\\')
}

fn token_end(chars: &[char], start: usize) -> usize {
    let mut end = start;
    while end < chars.len() && is_token_char(chars[end]) {
        end += 1;
    }
    // A sign after a numeric exponent, as in 1.5E+3, belongs to the number.
    let token: String = chars[start..end].iter().collect();
    let numeric_exponent = token.ends_with(['e', 'E'])
        && token.len() > 1
        && token[..token.len() - 1]
            .chars()
            .all(|c| c.is_ascii_digit() || c == '.');
    if numeric_exponent && matches!(chars.get(end), Some('+' | '-')) {
        end += 1;
        while end < chars.len() && chars[end].is_ascii_digit() {
            end += 1;
        }
    }
    end
}

/// Copies a quoted string or sheet name, where a doubled quote is an escape.
fn copy_quoted(chars: &[char], start: usize, quote: char, out: &mut String) -> usize {
    out.push(quote);
    let mut i = start + 1;
    while i < chars.len() {
        out.push(chars[i]);
        if chars[i] == quote {
            if chars.get(i + 1) == Some(&quote) {
                out.push(quote);
                i += 2;
                continue;
            }
            return i + 1;
        }
        i += 1;
    }
    i
}

/// Copies a structured reference, which may nest brackets.
fn copy_bracketed(chars: &[char], start: usize, out: &mut String) -> usize {
    let mut depth = 0;
    let mut i = start;
    while i < chars.len() {
        out.push(chars[i]);
        match chars[i] {
            '[' => depth += 1,
            ']' => {
                depth -= 1;
                if depth == 0 {
                    return i + 1;
                }
            }
            _ => {}
        }
        i += 1;
    }
    i
}

/// Splits `$`-marked letters off the front: `$AB12` gives (true, "AB", "12").
fn split_part(text: &str) -> (bool, &str, &str) {
    let (absolute, rest) = match text.strip_prefix('$') {
        Some(rest) => (true, rest),
        None => (false, text),
    };
    let split = rest
        .find(|c: char| !c.is_ascii_alphabetic())
        .unwrap_or(rest.len());
    let (letters, tail) = rest.split_at(split);
    (absolute, letters, tail)
}

fn column_number(letters: &str) -> Option<i64> {
    if letters.is_empty() || letters.len() > 3 {
        return None;
    }
    let number = letters.bytes().fold(0_i64, |total, byte| {
        total * 26 + i64::from(byte.to_ascii_uppercase() - b'A') + 1
    });
    (number <= i64::from(MAX_COLS)).then_some(number)
}

fn row_number(digits: &str) -> Option<i64> {
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let number: i64 = digits.parse().ok()?;
    (1..=i64::from(MAX_ROWS))
        .contains(&number)
        .then_some(number)
}

fn column_text(absolute: bool, number: i64, shift: i64) -> Option<String> {
    let moved = if absolute { number } else { number + shift };
    (1..=i64::from(MAX_COLS)).contains(&moved).then(|| {
        let name = crate::address::column_name((moved - 1) as u32);
        if absolute { format!("${name}") } else { name }
    })
}

fn row_text(absolute: bool, number: i64, shift: i64) -> Option<String> {
    let moved = if absolute { number } else { number + shift };
    (1..=i64::from(MAX_ROWS)).contains(&moved).then(|| {
        if absolute {
            format!("${moved}")
        } else {
            moved.to_string()
        }
    })
}

/// Returns the shifted form of a single-cell reference, or `None` if `token` is not one.
fn shift_cell(token: &str, cols: i64, rows: i64) -> Option<String> {
    let (column_absolute, letters, tail) = split_part(token);
    let column = column_number(letters)?;
    let (row_absolute, row_digits) = match tail.strip_prefix('$') {
        Some(rest) => (true, rest),
        None => (false, tail),
    };
    let row = row_number(row_digits)?;
    let shifted = column_text(column_absolute, column, cols)
        .zip(row_text(row_absolute, row, rows))
        .map(|(column, row)| format!("{column}{row}"));
    Some(shifted.unwrap_or_else(|| "#REF!".to_string()))
}

/// Shifts a whole-column (`A:B`) or whole-row (`1:2`) range.
fn shift_line_range(first: &str, second: &str, cols: i64, rows: i64) -> Option<String> {
    let parts = (split_part(first), split_part(second));
    let ((absolute_a, letters_a, tail_a), (absolute_b, letters_b, tail_b)) = parts;
    if tail_a.is_empty() && tail_b.is_empty() {
        let a = column_number(letters_a)?;
        let b = column_number(letters_b)?;
        let moved = column_text(absolute_a, a, cols).zip(column_text(absolute_b, b, cols));
        return Some(moved.map_or("#REF!".into(), |(a, b)| format!("{a}:{b}")));
    }
    if letters_a.is_empty() && letters_b.is_empty() {
        let a = row_number(tail_a)?;
        let b = row_number(tail_b)?;
        let moved = row_text(absolute_a, a, rows).zip(row_text(absolute_b, b, rows));
        return Some(moved.map_or("#REF!".into(), |(a, b)| format!("{a}:{b}")));
    }
    None
}
