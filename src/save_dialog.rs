//! The file save dialog: choosing a folder and a name for a workbook that has no file yet.

use std::io;
use std::path::{MAIN_SEPARATOR, Path, PathBuf};

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
}

impl SaveDialog {
    pub fn open(directory: &Path) -> Self {
        Self {
            directory: directory.to_path_buf(),
            entries: list_entries(directory).unwrap_or_default(),
        }
    }

    pub fn directory(&self) -> &Path {
        &self.directory
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
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
