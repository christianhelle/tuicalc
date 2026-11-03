use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
use tuicalc::command::{Command, Grammar, Operator};
use tuicalc::motion::Motion;

fn plain(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
}

fn ctrl(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

/// What typing these keys in normal mode ends with, if the last one finishes a command.
fn normal(keys: &str) -> Option<Command> {
    let mut grammar = Grammar::default();
    let mut last = None;
    for c in keys.chars() {
        last = grammar.normal(plain(c));
    }
    last
}

/// What typing these keys while selecting cells ends with.
fn visual(keys: &str) -> Option<Command> {
    let mut grammar = Grammar::default();
    let mut last = None;
    for c in keys.chars() {
        last = grammar.visual(plain(c), false);
    }
    last
}

fn move_by(motion: Motion, count: Option<u32>) -> Option<Command> {
    Some(Command::Move { motion, count })
}

#[test]
fn motions_take_a_count() {
    assert_eq!(normal("j"), move_by(Motion::Down, None));
    assert_eq!(normal("5j"), move_by(Motion::Down, Some(5)));
    assert_eq!(normal("12l"), move_by(Motion::Right, Some(12)));
    assert_eq!(normal("0"), move_by(Motion::RowStart, None));
    assert_eq!(
        normal("10"),
        None,
        "a zero after a digit is part of the count"
    );
    assert_eq!(normal("gg"), move_by(Motion::FirstRow, None));
    assert_eq!(normal("3gg"), move_by(Motion::FirstRow, Some(3)));
}

#[test]
fn every_motion_has_its_key() {
    let cases = [
        ('h', Motion::Left),
        ('l', Motion::Right),
        ('k', Motion::Up),
        ('j', Motion::Down),
        ('0', Motion::RowStart),
        ('|', Motion::Column),
        ('^', Motion::FirstFilled),
        ('$', Motion::LastFilled),
        ('G', Motion::LastRow),
        ('w', Motion::NextRunStart),
        ('e', Motion::NextRunEnd),
        ('b', Motion::PreviousRunStart),
        ('}', Motion::NextGap),
        ('{', Motion::PreviousGap),
        ('H', Motion::ScreenTop),
        ('M', Motion::ScreenMiddle),
        ('L', Motion::ScreenBottom),
    ];
    for (c, motion) in cases {
        assert_eq!(normal(&c.to_string()), move_by(motion, None), "{c}");
    }
    let keys = [
        (ctrl('d'), Motion::HalfViewDown),
        (ctrl('u'), Motion::HalfViewUp),
        (ctrl('f'), Motion::ViewDown),
        (ctrl('b'), Motion::ViewUp),
        (key(KeyCode::Left), Motion::Left),
        (key(KeyCode::Right), Motion::Right),
        (key(KeyCode::Up), Motion::Up),
        (key(KeyCode::Down), Motion::Down),
        (key(KeyCode::Tab), Motion::TabRight),
        (key(KeyCode::BackTab), Motion::TabLeft),
        (key(KeyCode::Enter), Motion::Down),
        (key(KeyCode::PageDown), Motion::PageDown),
        (key(KeyCode::PageUp), Motion::PageUp),
        (key(KeyCode::Home), Motion::RowStart),
        (key(KeyCode::End), Motion::RowEnd),
        (
            KeyEvent::new(KeyCode::Home, KeyModifiers::CONTROL),
            Motion::Origin,
        ),
        (
            KeyEvent::new(KeyCode::End, KeyModifiers::CONTROL),
            Motion::LastCell,
        ),
    ];
    for (pressed, motion) in keys {
        let mut grammar = Grammar::default();
        assert_eq!(
            grammar.normal(pressed),
            move_by(motion, None),
            "{pressed:?}"
        );
    }
}

#[test]
fn operators_wait_for_a_motion_and_multiply_counts() {
    assert_eq!(normal("d"), None);
    assert_eq!(
        normal("dw"),
        Some(Command::OperateOnMotion {
            operator: Operator::Delete,
            motion: Motion::NextRunStart,
            count: None,
        })
    );
    assert_eq!(
        normal("2y3j"),
        Some(Command::OperateOnMotion {
            operator: Operator::Yank,
            motion: Motion::Down,
            count: Some(6),
        })
    );
    assert_eq!(
        normal("cgg"),
        Some(Command::OperateOnMotion {
            operator: Operator::Change,
            motion: Motion::FirstRow,
            count: None,
        })
    );
}

#[test]
fn doubling_an_operator_takes_whole_rows() {
    assert_eq!(
        normal("dd"),
        Some(Command::OperateOnRows {
            operator: Operator::Delete,
            count: 1,
        })
    );
    assert_eq!(
        normal("3yy"),
        Some(Command::OperateOnRows {
            operator: Operator::Yank,
            count: 3,
        })
    );
    assert_eq!(
        normal("S"),
        Some(Command::OperateOnRows {
            operator: Operator::Change,
            count: 1,
        })
    );
}

#[test]
fn any_other_key_drops_an_operator_and_its_own_meaning() {
    let mut grammar = Grammar::default();
    assert_eq!(grammar.normal(plain('d')), None);
    assert_eq!(grammar.normal(plain('p')), None);
    assert_eq!(grammar.pending_keys(), "");
    assert_eq!(grammar.normal(plain('j')), move_by(Motion::Down, None));
}

#[test]
fn a_prefix_command_after_an_operator_runs_without_it() {
    assert_eq!(normal("dzz"), Some(Command::ScrollCursorToMiddle));
    let mut grammar = Grammar::default();
    for c in "dzz".chars() {
        grammar.normal(plain(c));
    }
    assert_eq!(grammar.normal(plain('j')), move_by(Motion::Down, None));
}

#[test]
fn two_key_commands_finish_on_their_second_key() {
    let cases = [
        ("gt", Command::SwitchSheet(1)),
        ("gT", Command::SwitchSheet(-1)),
        ("3gt", Command::GoToSheet(3)),
        ("ZZ", Command::SaveIfEditedAndQuit),
        ("ZQ", Command::QuitWithoutAsking),
        ("zz", Command::ScrollCursorToMiddle),
        ("zt", Command::ScrollCursorToTop),
        ("zb", Command::ScrollCursorToBottom),
    ];
    for (keys, command) in cases {
        assert_eq!(normal(keys), Some(command), "{keys}");
    }
    assert_eq!(normal("gx"), None);
    assert_eq!(normal("Zx"), None);
}

#[test]
fn single_key_commands_have_their_keys() {
    let cases = [
        ('q', Command::Quit),
        ('/', Command::BeginSearch),
        ('?', Command::OpenHelp),
        ('n', Command::FindNext),
        ('N', Command::FindPrevious),
        ('u', Command::Undo),
        ('p', Command::Paste { before: false }),
        ('P', Command::Paste { before: true }),
        ('x', Command::ClearRight { count: 1 }),
        ('X', Command::ClearLeft { count: 1 }),
        ('D', Command::ClearToRowEnd),
        ('v', Command::SelectCells),
        ('V', Command::SelectRows),
        ('i', Command::EditAtStart),
        ('I', Command::EditAtStart),
        ('a', Command::EditAtEnd),
        ('A', Command::EditAtEnd),
        ('s', Command::Replace),
        ('C', Command::ChangeToRowEnd),
        ('o', Command::EditBelow),
        ('O', Command::EditAbove),
        ('=', Command::BeginFormula),
    ];
    for (c, command) in cases {
        assert_eq!(normal(&c.to_string()), Some(command), "{c}");
    }
    assert_eq!(normal("3x"), Some(Command::ClearRight { count: 3 }));
    assert_eq!(normal("2X"), Some(Command::ClearLeft { count: 2 }));
}

#[test]
fn modified_and_function_keys_have_their_commands() {
    let shift_f6 = KeyEvent::new(KeyCode::F(6), KeyModifiers::SHIFT);
    let ctrl_page_down = KeyEvent::new(KeyCode::PageDown, KeyModifiers::CONTROL);
    let ctrl_page_up = KeyEvent::new(KeyCode::PageUp, KeyModifiers::CONTROL);
    let cases = [
        (ctrl('q'), Command::Quit),
        (ctrl('c'), Command::Quit),
        (ctrl('s'), Command::Save),
        (ctrl('o'), Command::OpenFile),
        (ctrl('g'), Command::BeginGoTo),
        (ctrl('z'), Command::Undo),
        (ctrl('y'), Command::Redo),
        (ctrl('r'), Command::Redo),
        (ctrl('v'), Command::SelectCells),
        (ctrl_page_down, Command::SwitchSheet(1)),
        (ctrl_page_up, Command::SwitchSheet(-1)),
        (key(KeyCode::F(1)), Command::OpenHelp),
        (key(KeyCode::F(2)), Command::EditAtEnd),
        (key(KeyCode::F(5)), Command::Reload),
        (key(KeyCode::F(6)), Command::SwitchSheet(1)),
        (shift_f6, Command::SwitchSheet(-1)),
        (key(KeyCode::Delete), Command::ClearHere),
        (key(KeyCode::Backspace), Command::ClearHere),
        (key(KeyCode::Esc), Command::CancelLoad),
    ];
    for (pressed, command) in cases {
        let mut grammar = Grammar::default();
        assert_eq!(grammar.normal(pressed), Some(command), "{pressed:?}");
    }
}

#[test]
fn altgr_characters_count_as_text_not_as_ctrl_commands() {
    let mut grammar = Grammar::default();
    let altgr_q = KeyEvent {
        code: KeyCode::Char('j'),
        modifiers: KeyModifiers::CONTROL | KeyModifiers::ALT,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    };
    assert_eq!(grammar.normal(altgr_q), move_by(Motion::Down, None));
}

#[test]
fn the_keys_typed_so_far_read_as_vim_shows_them() {
    let mut grammar = Grammar::default();
    assert_eq!(grammar.pending_keys(), "");
    grammar.normal(plain('2'));
    assert_eq!(grammar.pending_keys(), "2");
    grammar.normal(plain('d'));
    assert_eq!(grammar.pending_keys(), "2d");
    grammar.normal(plain('3'));
    assert_eq!(grammar.pending_keys(), "2d3");
    grammar.normal(plain('j'));
    assert_eq!(grammar.pending_keys(), "");
    grammar.normal(plain('4'));
    grammar.normal(plain('g'));
    assert_eq!(grammar.pending_keys(), "4g");
}

#[test]
fn forgetting_drops_a_half_typed_command() {
    let mut grammar = Grammar::default();
    for c in "2d3g".chars() {
        grammar.normal(plain(c));
    }

    grammar.forget();

    assert_eq!(grammar.pending_keys(), "");
    assert_eq!(grammar.normal(plain('j')), move_by(Motion::Down, None));
}

#[test]
fn while_selecting_motions_extend_and_operators_act_at_once() {
    assert_eq!(visual("3j"), move_by(Motion::Down, Some(3)));
    assert_eq!(visual("gg"), move_by(Motion::FirstRow, None));
    assert_eq!(
        visual("d"),
        Some(Command::OperateOnSelection(Operator::Delete))
    );
    assert_eq!(
        visual("x"),
        Some(Command::OperateOnSelection(Operator::Delete))
    );
    assert_eq!(
        visual("y"),
        Some(Command::OperateOnSelection(Operator::Yank))
    );
    assert_eq!(
        visual("c"),
        Some(Command::OperateOnSelection(Operator::Change))
    );
    assert_eq!(
        visual("s"),
        Some(Command::OperateOnSelection(Operator::Change))
    );
    assert_eq!(visual("o"), Some(Command::SwapSelectionCorners));
    assert_eq!(visual("O"), Some(Command::SwapSelectionCorners));
    assert_eq!(visual("?"), Some(Command::OpenHelp));
    assert_eq!(visual("zz"), Some(Command::ScrollCursorToMiddle));
    assert_eq!(visual("gt"), None, "sheets stay put during a selection");
    assert_eq!(visual("q"), None);
    assert_eq!(visual("ZZ"), None);
}

#[test]
fn the_selection_keys_toggle_or_change_its_kind() {
    let mut grammar = Grammar::default();
    assert_eq!(
        grammar.visual(plain('v'), false),
        Some(Command::StopSelecting)
    );
    assert_eq!(
        grammar.visual(plain('V'), true),
        Some(Command::StopSelecting)
    );
    assert_eq!(grammar.visual(plain('V'), false), Some(Command::SelectRows));
    assert_eq!(grammar.visual(plain('v'), true), Some(Command::SelectCells));
    assert_eq!(
        grammar.visual(key(KeyCode::Esc), true),
        Some(Command::StopSelecting)
    );
    assert_eq!(
        grammar.visual(ctrl('c'), false),
        Some(Command::StopSelecting)
    );
}
