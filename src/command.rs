//! The vim-style keys: which sequences of key presses make up which commands.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::address::MAX_ROWS;
use crate::keys::is_ctrl;
use crate::motion::Motion;

/// What `d`, `y` and `c` do to the cells a motion or selection covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operator {
    Delete,
    Yank,
    Change,
}

impl Operator {
    fn key(self) -> char {
        match self {
            Self::Delete => 'd',
            Self::Yank => 'y',
            Self::Change => 'c',
        }
    }

    fn from_key(c: char) -> Option<Self> {
        match c {
            'd' => Some(Self::Delete),
            'y' => Some(Self::Yank),
            'c' => Some(Self::Change),
            _ => None,
        }
    }
}

/// A whole command, typed in normal or visual mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Move {
        motion: Motion,
        count: Option<u32>,
    },
    OperateOnMotion {
        operator: Operator,
        motion: Motion,
        count: Option<u32>,
    },
    OperateOnRows {
        operator: Operator,
        count: u32,
    },
    OperateOnSelection(Operator),
    SwitchSheet(isize),
    GoToSheet(u32),
    SaveIfEditedAndQuit,
    QuitWithoutAsking,
    ScrollCursorToMiddle,
    ScrollCursorToTop,
    ScrollCursorToBottom,
    Reload,
    Quit,
    Save,
    OpenFile,
    BeginGoTo,
    BeginSearch,
    OpenHelp,
    FindNext,
    FindPrevious,
    Undo,
    Redo,
    ClearHere,
    Paste {
        before: bool,
    },
    ClearRight {
        count: u32,
    },
    ClearLeft {
        count: u32,
    },
    ClearToRowEnd,
    SelectCells,
    SelectRows,
    StopSelecting,
    SwapSelectionCorners,
    EditAtEnd,
    EditAtStart,
    Replace,
    ChangeToRowEnd,
    EditBelow,
    EditAbove,
    BeginFormula,
    CancelLoad,
}

/// The part of a command typed so far.
#[derive(Debug, Default)]
pub struct Grammar {
    /// The count typed so far, as in `5j`.
    count: Option<u32>,
    /// The first key of a two-key command, as in `gg`.
    prefix: Option<char>,
    /// The operator waiting for its motion.
    operator: Option<Operator>,
    operator_count: Option<u32>,
}

impl Grammar {
    /// Takes a key pressed in normal mode, and returns the command if that finishes one.
    pub fn normal(&mut self, key: KeyEvent) -> Option<Command> {
        if self.count_digit(key) {
            return None;
        }
        let count = match (self.count.take(), self.operator_count.take()) {
            (Some(a), Some(b)) => Some(a.saturating_mul(b).min(MAX_ROWS)),
            (a, b) => a.or(b),
        };
        if let Some(prefix) = self.prefix.take() {
            let operator = self.operator.take();
            return match (prefix_command(prefix, key, count), operator) {
                (Some(Command::Move { motion, count }), Some(operator)) => {
                    Some(Command::OperateOnMotion {
                        operator,
                        motion,
                        count,
                    })
                }
                (command, _) => command,
            };
        }
        if let (KeyCode::Char(prefix @ ('g' | 'z' | 'Z')), false) = (key.code, is_ctrl(key)) {
            self.prefix = Some(prefix);
            self.count = count;
            return None;
        }
        if let Some(motion) = motion_of(key) {
            return Some(match self.operator.take() {
                Some(operator) => Command::OperateOnMotion {
                    operator,
                    motion,
                    count,
                },
                None => Command::Move { motion, count },
            });
        }
        if let Some(operator) = self.operator.take() {
            return (key.code == KeyCode::Char(operator.key())).then_some(Command::OperateOnRows {
                operator,
                count: count.unwrap_or(1),
            });
        }
        if let KeyCode::Char(c) = key.code
            && !is_ctrl(key)
            && let Some(operator) = Operator::from_key(c)
        {
            self.operator = Some(operator);
            self.operator_count = count;
            return None;
        }
        normal_command(key, count)
    }

