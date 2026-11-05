//! Converts formulas between OpenFormula, as ODF stores them, and the Excel style the app works in.

const OPEN_FORMULA_PREFIXES: [&str; 3] = ["of:=", "oooc:=", "="];
const EXCEL_PREFIX: &str = "msoxl:=";

/// The text to show for a stored formula, without its `=`. A grammar it doesn't know stays as it is.
pub fn to_excel_style(stored: &str) -> String {
    if let Some(excel) = stored.strip_prefix(EXCEL_PREFIX) {
        return excel.to_string();
    }
    match OPEN_FORMULA_PREFIXES
        .iter()
        .find_map(|prefix| stored.strip_prefix(prefix))
    {
        Some(body) => convert_body(body),
        None => stored.to_string(),
    }
}

fn convert_body(body: &str) -> String {
    let chars: Vec<char> = body.chars().collect();
    let mut out = String::with_capacity(body.len());
    let mut in_array = false;
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            quote @ ('"' | '\'') => i = copy_quoted(&chars, i, quote, &mut out),
            '[' => {
                let end = reference_end(&chars, i);
                let inner: String = chars[i + 1..end].iter().collect();
                match reference_to_excel(&inner) {
                    Some(reference) => out.push_str(&reference),
                    None => {
                        out.push('[');
                        out.push_str(&inner);
                        if end < chars.len() {
                            out.push(']');
                        }
                    }
                }
                i = end + 1;
            }
            '{' => {
                in_array = true;
                out.push('{');
                i += 1;
            }
            '}' => {
                in_array = false;
                out.push('}');
                i += 1;
            }
            ';' => {
                out.push(',');
                i += 1;
            }
            // The intersection of two references, which Excel writes as a space.
            '!' => {
                out.push(' ');
                i += 1;
            }
            // What marks a name in the files Excel writes.
            '$' if chars.get(i + 1) == Some(&'$') => i += 2,
            '|' if in_array => {
                out.push(';');
                i += 1;
            }
            other => {
                out.push(other);
                i += 1;
            }
        }
    }
    out
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

/// The position of the `]` closing the reference opened at `start`, or the end of the text.
fn reference_end(chars: &[char], start: usize) -> usize {
    let mut quoted = false;
    let mut i = start + 1;
    while i < chars.len() {
        match chars[i] {
            '\'' => quoted = !quoted,
            ']' if !quoted => return i,
            _ => {}
        }
        i += 1;
    }
    i
}

/// Splits at the separators that aren't in a quoted sheet name.
fn split_unquoted(text: &str, separator: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut quoted = false;
    let mut from = 0;
    for (index, c) in text.char_indices() {
        if c == '\'' {
            quoted = !quoted;
        } else if c == separator && !quoted {
            parts.push(&text[from..index]);
            from = index + c.len_utf8();
        }
    }
    parts.push(&text[from..]);
    parts
}

/// The sheet name and address of one end of a reference, such as `$Sheet2.C3` or `.C3`.
fn split_end(end: &str) -> (&str, &str) {
    let parts = split_unquoted(end, '.');
    let address = parts[parts.len() - 1];
    let sheet = &end[..end.len() - address.len()];
    let sheet = sheet.strip_suffix('.').unwrap_or(sheet);
    (sheet.strip_prefix('$').unwrap_or(sheet), address)
}

fn is_address(text: &str) -> bool {
    let rest = text.strip_prefix('$').unwrap_or(text);
    let letters = rest.len()
        - rest
            .trim_start_matches(|c: char| c.is_ascii_alphabetic())
            .len();
    let (column, row) = rest.split_at(letters);
    let row = row.strip_prefix('$').unwrap_or(row);
    column.len() <= 3
        && !(column.is_empty() && row.is_empty())
        && row.bytes().all(|b| b.is_ascii_digit())
}

/// A reference's text without its brackets in Excel's terms, if it's one the app can work with.
fn reference_to_excel(inner: &str) -> Option<String> {
    let ends = split_unquoted(inner, ':');
    if ends.len() > 2 {
        return None;
    }
    let (first_sheet, first) = split_end(ends[0]);
    let (second_sheet, second) = ends.get(1).map_or(("", ""), |end| split_end(end));
    if first == "#REF!" || second == "#REF!" {
        return Some("#REF!".into());
    }
    let external = |sheet: &str| split_unquoted(sheet, '#').len() > 1;
    if external(first_sheet) || external(second_sheet) || !is_address(first) {
        return None;
    }
    let mut out = String::new();
    if !first_sheet.is_empty() {
        out.push_str(first_sheet);
        if !second_sheet.is_empty() && second_sheet != first_sheet {
            out.push(':');
            out.push_str(second_sheet);
        }
        out.push('!');
    }
    out.push_str(first);
    if ends.len() == 2 {
        if !is_address(second) {
            return None;
        }
        out.push(':');
        out.push_str(second);
    }
    Some(out)
}

