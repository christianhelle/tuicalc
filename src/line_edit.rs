//! One line of text being typed, and the caret in it.

use crossterm::event::{KeyCode, KeyEvent};

use unicode_width::UnicodeWidthChar;

use crate::keys::{is_ctrl, is_text_input};

/// The part of a line to show in a limited room.
#[derive(Debug, PartialEq, Eq)]
pub struct Window {
    /// The text from the first visible character on. It may run past the room.
    pub text: String,
    /// How many terminal cells from the start of `text` the caret is.
    pub caret_x: u16,
}

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

    /// Moves the caret to the start of the text.
    pub fn go_home(&mut self) {
        self.caret = 0;
    }

    /// Acts on the keys that edit text. Other keys do nothing.
    pub fn handle_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('u') if is_ctrl(key) => {
                self.chars.drain(..self.caret);
                self.caret = 0;
            }
            KeyCode::Char('w') if is_ctrl(key) => self.delete_word(),
            KeyCode::Char(c) if is_text_input(key) => {
                self.chars.insert(self.caret, c);
                self.caret += 1;
            }
            KeyCode::Left => self.caret = self.caret.saturating_sub(1),
            KeyCode::Right => self.caret = (self.caret + 1).min(self.chars.len()),
            KeyCode::Home => self.caret = 0,
            KeyCode::End => self.caret = self.chars.len(),
            KeyCode::Backspace if self.caret > 0 => {
                self.caret -= 1;
                self.chars.remove(self.caret);
            }
            KeyCode::Delete if self.caret < self.chars.len() => {
                self.chars.remove(self.caret);
            }
            _ => {}
        }
    }

    /// What to show in `room` terminal cells, starting late enough to keep the caret in view.
    pub fn window(&self, room: usize) -> Window {
        let shown: Vec<char> = self
            .chars
            .iter()
            .map(|&c| if c.is_control() { ' ' } else { c })
            .collect();
        let mut start = 0;
        while start < self.caret && width(&shown[start..self.caret]) > room {
            start += 1;
        }
        Window {
            text: shown[start..].iter().collect(),
            caret_x: width(&shown[start..self.caret]) as u16,
        }
    }

    fn delete_word(&mut self) {
        let mut start = self.caret;
        while start > 0 && self.chars[start - 1].is_whitespace() {
            start -= 1;
        }
        while start > 0 && !self.chars[start - 1].is_whitespace() {
            start -= 1;
        }
        self.chars.drain(start..self.caret);
        self.caret = start;
    }
}

fn width(chars: &[char]) -> usize {
    chars.iter().map(|c| c.width().unwrap_or(0)).sum()
}