    /// Takes a key pressed during a selection of cells or, with `rows`, of whole rows, and returns
    /// the command if that finishes one.
    pub fn visual(&mut self, key: KeyEvent, rows: bool) -> Option<Command> {
        if self.count_digit(key) {
            return None;
        }
        let count = self.count.take();
        if let Some(prefix) = self.prefix.take() {
            return match prefix_command(prefix, key, count) {
                Some(Command::SwitchSheet(_) | Command::GoToSheet(_)) => None,
                command => command,
            };
        }
        if let (KeyCode::Char(prefix @ ('g' | 'z')), false) = (key.code, is_ctrl(key)) {
            self.prefix = Some(prefix);
            self.count = count;
            return None;
        }
        if let Some(motion) = motion_of(key) {
            return Some(Command::Move { motion, count });
        }
        match key.code {
            KeyCode::Esc => Some(Command::StopSelecting),
            KeyCode::Char('v') if !rows => Some(Command::StopSelecting),
            KeyCode::Char('V') if rows => Some(Command::StopSelecting),
            KeyCode::Char('c') if is_ctrl(key) => Some(Command::StopSelecting),
            KeyCode::Char('?') | KeyCode::F(1) => Some(Command::OpenHelp),
            KeyCode::Char('v') => Some(Command::SelectCells),
            KeyCode::Char('V') => Some(Command::SelectRows),
            KeyCode::Char('o' | 'O') => Some(Command::SwapSelectionCorners),
            KeyCode::Char('d' | 'x') | KeyCode::Delete => {
                Some(Command::OperateOnSelection(Operator::Delete))
            }
            KeyCode::Char('y') => Some(Command::OperateOnSelection(Operator::Yank)),
            KeyCode::Char('c' | 's') => Some(Command::OperateOnSelection(Operator::Change)),
            _ => None,
        }
    }

    /// The keys of a command still being typed, as vim's `showcmd` shows them.
    pub fn pending_keys(&self) -> String {
        let count = self
            .operator_count
            .or(self.count)
            .map_or(String::new(), |n| n.to_string());
        let operator = self
            .operator
            .map_or(String::new(), |operator| operator.key().to_string());
        let inner = match (self.operator_count, self.count) {
            (Some(_), Some(n)) => n.to_string(),
            _ => String::new(),
        };
        let prefix = self
            .prefix
            .map_or(String::new(), |prefix| prefix.to_string());
        format!("{count}{operator}{inner}{prefix}")
    }

    /// Drops a half-typed command.
    pub fn forget(&mut self) {
        *self = Self::default();
    }

    /// Collects count digits, and says whether the key was one.
    fn count_digit(&mut self, key: KeyEvent) -> bool {
        let KeyCode::Char(c @ '0'..='9') = key.code else {
            return false;
        };
        // A leading zero is the motion to the first column, not a count.
        if c == '0' && self.count.is_none() {
            return false;
        }
        let digit = c.to_digit(10).unwrap_or(0);
        let count = self
            .count
            .unwrap_or(0)
            .saturating_mul(10)
            .saturating_add(digit);
        self.count = Some(count.min(MAX_ROWS));
        true
    }
}

/// The motion a key stands for, if any.
fn motion_of(key: KeyEvent) -> Option<Motion> {
    let ctrl = is_ctrl(key);
    Some(match key.code {
        KeyCode::Char('0') if !ctrl => Motion::RowStart,
        KeyCode::Char('|') if !ctrl => Motion::Column,
        KeyCode::Char('^') if !ctrl => Motion::FirstFilled,
        KeyCode::Char('$') if !ctrl => Motion::LastFilled,
        KeyCode::Char('G') if !ctrl => Motion::LastRow,
        KeyCode::Char('w') if !ctrl => Motion::NextRunStart,
        KeyCode::Char('e') if !ctrl => Motion::NextRunEnd,
        KeyCode::Char('b') if !ctrl => Motion::PreviousRunStart,
        KeyCode::Char('}') if !ctrl => Motion::NextGap,
        KeyCode::Char('{') if !ctrl => Motion::PreviousGap,
        KeyCode::Char('H') if !ctrl => Motion::ScreenTop,
        KeyCode::Char('M') if !ctrl => Motion::ScreenMiddle,
        KeyCode::Char('L') if !ctrl => Motion::ScreenBottom,
        KeyCode::Char('d') if ctrl => Motion::HalfViewDown,
        KeyCode::Char('u') if ctrl => Motion::HalfViewUp,
        KeyCode::Char('f') if ctrl => Motion::ViewDown,
        KeyCode::Char('b') if ctrl => Motion::ViewUp,
        KeyCode::Char('h') if !ctrl => Motion::Left,
        KeyCode::Char('l') if !ctrl => Motion::Right,
        KeyCode::Char('k') if !ctrl => Motion::Up,
        KeyCode::Char('j') if !ctrl => Motion::Down,
        KeyCode::Left => Motion::Left,
        KeyCode::Right => Motion::Right,
        KeyCode::Up => Motion::Up,
        KeyCode::Down => Motion::Down,
        KeyCode::Tab => Motion::TabRight,
        KeyCode::BackTab => Motion::TabLeft,
        KeyCode::Enter => Motion::Down,
        KeyCode::PageDown if !ctrl => Motion::PageDown,
        KeyCode::PageUp if !ctrl => Motion::PageUp,
        KeyCode::Home if ctrl => Motion::Origin,
        KeyCode::Home => Motion::RowStart,
        KeyCode::End if ctrl => Motion::LastCell,
        KeyCode::End => Motion::RowEnd,
        _ => return None,
    })
}

