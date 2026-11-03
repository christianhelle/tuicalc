//! The file dialog: browsing folders to choose a file to save to or open.

use std::io;
use std::path::{MAIN_SEPARATOR, Path, PathBuf};

use crossterm::event::{KeyCode, KeyEvent};

use crate::keys::is_ctrl;
use crate::line_edit::LineEdit;

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

/// What the application should do after a key press.
#[derive(Debug, PartialEq, Eq)]
pub enum DialogAction {
    Stay,
    /// Save to a file that doesn't exist yet.
    Save(PathBuf),
    /// Replace a file the user agreed to overwrite.
    Overwrite(PathBuf),
    /// Open an existing file.
    Open(PathBuf),
    /// Close the dialog without doing anything.
    Cancel,
}

/// What the dialog is choosing a file for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogKind {
    Save,
    Open,
}

#[derive(Debug)]
pub struct FileDialog {
    kind: DialogKind,
    directory: PathBuf,
    entries: Vec<Entry>,
    highlight: Option<usize>,
    name: LineEdit,
    message: String,
    /// The existing file waiting for the user to allow overwriting it.
    overwrite: Option<PathBuf>,
}

impl FileDialog {
    pub fn new(kind: DialogKind, directory: &Path) -> Self {
        let (entries, message) = match list_entries(directory) {
            Ok(entries) => (entries, String::new()),
            Err(error) => (
                directory
                    .parent()
                    .map(|_| Entry::Parent)
                    .into_iter()
                    .collect(),
                format!("Cannot read folder: {error}"),
            ),
        };
        Self {
            kind,
            directory: directory.to_path_buf(),
            entries,
            highlight: None,
            name: LineEdit::default(),
            message,
            overwrite: None,
        }
    }

    pub fn kind(&self) -> DialogKind {
        self.kind
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

    pub fn name(&self) -> String {
        self.name.text()
    }

    /// The name being typed, with its caret.
    pub fn name_line(&self) -> &LineEdit {
        &self.name
    }

    /// The existing file the dialog is asking to overwrite.
    pub fn pending_overwrite(&self) -> Option<&Path> {
        self.overwrite.as_deref()
    }

    /// The latest error or hint for the user.
    pub fn message(&self) -> &str {
        &self.message
    }

    pub fn set_message(&mut self, message: String) {
        self.message = message;
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> DialogAction {
        self.message.clear();
        if self.overwrite.is_some() {
            return self.overwrite_key(key);
        }
        match key.code {
            KeyCode::Down => self.move_highlight(1),
            KeyCode::Up => self.move_highlight(-1),
            KeyCode::Enter => return self.confirm(),
            KeyCode::Esc => return DialogAction::Cancel,
            KeyCode::Char('c') if is_ctrl(key) => return DialogAction::Cancel,
            _ => {
                let before = self.name.text();
                self.name.handle_key(key);
                if self.name.text() != before {
                    self.highlight = None;
                }
            }
        }
        DialogAction::Stay
    }

    fn overwrite_key(&mut self, key: KeyEvent) -> DialogAction {
        match key.code {
            KeyCode::Char('y' | 'Y') => {
                if let Some(path) = self.overwrite.take() {
                    return DialogAction::Overwrite(path);
                }
            }
            KeyCode::Char('n' | 'N') | KeyCode::Esc => self.overwrite = None,
            _ => {}
        }
        DialogAction::Stay
    }

    fn confirm(&mut self) -> DialogAction {
        match self.highlight.map(|index| self.entries[index].clone()) {
            Some(Entry::Parent) => {
                if let Some(parent) = self.directory.parent().map(Path::to_path_buf) {
                    self.enter_folder(parent);
                }
            }
            Some(Entry::Folder(name)) => self.enter_folder(self.directory.join(name)),
            _ => return self.submit(),
        }
        DialogAction::Stay
    }

    fn submit(&mut self) -> DialogAction {
        let name = self.name.text();
        let name = name.trim();
        if name.is_empty() {
            self.message = "Enter a file name".into();
            return DialogAction::Stay;
        }
        let path = self.directory.join(with_default_extension(name));
        if self.kind == DialogKind::Open {
            return DialogAction::Open(path);
        }
        if path.exists() {
            self.overwrite = Some(path);
            return DialogAction::Stay;
        }
        DialogAction::Save(path)
    }

    fn enter_folder(&mut self, directory: PathBuf) {
        match list_entries(&directory) {
            Ok(entries) => {
                self.directory = directory;
                self.entries = entries;
                self.highlight = None;
            }
            Err(error) => self.message = format!("Cannot read folder: {error}"),
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
            self.name = LineEdit::new(name);
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

/// A file name as typed, with `.xlsx` added if it has no extension.
pub fn with_default_extension(name: &str) -> PathBuf {
    let path = Path::new(name);
    if path.extension().is_some() {
        path.to_path_buf()
    } else {
        PathBuf::from(format!("{name}.xlsx"))
    }
}

fn is_workbook(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("xlsx"))
}
