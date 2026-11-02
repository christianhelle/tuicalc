//! The file save dialog: choosing a folder and a name for a workbook that has no file yet.

use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct SaveDialog {
    directory: PathBuf,
}

impl SaveDialog {
    pub fn open(directory: &Path) -> Self {
        Self {
            directory: directory.to_path_buf(),
        }
    }

    pub fn directory(&self) -> &Path {
        &self.directory
    }
}