/// The command a two-key sequence stands for, if any.
fn prefix_command(prefix: char, key: KeyEvent, count: Option<u32>) -> Option<Command> {
    match (prefix, key.code) {
        ('g', KeyCode::Char('g')) => Some(Command::Move {
            motion: Motion::FirstRow,
            count,
        }),
        ('g', KeyCode::Char('t')) => Some(match count {
            Some(number) => Command::GoToSheet(number),
            None => Command::SwitchSheet(1),
        }),
        ('g', KeyCode::Char('T')) => Some(Command::SwitchSheet(-1)),
        ('Z', KeyCode::Char('Z')) => Some(Command::SaveIfEditedAndQuit),
        ('Z', KeyCode::Char('Q')) => Some(Command::QuitWithoutAsking),
        ('z', KeyCode::Char('z')) => Some(Command::ScrollCursorToMiddle),
        ('z', KeyCode::Char('t')) => Some(Command::ScrollCursorToTop),
        ('z', KeyCode::Char('b')) => Some(Command::ScrollCursorToBottom),
        _ => None,
    }
}

/// The command a single key stands for in normal mode, if any.
fn normal_command(key: KeyEvent, count: Option<u32>) -> Option<Command> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);
    let times = count.unwrap_or(1);
    Some(match key.code {
        KeyCode::PageDown if ctrl => Command::SwitchSheet(1),
        KeyCode::PageUp if ctrl => Command::SwitchSheet(-1),
        KeyCode::F(5) => Command::Reload,
        KeyCode::F(6) if shift => Command::SwitchSheet(-1),
        KeyCode::F(6) => Command::SwitchSheet(1),
        KeyCode::Char('q' | 'c') if ctrl => Command::Quit,
        KeyCode::Char('q') => Command::Quit,
        KeyCode::Char('s') if ctrl => Command::Save,
        KeyCode::Char('o') if ctrl => Command::OpenFile,
        KeyCode::Char('g') if ctrl => Command::BeginGoTo,
        KeyCode::Char('/') if !ctrl => Command::BeginSearch,
        KeyCode::Char('?') if !ctrl => Command::OpenHelp,
        KeyCode::F(1) => Command::OpenHelp,
        KeyCode::Char('n') if !ctrl => Command::FindNext,
        KeyCode::Char('N') if !ctrl => Command::FindPrevious,
        KeyCode::Char('z') if ctrl => Command::Undo,
        KeyCode::Char('y') if ctrl => Command::Redo,
        KeyCode::Delete | KeyCode::Backspace => Command::ClearHere,
        KeyCode::Char('p') if !ctrl => Command::Paste { before: false },
        KeyCode::Char('P') if !ctrl => Command::Paste { before: true },
        KeyCode::Char('u') if !ctrl => Command::Undo,
        KeyCode::Char('r') if ctrl => Command::Redo,
        KeyCode::Char('x') if !ctrl => Command::ClearRight { count: times },
        KeyCode::Char('X') if !ctrl => Command::ClearLeft { count: times },
        KeyCode::Char('D') if !ctrl => Command::ClearToRowEnd,
        KeyCode::Char('v') if is_ctrl(key) => Command::SelectCells,
        KeyCode::Char('v') if !ctrl => Command::SelectCells,
        KeyCode::Char('V') if !ctrl => Command::SelectRows,
        KeyCode::F(2) => Command::EditAtEnd,
        KeyCode::Char('a' | 'A') if !ctrl => Command::EditAtEnd,
        KeyCode::Char('i' | 'I') if !ctrl => Command::EditAtStart,
        KeyCode::Char('s') if !ctrl => Command::Replace,
        KeyCode::Char('S') if !ctrl => Command::OperateOnRows {
            operator: Operator::Change,
            count: times,
        },
        KeyCode::Char('C') if !ctrl => Command::ChangeToRowEnd,
        KeyCode::Char('o') if !ctrl => Command::EditBelow,
        KeyCode::Char('O') if !ctrl => Command::EditAbove,
        KeyCode::Char('=') if !ctrl => Command::BeginFormula,
        KeyCode::Esc => Command::CancelLoad,
        _ => return None,
    })
}