/// The stored form of a formula typed without its `=`, using the prefix the file declares for
/// OpenFormula if it does. One already in a grammar stays as it is.
pub fn to_open_formula(typed: &str, prefix: Option<&str>) -> String {
    if has_grammar_prefix(typed) {
        return typed.to_string();
    }
    let chars: Vec<char> = typed.chars().collect();
    let mut out = prefix.map_or_else(|| "=".to_string(), |prefix| format!("{prefix}:="));
    let mut in_array = false;
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '"' => i = copy_quoted(&chars, i, '"', &mut out),
            '[' => {
                let end = reference_end(&chars, i);
                out.extend(&chars[i..(end + 1).min(chars.len())]);
                i = end + 1;
            }
            '\'' => {
                let mut sheet = String::new();
                i = copy_quoted(&chars, i, '\'', &mut sheet);
                if chars.get(i) == Some(&'!') {
                    i = write_reference(&chars, i + 1, &sheet, &mut out);
                } else {
                    out.push_str(&sheet);
                }
            }
            '#' if chars[i..].starts_with(&['#', 'R', 'E', 'F', '!']) => {
                out.push_str("[.#REF!]");
                i += 5;
            }
            '{' => {
                in_array = true;
                out.push('{');
                i += 1;
            }
            '}' => {
                in_array = false;
                out.push('}');
                i += 1;
            }
            ',' => {
                out.push(';');
                i += 1;
            }
            ';' if in_array => {
                out.push('|');
                i += 1;
            }
            ' ' if out.ends_with(']') && starts_reference(&chars[i + 1..]) => {
                out.push('!');
                i += 1;
            }
            c if is_token_char(c) => i = write_token(&chars, i, &mut out),
            other => {
                out.push(other);
                i += 1;
            }
        }
    }
    out
}

/// Whether what follows a space could be the second reference of an intersection.
fn starts_reference(rest: &[char]) -> bool {
    rest.first()
        .is_some_and(|&c| is_token_char(c) || matches!(c, '\'' | '['))
}

/// Whether a formula starts with a grammar's name, such as `ooow:=`.
fn has_grammar_prefix(typed: &str) -> bool {
    typed.split_once(":=").is_some_and(|(name, _)| {
        !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric())
    })
}

fn is_token_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '$' | '_' | '.' | '\\')
}

fn token_end(chars: &[char], start: usize) -> usize {
    let mut end = start;
    while end < chars.len() && is_token_char(chars[end]) {
        end += 1;
    }
    end
}

/// Writes a sheet name the way a reference needs it, quoting one that would be misread.
fn write_sheet(sheet: &str, out: &mut String) {
    out.push('$');
    if sheet.contains('.') && !sheet.starts_with('\'') {
        out.push('\'');
        out.push_str(sheet);
        out.push('\'');
    } else {
        out.push_str(sheet);
    }
}

/// Writes the addresses of a reference starting at `start`, on `sheet` if it isn't empty.
fn write_reference(chars: &[char], start: usize, sheet: &str, out: &mut String) -> usize {
    let first_end = token_end(chars, start);
    let first: String = chars[start..first_end].iter().collect();
    let second = (chars.get(first_end) == Some(&':')).then(|| {
        let end = token_end(chars, first_end + 1);
        (chars[first_end + 1..end].iter().collect::<String>(), end)
    });
    out.push('[');
    if sheet.is_empty() {
        out.push('.');
    } else {
        write_sheet(sheet, out);
        out.push('.');
    }
    out.push_str(&first);
    match second {
        Some((second, end)) if is_address(&second) => {
            out.push_str(":.");
            out.push_str(&second);
            out.push(']');
            end
        }
        _ => {
            out.push(']');
            first_end
        }
    }
}

/// Writes a run of name characters, which may turn out to be a reference, a sheet's name, a
/// function or a name that isn't any of the app's business.
fn write_token(chars: &[char], start: usize, out: &mut String) -> usize {
    let end = token_end(chars, start);
    let token: String = chars[start..end].iter().collect();
    match chars.get(end) {
        Some('!') => return write_reference(chars, end + 1, &token, out),
        Some('(') => {
            out.push_str(&token);
            return end;
        }
        Some(':') => {
            let second_end = token_end(chars, end + 1);
            let second: String = chars[end + 1..second_end].iter().collect();
            if chars.get(second_end) == Some(&'!') {
                return write_sheet_span(chars, second_end + 1, &token, &second, out);
            }
            if is_address(&token) && is_address(&second) {
                out.push_str(&format!("[.{token}:.{second}]"));
                return second_end;
            }
        }
        _ => {}
    }
    if is_cell_address(&token) {
        out.push_str(&format!("[.{token}]"));
    } else {
        out.push_str(&token);
    }
    end
}

/// Writes a reference across sheets, such as `Sheet1:Sheet3!A1:B2`.
fn write_sheet_span(
    chars: &[char],
    start: usize,
    first_sheet: &str,
    last_sheet: &str,
    out: &mut String,
) -> usize {
    let first_end = token_end(chars, start);
    let first: String = chars[start..first_end].iter().collect();
    let mut last = first.clone();
    let mut end = first_end;
    if chars.get(first_end) == Some(&':') {
        end = token_end(chars, first_end + 1);
        last = chars[first_end + 1..end].iter().collect();
    }
    out.push('[');
    write_sheet(first_sheet, out);
    out.push('.');
    out.push_str(&first);
    out.push(':');
    write_sheet(last_sheet, out);
    out.push('.');
    out.push_str(&last);
    out.push(']');
    end
}

/// Whether a token names one cell, rather than a number, a name or a whole line.
fn is_cell_address(text: &str) -> bool {
    let rest = text.strip_prefix('$').unwrap_or(text);
    let digits_at = rest
        .find(|c: char| !c.is_ascii_alphabetic())
        .unwrap_or(rest.len());
    let (column, row) = rest.split_at(digits_at);
    let row = row.strip_prefix('$').unwrap_or(row);
    !column.is_empty() && !row.is_empty() && is_address(text)
}
