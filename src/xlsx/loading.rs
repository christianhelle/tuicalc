//! Reads a workbook's cells on other threads, so it can be on screen while they arrive.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender, sync_channel};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use super::reader::{self, Plan, SheetChunk};
use crate::workbook::{Workbook, WorkbookError};

/// More than this adds little, since one sheet is read by one thread and cells merge on another.
const MAX_WORKERS: usize = 4;
/// Enough to keep the workers busy between frames, without reading far ahead of the screen.
const QUEUED_CHUNKS: usize = 16;

enum LoadEvent {
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
struct SheetQueue {
    state: Mutex<QueueState>,
}

struct QueueState {
    started: Vec<bool>,
    preferred: usize,
}

impl SheetQueue {
    fn new(sheets: usize, preferred: usize) -> Self {
        Self {
            state: Mutex::new(QueueState {
                started: vec![false; sheets],
                preferred,
            }),
        }
    }

    /// The sheet to read next, if any are left.
    fn next(&self) -> Option<usize> {
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
    queue: Arc<SheetQueue>,
    read_bytes: Arc<AtomicU64>,
    total_bytes: u64,
}

impl WorkbookLoad {
    pub(crate) fn start(source: Arc<Vec<u8>>, plan: &Plan, preferred: usize) -> Self {
        let (sender, events) = sync_channel(QUEUED_CHUNKS);
        let queue = Arc::new(SheetQueue::new(plan.sheet_names.len(), preferred));
        let read_bytes = Arc::new(AtomicU64::new(0));
        let job = Job {
            source,
            sheet_parts: plan.layout.sheet_parts.clone(),
            shared_strings_part: plan.shared_strings_part.clone(),
            date_styles: plan.date_styles.clone(),
            queue: Arc::clone(&queue),
            read_bytes: Arc::clone(&read_bytes),
        };
        thread::spawn(move || job.run(&sender));
        Self {
            events,
            queue,
            read_bytes,
            total_bytes: plan.total_bytes,
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
        self.queue.prefer(sheet);
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

/// What the threads need, shared between them.
struct Job {
    source: Arc<Vec<u8>>,
    sheet_parts: Vec<String>,
    shared_strings_part: Option<String>,
    date_styles: Vec<bool>,
    queue: Arc<SheetQueue>,
    read_bytes: Arc<AtomicU64>,
}

impl Job {
    fn run(self, sender: &SyncSender<LoadEvent>) {
        let strings = reader::read_shared_strings(
            &self.source,
            self.shared_strings_part.as_deref(),
            &self.read_bytes,
        );
        let strings = match strings {
            Ok(strings) => strings,
            Err(error) => {
                let _ = sender.send(LoadEvent::Failed(error));
                return;
            }
        };
        let cores = thread::available_parallelism().map_or(1, usize::from);
        let workers = cores.min(MAX_WORKERS).min(self.sheet_parts.len());
        let job = &self;
        let strings = &strings;
        thread::scope(|scope| {
            for _ in 0..workers {
                let sender = sender.clone();
                scope.spawn(move || job.read_sheets(strings, &sender));
            }
        });
    }

    /// Reads sheets from the queue until none are left, or nobody is listening.
    fn read_sheets(&self, strings: &[String], sender: &SyncSender<LoadEvent>) {
        while let Some(sheet) = self.queue.next() {
            let mut listening = true;
            let result = reader::read_sheet_cells(
                &self.source,
                &self.sheet_parts[sheet],
                strings,
                &self.date_styles,
                &self.read_bytes,
                &mut |chunk| {
                    listening = sender.send(LoadEvent::Cells { sheet, chunk }).is_ok();
                    listening
                },
            );
            if let Err(error) = result {
                let _ = sender.send(LoadEvent::Failed(error));
                return;
            }
            if !listening {
                return;
            }
        }
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
