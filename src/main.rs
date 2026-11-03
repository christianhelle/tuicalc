use std::io;
use std::panic;
use std::process::ExitCode;
use std::time::Duration;

use crossterm::event::{self, DisableMouseCapture, EnableMouseCapture, Event};
use crossterm::execute;
use ratatui::DefaultTerminal;
use ratatui::layout::Rect;
use tuicalc::app::App;
use tuicalc::ui::{draw, fit, handle_mouse};
use tuicalc::xlsx::Workbook;

const USAGE: &str = "Usage: tcalc [file.xlsx]";
/// How long to spend on a load or save in progress before looking at the keyboard and drawing
/// again.
const BUSY_FRAME: Duration = Duration::from_millis(12);

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let target = args.first().map_or("a blank workbook", String::as_str);
    let opened = match args.as_slice() {
        [flag] if flag == "--help" || flag == "-h" => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        [flag] if flag == "--version" || flag == "-V" => {
            println!("tcalc {}", env!("CARGO_PKG_VERSION"));
            return ExitCode::SUCCESS;
        }
        [] => Workbook::blank().map(App::new),
        [path] => Workbook::open_loading(path, 0).map(|(book, load)| App::opening(book, load)),
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    let mut app = match opened {
        Ok(app) => app,
        Err(error) => {
            eprintln!("tcalc: cannot open {target}: {error}");
            return ExitCode::FAILURE;
        }
    };

    let mut terminal = ratatui::init();
    release_mouse_on_panic();
    let result =
        execute!(io::stdout(), EnableMouseCapture).and_then(|()| run(&mut terminal, &mut app));
    // A terminal left capturing the mouse would type escape codes into the user's shell.
    let released = execute!(io::stdout(), DisableMouseCapture);
    ratatui::restore();
    let result = result.and(released);
    let failure = app.load_failure().map(str::to_string);
    // Freeing millions of cells takes a noticeable moment, and the process is ending anyway.
    std::mem::forget(app);
    if let Err(error) = result {
        eprintln!("tcalc: {error}");
        return ExitCode::FAILURE;
    }
    if let Some(failure) = failure {
        eprintln!("tcalc: cannot open {target}: {failure}");
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

fn run(terminal: &mut DefaultTerminal, app: &mut App) -> io::Result<()> {
    while !app.should_quit() {
        let size = terminal.size()?;
        fit(app, Rect::new(0, 0, size.width, size.height));
        terminal.draw(|frame| draw(frame, app))?;
        if app.document().is_busy() {
            app.absorb_background(BUSY_FRAME);
            while !app.should_quit() && event::poll(Duration::ZERO)? {
                handle_event(terminal, app, event::read()?)?;
            }
        } else {
            handle_event(terminal, app, event::read()?)?;
        }
    }
    Ok(())
}

fn handle_event(terminal: &DefaultTerminal, app: &mut App, event: Event) -> io::Result<()> {
    match event {
        Event::Key(key) => app.handle_key(key),
        Event::Mouse(mouse) => {
            let size = terminal.size()?;
            let screen = Rect::new(0, 0, size.width, size.height);
            handle_mouse(app, screen, mouse);
        }
        _ => {}
    }
    Ok(())
}
