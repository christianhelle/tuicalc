use tuicalc::status::Status;

#[test]
fn says_and_reports_errors_until_cleared() {
    let mut status = Status::default();
    assert_eq!(status.text(), "");

    status.say("Saved");
    assert_eq!((status.text(), status.is_error()), ("Saved", false));

    status.report_error("Save failed");
    assert_eq!((status.text(), status.is_error()), ("Save failed", true));

    status.clear();
    assert_eq!((status.text(), status.is_error()), ("", false));
}

#[test]
fn news_is_shown_at_once_when_it_can_be_read() {
    let mut status = Status::default();
    status.announce("Opened book.xlsx", false, true);
    assert_eq!(status.text(), "Opened book.xlsx");
}

#[test]
fn news_waits_until_the_user_can_read_it() {
    let mut status = Status::default();
    status.announce("Reload failed", true, false);
    assert_eq!(status.text(), "");

    status.catch_up(false);
    assert_eq!(status.text(), "", "still in a prompt");

    status.catch_up(true);
    assert_eq!((status.text(), status.is_error()), ("Reload failed", true));

    status.clear();
    status.catch_up(true);
    assert_eq!(status.text(), "", "news is shown once");
}

#[test]
fn held_news_does_not_hide_what_was_just_said() {
    let mut status = Status::default();
    status.announce("Saved", false, false);
    status.say("3 cells yanked");

    status.catch_up(true);
    assert_eq!(status.text(), "3 cells yanked");

    status.clear();
    status.catch_up(true);
    assert_eq!(status.text(), "Saved");
}
