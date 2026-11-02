//! What the help dialog lists: one table of keys for each mode.

use crate::app::Mode;

/// The dialog's width, which every table has to fit.
pub const HELP_WIDTH: u16 = 76;
/// The width of the key column.
pub const HELP_KEYS: usize = 25;
/// The longest table that fits a 24-line terminal with the dialog's border.
pub const HELP_ROWS: usize = 22;

pub type HelpTable = &'static [(&'static str, &'static str)];

pub const NORMAL_HELP: HelpTable = &[
    ("h j k l ← ↓ ↑ →", "Move, with a count as in 5j"),
    ("Tab Shift-Tab Enter", "Move right, left, down"),
    ("w e b { }", "Jump between runs of filled and empty cells"),
    ("0 ^ $ {n}|", "First, first filled, last filled, nth column"),
    ("gg G {n}G", "First row, last used row, row n"),
    ("H M L zz zt zb", "Move on screen; place the cursor's row"),
    (
        "Ctrl-D/U/F/B PgDn PgUp",
        "Half page and page, with the view",
    ),
    ("Home End Ctrl-Home/End", "Row edge, A1, last used cell"),
    ("gt gT {n}gt F6 Shift-F6", "Next, previous, nth sheet"),
    ("Ctrl-PgDn Ctrl-PgUp", "Next, previous sheet"),
    ("i I a A F2", "Edit the cell, caret at the start or end"),
    (
        "s S C o O =",
        "Replace, rows, to row end, below, above, formula",
    ),
    ("d y c + motion, dd yy cc", "Delete, copy, change"),
    (
        "x X D Delete Backspace",
        "Clear here, to the left, to row end",
    ),
    ("p P u Ctrl-Z Ctrl-R/Y", "Paste after, before; undo; redo"),
    ("v Ctrl-V V", "Select cells, select rows"),
    ("/ n N", "Find in this sheet, next, previous"),
    ("Ctrl-G", "Go to a cell or row"),
    ("Ctrl-S Ctrl-O F5", "Save, open a file, reload from disk"),
    ("q Ctrl-Q Ctrl-C", "Quit, asking about unsaved changes"),
    (
        "ZZ ZQ ? F1",
        "Save and quit, quit without saving, this help",
    ),
    ("", "Press any key to close"),
];

pub const INSERT_HELP: HelpTable = &[
    ("Esc", "Keep the text and stay"),
    ("Enter ↓ ↑", "Keep the text and move down, up"),
    ("Tab Shift-Tab", "Keep the text and move right, left"),
    ("Ctrl-C", "Cancel the edit"),
    ("Ctrl-U Ctrl-W", "Delete to the start, delete a word"),
    ("← → Home End", "Move the caret"),
    ("Backspace Delete", "Delete a character"),
    ("F1", "This help"),
    ("", "Press any key to close"),
];

pub const VISUAL_HELP: HelpTable = &[
    ("Motions", "Extend the selection"),
    ("Esc Ctrl-C", "Stop selecting"),
    ("v V", "Select cells, select rows"),
    ("o O", "Jump to the other corner"),
    ("d x Delete", "Delete the selection"),
    ("y", "Copy the selection"),
    ("c s", "Change the selection"),
    ("zz zt zb", "Place the cursor's row on screen"),
    ("? F1", "This help"),
    ("", "Press any key to close"),
];

/// The keys that matter in a mode. Only the modes that can open help have their own table.
pub fn table(mode: Mode) -> HelpTable {
    match mode {
        Mode::Insert => INSERT_HELP,
        Mode::Visual | Mode::VisualRow => VISUAL_HELP,
        _ => NORMAL_HELP,
    }
}
