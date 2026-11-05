//! Reads a package's sheets on several threads, with whichever one is being looked at first.

use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::sync::mpsc::SyncSender;
use std::thread;

use super::reader::{self, Plan};
use crate::workbook::loading::{LoadEvent, SheetQueue, WorkbookLoad};

/// More than this adds little, since one sheet is read by one thread and cells merge on another.
const MAX_WORKERS: usize = 4;

pub fn start(source: Arc<Vec<u8>>, plan: &Plan, preferred: usize) -> WorkbookLoad {
    let queue = Arc::new(SheetQueue::new(plan.sheet_names.len(), preferred));
    let job = Job {
        source,
        sheet_parts: plan.layout.sheet_parts.clone(),
        shared_strings_part: plan.shared_strings_part.clone(),
        date_styles: plan.date_styles.clone(),
        queue: Arc::clone(&queue),
    };
    WorkbookLoad::start(plan.total_bytes, Some(queue), move |sender, read_bytes| {
        job.run(sender, read_bytes);
    })
}

/// What the threads need, shared between them.
struct Job {
    source: Arc<Vec<u8>>,
    sheet_parts: Vec<String>,
    shared_strings_part: Option<String>,
    date_styles: Vec<bool>,
    queue: Arc<SheetQueue>,
}

impl Job {
    fn run(self, sender: &SyncSender<LoadEvent>, read_bytes: &AtomicU64) {
        let strings = reader::read_shared_strings(
            &self.source,
            self.shared_strings_part.as_deref(),
            read_bytes,
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
                scope.spawn(move || job.read_sheets(strings, &sender, read_bytes));
            }
        });
    }

    /// Reads sheets from the queue until none are left, or nobody is listening.
    fn read_sheets(
        &self,
        strings: &[String],
        sender: &SyncSender<LoadEvent>,
        read_bytes: &AtomicU64,
    ) {
        while let Some(sheet) = self.queue.next() {
            let mut listening = true;
            let result = reader::read_sheet_cells(
                &self.source,
                &self.sheet_parts[sheet],
                strings,
                &self.date_styles,
                read_bytes,
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
