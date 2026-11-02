use std::io;
use std::panic;
use std::process::ExitCode;

use crossterm::event::{self, DisableMouseCapture, EnableMouseCapture, Event};
use crossterm::execute;
use ratatui::DefaultTerminal;
use ratatui::layout::Rect;
use tuicalc::app::App;
use tuicalc::ui::{draw, handle_mouse};
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
    release_mouse_on_panic();
    let result = execute!(io::stdout(), EnableMouseCapture)
        .and_then(|()| run(&mut terminal, App::new(workbook)));
    // A terminal left capturing the mouse would type escape codes into the user's shell.
    let released = execute!(io::stdout(), DisableMouseCapture);
    ratatui::restore();
    let result = result.and(released);
    if let Err(error) = result {
        eprintln!("tcalc: {error}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

/// Adds to the panic handling `ratatui::init` installed, which knows nothing about the mouse.
fn release_mouse_on_panic() {
    let previous = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        let _ = execute!(io::stdout(), DisableMouseCapture);
        previous(info);
    }));
}

fn run(terminal: &mut DefaultTerminal, mut app: App) -> io::Result<()> {
    while !app.should_quit() {
        terminal.draw(|frame| draw(frame, &mut app))?;
        match event::read()? {
            Event::Key(key) => app.handle_key(key),
            Event::Mouse(mouse) => {
                let size = terminal.size()?;
                let screen = Rect::new(0, 0, size.width, size.height);
                handle_mouse(&mut app, screen, mouse);
            }
            _ => {}
        }
    }
    Ok(())
}
