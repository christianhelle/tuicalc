//! Reads a workbook's cells on other threads, so it can be on screen while they arrive.

use std::io::Read;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender, sync_channel};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use super::{Workbook, WorkbookError};
use crate::address::CellRef;
use crate::model::{Cell, Sheet};

/// Enough to keep the workers busy between frames, without reading far ahead of the screen.
const QUEUED_CHUNKS: usize = 16;
/// A sheet's first chunk is about a screenful, so that it can be shown as soon as possible.
pub(crate) const FIRST_CHUNK_CELLS: usize = 256;
pub(crate) const CHUNK_CELLS: usize = 16_384;
pub(crate) const READ_BUFFER_BYTES: usize = 64 * 1024;

/// A run of a sheet's cells and read-only regions, in the order the file holds them.
#[derive(Default)]
pub(crate) struct SheetChunk {
    pub cells: Vec<(CellRef, Cell)>,
    pub locked: Vec<(CellRef, CellRef)>,
}

impl SheetChunk {
    pub fn apply_to(self, sheet: &mut Sheet) {
        for (at, cell) in self.cells {
            sheet.load_cell(at, cell);
        }
        for (first, last) in self.locked {
            sheet.lock_range(first, last);
        }
    }
}

/// Counts the uncompressed bytes passing through, for showing progress.
pub(crate) struct Counting<'a, R> {
    pub inner: R,
    pub count: &'a AtomicU64,
}

impl<R: Read> Read for Counting<'_, R> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        let read = self.inner.read(buffer)?;
        self.count.fetch_add(read as u64, Ordering::Relaxed);
        Ok(read)
    }
}

pub(crate) enum LoadEvent {
    Cells { sheet: usize, chunk: SheetChunk },
    Failed(WorkbookError),
}

/// What came of waiting on a load.
pub enum Arrival {
    /// More of the workbook's cells are in it.
    Cells,
    /// Nothing happened in the time given.
    Nothing,
    /// The load is over.
    Ended,
    /// Something went wrong further in. The load won't get any further.
    Failed(WorkbookError),
}

/// The sheets waiting to be read. Whichever one is being looked at goes first.
pub(crate) struct SheetQueue {
    state: Mutex<QueueState>,
}

struct QueueState {
    started: Vec<bool>,
    preferred: usize,
}

impl SheetQueue {
    pub(crate) fn new(sheets: usize, preferred: usize) -> Self {
        Self {
            state: Mutex::new(QueueState {
                started: vec![false; sheets],
                preferred,
            }),
        }
    }

    /// The sheet to read next, if any are left.
    pub(crate) fn next(&self) -> Option<usize> {
        let mut state = self.state.lock().ok()?;
        let preferred = state.preferred;
        let sheet = if state.started.get(preferred) == Some(&false) {
            preferred
        } else {
            state.started.iter().position(|started| !started)?
        };
        state.started[sheet] = true;
        Some(sheet)
    }

    fn prefer(&self, sheet: usize) {
        if let Ok(mut state) = self.state.lock() {
            state.preferred = sheet;
        }
    }
}

/// A load in progress. Dropping it stops the load.
pub struct WorkbookLoad {
    events: Receiver<LoadEvent>,
    /// The sheets waiting, if the format lets them be read in any order.
    queue: Option<Arc<SheetQueue>>,
    read_bytes: Arc<AtomicU64>,
    total_bytes: u64,
}

impl WorkbookLoad {
    /// Runs `read` on another thread. It sends what it finds, and counts what it has read.
    pub(crate) fn start(
        total_bytes: u64,
        queue: Option<Arc<SheetQueue>>,
        read: impl FnOnce(&SyncSender<LoadEvent>, &AtomicU64) + Send + 'static,
    ) -> Self {
        let (sender, events) = sync_channel(QUEUED_CHUNKS);
        let read_bytes = Arc::new(AtomicU64::new(0));
        let counter = Arc::clone(&read_bytes);
        thread::spawn(move || read(&sender, &counter));
        Self {
            events,
            queue,
            read_bytes,
            total_bytes,
        }
    }

    /// Waits for the next cells, or for the load to end, and puts them in the workbook.
    pub fn receive(&self, workbook: &mut Workbook) -> Arrival {
        match self.events.recv() {
            Ok(event) => arrive(event, workbook),
            Err(_) => Arrival::Ended,
        }
    }

    /// Waits up to `wait` for the next cells, and puts them in the workbook.
    pub fn receive_within(&self, workbook: &mut Workbook, wait: Duration) -> Arrival {
        match self.events.recv_timeout(wait) {
            Ok(event) => arrive(event, workbook),
            Err(RecvTimeoutError::Timeout) => Arrival::Nothing,
            Err(RecvTimeoutError::Disconnected) => Arrival::Ended,
        }
    }

    /// Reads this sheet next, if it hasn't been started.
    pub fn prefer(&self, sheet: usize) {
        if let Some(queue) = &self.queue {
            queue.prefer(sheet);
        }
    }

    /// How much of the file has been read, from 0 to 1.
    pub fn progress(&self) -> f64 {
        if self.total_bytes == 0 {
            return 1.0;
        }
        let read = self.read_bytes.load(Ordering::Relaxed);
        (read as f64 / self.total_bytes as f64).min(1.0)
    }
}

fn arrive(event: LoadEvent, workbook: &mut Workbook) -> Arrival {
    match event {
        LoadEvent::Cells { sheet, chunk } => {
            chunk.apply_to(&mut workbook.sheets[sheet]);
            Arrival::Cells
        }
        LoadEvent::Failed(error) => Arrival::Failed(error),
    }
}

#[cfg(test)]
mod tests {
    use super::SheetQueue;

    #[test]
    fn the_queue_starts_the_preferred_sheet_first_then_the_rest_in_order() {
        let queue = SheetQueue::new(4, 2);
        assert_eq!(queue.next(), Some(2));
        assert_eq!(queue.next(), Some(0));

        queue.prefer(3);
        assert_eq!(queue.next(), Some(3));

        queue.prefer(0);
        assert_eq!(queue.next(), Some(1), "the preferred sheet already started");
        assert_eq!(queue.next(), None);
    }
}
