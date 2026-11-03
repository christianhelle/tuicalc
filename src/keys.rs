//! What a key press means, whichever modifiers the platform reports it with.

use crossterm::event::{KeyEvent, KeyModifiers};

/// Plain characters, plus AltGr combinations, which Windows reports as Ctrl+Alt.
pub(crate) fn is_text_input(key: KeyEvent) -> bool {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    ctrl == alt
}

/// Ctrl alone, not the Ctrl+Alt that Windows reports for AltGr.
pub(crate) fn is_ctrl(key: KeyEvent) -> bool {
    key.modifiers.contains(KeyModifiers::CONTROL) && !key.modifiers.contains(KeyModifiers::ALT)
}
