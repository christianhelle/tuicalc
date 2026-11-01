use std::process::{Command, Output};

fn tuicalc(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_tuicalc")).args(args).output().unwrap()
}

#[test]
fn prints_usage_when_given_no_file() {
    let output = tuicalc(&[]);
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("Usage: tuicalc <file.xlsx>"));
}

#[test]
fn help_prints_usage_to_stdout_and_succeeds() {
    let output = tuicalc(&["--help"]);
    assert_eq!(output.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&output.stdout).contains("Usage: tuicalc <file.xlsx>"));
}

#[test]
fn reports_a_file_it_cannot_open_without_starting_the_interface() {
    let output = tuicalc(&["no-such-file.xlsx"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("no-such-file.xlsx"));
}
