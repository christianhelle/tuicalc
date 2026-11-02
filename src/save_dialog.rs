//! The file save dialog: choosing a folder and a name for a workbook that has no file yet.

use std::io;
use std::path::{MAIN_SEPARATOR, Path, PathBuf};

use crossterm::event::{KeyCode, KeyEvent};

use crate::app::is_text_input;

/// One line of the dialog's list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Entry {
    Parent,
    Folder(String),
    File(String),
}

impl Entry {
    pub fn label(&self) -> String {
        match self {
            Self::Parent => "..".into(),
            Self::Folder(name) => format!("{name}{MAIN_SEPARATOR}"),
            Self::File(name) => name.clone(),
        }
    }
}

#[derive(Debug)]
pub struct SaveDialog {
    directory: PathBuf,
    entries: Vec<Entry>,
    highlight: Option<usize>,
    name: String,
}

impl SaveDialog {
    pub fn open(directory: &Path) -> Self {
        Self {
            directory: directory.to_path_buf(),
            entries: list_entries(directory).unwrap_or_default(),
            highlight: None,
            name: String::new(),
        }
    }

    pub fn directory(&self) -> &Path {
        &self.directory
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// The position in the list of the highlighted line.
    pub fn highlight(&self) -> Option<usize> {
        self.highlight
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Down => self.move_highlight(1),
            KeyCode::Up => self.move_highlight(-1),
            KeyCode::Enter => self.confirm(),
            KeyCode::Backspace => {
                self.name.pop();
                self.highlight = None;
            }
            KeyCode::Char(c) if is_text_input(key) => {
                self.name.push(c);
                self.highlight = None;
            }
            _ => {}
        }
    }

    fn confirm(&mut self) {
        match self.highlight.map(|index| self.entries[index].clone()) {
            Some(Entry::Parent) => {
                if let Some(parent) = self.directory.parent().map(Path::to_path_buf) {
                    self.enter_folder(parent);
                }
            }
            Some(Entry::Folder(name)) => self.enter_folder(self.directory.join(name)),
            _ => {}
        }
    }

    fn enter_folder(&mut self, directory: PathBuf) {
        if let Ok(entries) = list_entries(&directory) {
            self.directory = directory;
            self.entries = entries;
            self.highlight = None;
        }
    }

    fn move_highlight(&mut self, offset: isize) {
        let Some(last) = self.entries.len().checked_sub(1) else {
            return;
        };
        let next = match self.highlight {
            Some(index) => index.saturating_add_signed(offset).min(last),
            None if offset > 0 => 0,
            None => last,
        };
        self.highlight = Some(next);
        if let Entry::File(name) = &self.entries[next] {
            self.name = name.clone();
        }
    }
}

/// The parent, then folders, then workbooks, each sorted ignoring case.
fn list_entries(directory: &Path) -> io::Result<Vec<Entry>> {
    let mut folders = Vec::new();
    let mut files = Vec::new();
    for entry in std::fs::read_dir(directory)?.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = entry.path();
        if path.is_dir() {
            folders.push(name);
        } else if is_workbook(&path) {
            files.push(name);
        }
    }
    folders.sort_by_key(|name| name.to_lowercase());
    files.sort_by_key(|name| name.to_lowercase());

    let parent = directory.parent().map(|_| Entry::Parent);
    Ok(parent
        .into_iter()
        .chain(folders.into_iter().map(Entry::Folder))
        .chain(files.into_iter().map(Entry::File))
        .collect())
}

fn is_workbook(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("xlsx"))
}
