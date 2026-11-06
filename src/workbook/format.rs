//! The one place that knows which file formats there are: it tells them apart, starts reading
//! them, and patches edits into them.

use std::path::Path;
use std::sync::Arc;

use super::{WorkbookError, WorkbookLoad};
use crate::model::Sheet;
use crate::ods;
use crate::xlsx::{loading as xlsx_loading, reader, writer};

/// Where the parts that saving needs live in the file, which also says what format it's in.
#[derive(Debug, Clone)]
pub(crate) enum Layout {
    Xlsx(reader::Layout),
    Ods(ods::Layout),
}

/// What a file holds before its cells arrive, and the load bringing them.
pub(crate) struct Opening {
    pub sheet_names: Vec<String>,
    pub date1904: bool,
    pub layout: Layout,
    pub load: WorkbookLoad,
}

/// Works out the format of `source` and starts reading its cells, preferring the given sheet.
pub(crate) fn open(source: Arc<Vec<u8>>, preferred_sheet: usize) -> Result<Opening, WorkbookError> {
    if let Some(layout) = ods::detect(&source)? {
        let plan = ods::reader::read_plan(&source, layout)?;
        let load = ods::reader::start_load(source, layout, &plan);
        return Ok(Opening {
            sheet_names: plan.sheet_names,
            date1904: false,
            layout: Layout::Ods(layout),
            load,
        });
    }
    let plan = reader::read_plan(&source)?;
    let load = xlsx_loading::start(source, &plan, preferred_sheet);
    Ok(Opening {
        sheet_names: plan.sheet_names,
        date1904: plan.date1904,
        layout: Layout::Xlsx(plan.layout),
        load,
    })
}

/// The package a workbook with no file starts from.
pub(crate) fn blank() -> Result<Vec<u8>, WorkbookError> {
    writer::blank_package()
}

impl Layout {
    /// Writes the edited cells of `sheets` into `source`, leaving everything else as it was.
    pub(crate) fn patch(&self, source: &[u8], sheets: &[Sheet]) -> Result<Vec<u8>, WorkbookError> {
        match self {
            Self::Xlsx(layout) => writer::patch(source, layout, sheets),
            Self::Ods(layout) => ods::writer::patch(source, *layout, sheets),
        }
    }

    /// Whether a file with this name is written in this format. A name no format claims goes with
    /// whatever the workbook is in already.
    fn suits(&self, path: &Path) -> bool {
        let kind = ods::Kind::of_path(path);
        let is_xlsx = path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("xlsx"));
        match (self, kind) {
            (Self::Xlsx(_), kind) => kind.is_none(),
            (Self::Ods(layout), Some(kind)) => kind.layout() == *layout,
            (Self::Ods(_), None) => !is_xlsx,
        }
    }
}

/// What to write edits into, for a file of the given name. A workbook stays in its format, except
/// that one with no file yet may take whatever format the name asks for.
pub(crate) fn package_for(
    path: &Path,
    source: &Arc<Vec<u8>>,
    layout: &Layout,
    has_file: bool,
) -> Result<(Arc<Vec<u8>>, Layout), WorkbookError> {
    if layout.suits(path) {
        return Ok((Arc::clone(source), layout.clone()));
    }
    match ods::Kind::of_path(path) {
        Some(kind) if !has_file => {
            let source = ods::writer::blank(kind)?;
            Ok((Arc::new(source), Layout::Ods(kind.layout())))
        }
        _ => Err(WorkbookError::Invalid(
            "can't save an opened workbook in another format".into(),
        )),
    }
}
