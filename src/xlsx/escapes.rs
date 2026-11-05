//! Excel's `_xHHHH_` escapes in text.

/// Whether `text` begins with an Excel escape sequence such as `_x000D_`.
pub fn starts_with_excel_escape(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() >= 7
        && bytes.starts_with(b"_x")
        && bytes[6] == b'_'
        && bytes[2..6].iter().all(u8::is_ascii_hexdigit)
}

/// Replaces Excel `_xHHHH_` sequences with the characters they stand for.
pub fn decode_excel_escapes(text: &str) -> String {
    let mut decoded = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(index) = rest.find("_x") {
        decoded.push_str(&rest[..index]);
        rest = &rest[index..];
        let character = starts_with_excel_escape(rest)
            .then(|| u32::from_str_radix(&rest[2..6], 16).ok())
            .flatten()
            .and_then(char::from_u32);
        if let Some(character) = character {
            decoded.push(character);
            rest = &rest[7..];
        } else {
            decoded.push_str("_x");
            rest = &rest[2..];
        }
    }
    decoded.push_str(rest);
    decoded
}
