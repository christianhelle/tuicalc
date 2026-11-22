//! Links in cell text, and handing them to the default browser.

/// The first web address in some text, with `https://` added to one that starts at `www.`.
pub fn find_url(text: &str) -> Option<String> {
    text.split_whitespace().find_map(url_in_word)
}

fn url_in_word(word: &str) -> Option<String> {
    let start = ["http://", "https://", "www."]
        .iter()
        .filter_map(|prefix| find_ignoring_case(word, prefix))
        .min()?;
    let url = trim_trailing_punctuation(&word[start..]);
    let lower = url.to_ascii_lowercase();
    let host = ["http://", "https://", "www."]
        .iter()
        .find_map(|prefix| lower.strip_prefix(prefix))?;
    if host.is_empty() {
        return None;
    }
    if lower.starts_with("www.") {
        Some(format!("https://{url}"))
    } else {
        Some(url.to_string())
    }
}

fn find_ignoring_case(haystack: &str, needle: &str) -> Option<usize> {
    haystack.to_ascii_lowercase().find(needle)
}

/// Drops punctuation that ends a sentence or closes a bracket around the link, keeping a closing
/// parenthesis that pairs with one inside it, as in Wikipedia's `Rust_(programming_language)`.
fn trim_trailing_punctuation(url: &str) -> &str {
    let mut url = url;
    while let Some(last) = url.chars().last() {
        let belongs = match last {
            ')' => url.matches('(').count() >= url.matches(')').count(),
            '.' | ',' | ';' | ':' | '!' | '?' | ']' | '}' | '>' | '"' | '\'' => false,
            _ => true,
        };
        if belongs {
            break;
        }
        url = &url[..url.len() - last.len_utf8()];
    }
    url
}

/// Opens a link in the default browser, without waiting for it.
pub fn open_in_browser(url: &str) -> std::io::Result<()> {
    use std::process::{Command, Stdio};
    // rundll32 takes the link as one argument, unlike `cmd /c start`, which splits it at `&`.
    let mut command = if cfg!(windows) {
        let mut command = Command::new("rundll32");
        command.arg("url.dll,FileProtocolHandler");
        command
    } else if cfg!(target_os = "macos") {
        Command::new("open")
    } else {
        Command::new("xdg-open")
    };
    command
        .arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(drop)
}
