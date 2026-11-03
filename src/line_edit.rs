//! One line of text being typed, and the caret in it.

use crossterm::event::{KeyCode, KeyEvent};

use crate::keys::is_text_input;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LineEdit {
    chars: Vec<char>,
    caret: usize,
}

impl LineEdit {
    /// The given text, with the caret at its end.
    pub fn new(text: &str) -> Self {
        let chars: Vec<char> = text.chars().collect();
        Self {
            caret: chars.len(),
            chars,
        }
    }

    pub fn text(&self) -> String {
        self.chars.iter().collect()
    }

    /// The caret's position, in characters.
    pub fn caret(&self) -> usize {
        self.caret
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char(c) if is_text_input(key) => {
                self.chars.insert(self.caret, c);
                self.caret += 1;
            }
            KeyCode::Left => self.caret = self.caret.saturating_sub(1),
            _ => {}
        }
    }
}
