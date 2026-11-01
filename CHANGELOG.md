# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Viewing workbooks: `tuicalc <file.xlsx>` shows a sheet as a grid, with the
  cursor's address and full contents in a formula bar above it. Numbers show as
  many digits as fit, as in Excel's General format, and dates show as
  `YYYY-MM-DD`, in workbooks of either date system. Text is cut to the terminal
  width of its column, wide characters included.
- Moving around: the arrow keys, `Tab`, `Shift-Tab` and `Enter` move the cursor,
  `PgUp` and `PgDn` move by a page, `Home` and `End` go to the row's edges, and
  `Ctrl-Home` and `Ctrl-End` go to A1 and the last used cell. The view scrolls
  just enough to keep the cursor on screen.
- Several sheets: tabs along the bottom show the sheets, keeping the current one
  on screen. `F6` and `Ctrl-PgDn` show the next, `Shift-F6` and `Ctrl-PgUp` the
  previous, and each sheet remembers where its cursor was.
- Editing cells: typing replaces a cell and `F2` edits what is in it. `Enter`,
  `Tab` and the up and down arrows confirm and move, and `Esc` cancels. `Delete`
  and `Backspace` clear a cell. Numbers, `TRUE` and `FALSE` and errors such as
  `#N/A` are recognised, `=` starts a formula and `'` makes the rest text. Date
  cells take dates in the form they show them.
- Undo and redo: `Ctrl-Z` and `Ctrl-Y` go back and forth through edits, returning
  to the sheet and cell of each one.
- Saving: `Ctrl-S` changes only the edited cells in the file and keeps everything
  else, such as charts and styles. A workbook with formulas recalculates when
  Excel next opens it. A failed save leaves the file as it was, and a file that
  another program changed since it was opened is not overwritten.
- Quitting: `Ctrl-Q` or `Ctrl-C` asks whether to save or discard unsaved edits
  first, and stays open if the save fails.
- Cells in array formulas and data tables, which Excel only changes as a whole,
  are read-only and say why.

[Unreleased]: https://github.com/christianhelle/tuicalc/commits/main
