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
