use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use tuicalc::line_edit::LineEdit;

fn press(line: &mut LineEdit, code: KeyCode) {
    line.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
}

fn ctrl(line: &mut LineEdit, code: KeyCode) {
    line.handle_key(KeyEvent::new(code, KeyModifiers::CONTROL));
}

#[test]
fn typing_goes_in_at_the_caret() {
    let mut line = LineEdit::new("héllo");
    assert_eq!(line.caret(), 5, "the caret starts at the end");

    press(&mut line, KeyCode::Left);
    press(&mut line, KeyCode::Char('!'));

    assert_eq!(line.text(), "héll!o");
    assert_eq!(line.caret(), 5);
}

#[test]
fn the_caret_moves_within_the_text_and_stops_at_its_ends() {
    let mut line = LineEdit::new("abc");

    press(&mut line, KeyCode::Right);
    assert_eq!(line.caret(), 3);
    press(&mut line, KeyCode::Home);
    assert_eq!(line.caret(), 0);
    press(&mut line, KeyCode::Left);
    assert_eq!(line.caret(), 0);
    press(&mut line, KeyCode::Right);
    assert_eq!(line.caret(), 1);
    press(&mut line, KeyCode::End);
    assert_eq!(line.caret(), 3);
}

#[test]
fn backspace_and_delete_take_the_character_on_either_side_of_the_caret() {
    let mut line = LineEdit::new("abcd");
    press(&mut line, KeyCode::Left);
    press(&mut line, KeyCode::Left);

    press(&mut line, KeyCode::Backspace);
    assert_eq!(line.text(), "acd");
    assert_eq!(line.caret(), 1);

    press(&mut line, KeyCode::Delete);
    assert_eq!(line.text(), "ad");
    assert_eq!(line.caret(), 1);

    press(&mut line, KeyCode::Home);
    press(&mut line, KeyCode::Backspace);
    press(&mut line, KeyCode::End);
    press(&mut line, KeyCode::Delete);
    assert_eq!(line.text(), "ad");
}

#[test]
fn ctrl_u_deletes_back_to_the_start() {
    let mut line = LineEdit::new("one two");
    press(&mut line, KeyCode::Left);

    ctrl(&mut line, KeyCode::Char('u'));

    assert_eq!(line.text(), "o");
    assert_eq!(line.caret(), 0);
}

#[test]
fn ctrl_w_deletes_the_word_before_the_caret_and_the_spaces_after_it() {
    let mut line = LineEdit::new("one two  ");

    ctrl(&mut line, KeyCode::Char('w'));

    assert_eq!(line.text(), "one ");
    assert_eq!(line.caret(), 4);
}

#[test]
fn ctrl_letters_are_not_text_but_altgr_characters_are() {
    let mut line = LineEdit::default();

    ctrl(&mut line, KeyCode::Char('x'));
    assert_eq!(line.text(), "");

    line.handle_key(KeyEvent::new(
        KeyCode::Char('@'),
        KeyModifiers::CONTROL | KeyModifiers::ALT,
    ));
    assert_eq!(line.text(), "@");
}
