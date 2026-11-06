//! What the app has to tell the user, and news held back until it can be read.

/// A message for the user.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Said {
    text: String,
    is_error: bool,
}

/// The latest message for the status line, and news of background work waiting to be shown.
#[derive(Debug, Default)]
pub struct Status {
    latest: Option<Said>,
    held: Option<Said>,
}

impl Status {
    /// Says something that isn't going wrong.
    pub fn say(&mut self, text: impl Into<String>) {
        self.set(text.into(), false);
    }

    pub fn report_error(&mut self, text: impl Into<String>) {
        self.set(text.into(), true);
    }

    /// Forgets the latest message, keeping held news.
    pub fn clear(&mut self) {
        self.latest = None;
    }

    /// Tells the user about background work now if they can read it, or holds it until
    /// `catch_up` says they can.
    pub fn announce(&mut self, text: impl Into<String>, is_error: bool, readable: bool) {
        let said = Said {
            text: text.into(),
            is_error,
        };
        if readable {
            self.latest = Some(said);
        } else {
            self.held = Some(said);
        }
    }

    /// Shows held news once the user can read it, unless something else is being said.
    pub fn catch_up(&mut self, readable: bool) {
        if readable && self.latest.is_none() {
            self.latest = self.held.take();
        }
    }

    /// The latest message, empty if there's none.
    pub fn text(&self) -> &str {
        self.latest.as_ref().map_or("", |said| &said.text)
    }

    /// Whether the latest message reports something going wrong.
    pub fn is_error(&self) -> bool {
        self.latest.as_ref().is_some_and(|said| said.is_error)
    }

    fn set(&mut self, text: String, is_error: bool) {
        self.latest = (!text.is_empty()).then_some(Said { text, is_error });
    }
}
