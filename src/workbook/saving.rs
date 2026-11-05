//! Writes a workbook to disk on another thread, so it can stay on screen meanwhile.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
use std::thread;
use std::time::Duration;

use super::{Layout, WorkbookError};
use crate::model::Sheet;
use crate::ods;
use crate::xlsx::writer;

/// A package that has reached the disk, ready for its workbook to take in.
#[derive(Debug)]
pub struct SavedPackage {
    pub(super) path: PathBuf,
    pub(super) bytes: Arc<Vec<u8>>,
}

/// A save in progress. Dropping it leaves the save to finish by itself.
pub struct WorkbookSave {
    result: Receiver<Result<SavedPackage, WorkbookError>>,
}

impl WorkbookSave {
    pub(super) fn start(job: SaveJob) -> Self {
        let (sender, result) = channel();
        thread::spawn(move || {
            let outcome = job.write();
            // The snapshot goes first: while it's alive, the next edit would copy every cell.
            drop(job);
            let _ = sender.send(outcome);
        });
        Self { result }
    }

    /// Waits for the outcome.
    pub fn wait(&self) -> Result<SavedPackage, WorkbookError> {
        self.result.recv().unwrap_or_else(|_| Err(stopped()))
    }

    /// The outcome, if it arrives within `wait`.
    pub fn result_within(&self, wait: Duration) -> Option<Result<SavedPackage, WorkbookError>> {
        match self.result.recv_timeout(wait) {
            Ok(outcome) => Some(outcome),
            Err(RecvTimeoutError::Timeout) => None,
            Err(RecvTimeoutError::Disconnected) => Some(Err(stopped())),
        }
    }
}

fn stopped() -> WorkbookError {
    WorkbookError::Io(std::io::Error::other("the save stopped unexpectedly"))
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Target {
    /// The file the workbook came from, which nobody else may have changed.
    Own,
    /// A file that mustn't exist yet.
    New,
    /// An existing file the user agreed to replace.
    Over,
}

/// A snapshot of the workbook, and where it goes.
pub(super) struct SaveJob {
    pub(super) source: Arc<Vec<u8>>,
    pub(super) layout: Layout,
    pub(super) sheets: Vec<Sheet>,
    pub(super) path: PathBuf,
    pub(super) target: Target,
}

impl SaveJob {
    fn write(&self) -> Result<SavedPackage, WorkbookError> {
        let dirty = self.sheets.iter().any(Sheet::is_dirty);
        if self.target == Target::Own {
            if !dirty {
                return Ok(self.saved(Arc::clone(&self.source)));
            }
            if std::fs::read(&self.path)? != *self.source {
                return Err(WorkbookError::ChangedOnDisk);
            }
        }
        let bytes = if dirty {
            Arc::new(self.patched()?)
        } else {
            Arc::clone(&self.source)
        };
        match self.target {
            Target::New => create_file(&self.path, &bytes)?,
            Target::Own | Target::Over => replace_file(&self.path, &bytes)?,
        }
        Ok(self.saved(bytes))
    }

    fn patched(&self) -> Result<Vec<u8>, WorkbookError> {
        match &self.layout {
            Layout::Xlsx(layout) => writer::patch(&self.source, layout, &self.sheets),
            Layout::Ods(layout) => ods::writer::patch(&self.source, *layout, &self.sheets),
        }
    }

    fn saved(&self, bytes: Arc<Vec<u8>>) -> SavedPackage {
        SavedPackage {
            path: self.path.clone(),
            bytes,
        }
    }
}

/// Creates `path`, refusing a file that already exists, and removes it if the write fails.
fn create_file(path: &Path, bytes: &[u8]) -> Result<(), WorkbookError> {
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    if let Err(error) = file.write_all(bytes).and_then(|()| file.sync_all()) {
        drop(file);
        let _ = std::fs::remove_file(path);
        return Err(error.into());
    }
    Ok(())
}

/// Replaces `path` atomically, so a failed save cannot leave a truncated workbook behind.
fn replace_file(path: &Path, bytes: &[u8]) -> Result<(), WorkbookError> {
    let target = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let directory = target
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty());
    let mut temporary = tempfile::NamedTempFile::new_in(directory.unwrap_or(Path::new(".")))?;
    temporary.write_all(bytes)?;
    temporary.as_file().sync_all()?;
    if let Ok(metadata) = std::fs::metadata(&target) {
        std::fs::set_permissions(temporary.path(), metadata.permissions())?;
    }
    temporary
        .persist(&target)
        .map_err(|error| WorkbookError::Io(error.error))?;
    Ok(())
}
