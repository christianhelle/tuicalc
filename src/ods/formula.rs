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
