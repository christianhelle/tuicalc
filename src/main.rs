use std::io;
use std::process::ExitCode;

use crossterm::event::{self, Event};
use ratatui::DefaultTerminal;
use tuicalc::app::App;
use tuicalc::ui::draw;
use tuicalc::xlsx::Workbook;

const USAGE: &str = "Usage: tcalc [file.xlsx]";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let opened = match args.as_slice() {
        [flag] if flag == "--help" || flag == "-h" => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        [] => Workbook::blank(),
        [path] => Workbook::open(path),
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    let workbook = match opened {
        Ok(workbook) => workbook,
        Err(error) => {
            let target = args.first().map_or("a blank workbook", String::as_str);
            eprintln!("tcalc: cannot open {target}: {error}");
            return ExitCode::FAILURE;
        }
    };

    let mut terminal = ratatui::init();
    let result = run(&mut terminal, App::new(workbook));
    ratatui::restore();
    if let Err(error) = result {
        eprintln!("tcalc: {error}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

fn run(terminal: &mut DefaultTerminal, mut app: App) -> io::Result<()> {
    while !app.should_quit() {
        terminal.draw(|frame| draw(frame, &mut app))?;
        if let Event::Key(key) = event::read()? {
            app.handle_key(key);
        }
    }
    Ok(())
}
