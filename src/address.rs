//! A1-style cell addressing.

/// Excel's column limit (XFD).
pub const MAX_COLS: u32 = 16_384;
/// Excel's row limit.
pub const MAX_ROWS: u32 = 1_048_576;

/// A zero-based cell position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CellRef {
    pub col: u32,
    pub row: u32,
}

impl CellRef {
    /// Parses a reference such as `B7`.
    pub fn parse(text: &str) -> Option<Self> {
        let split = text.find(|c: char| !c.is_ascii_alphabetic())?;
        let (letters, digits) = text.split_at(split);
        if letters.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let mut col: u32 = 0;
        for byte in letters.bytes() {
            let value = u32::from(byte.to_ascii_uppercase() - b'A') + 1;
            col = col.checked_mul(26)?.checked_add(value)?;
        }
        let row: u32 = digits.parse().ok()?;
        (1..=MAX_COLS)
            .contains(&col)
            .then_some(())
            .filter(|()| (1..=MAX_ROWS).contains(&row))?;
        Some(Self { col: col - 1, row: row - 1 })
    }
}

/// The letters naming a zero-based column.
pub fn column_name(col: u32) -> String {
    let mut remaining = col + 1;
    let mut letters = Vec::new();
    while remaining > 0 {
        remaining -= 1;
        letters.push(b'A' + (remaining % 26) as u8);
        remaining /= 26;
    }
    letters.reverse();
    String::from_utf8(letters).expect("column letters are ascii")
}

impl std::fmt::Display for CellRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}{}", column_name(self.col), self.row + 1)
    }
}
