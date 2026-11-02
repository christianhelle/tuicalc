# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Viewing workbooks: `tcalc <file.xlsx>` shows a sheet as a grid, with the
  cursor's address and full contents in a formula bar above it. Numbers show as
  many digits as fit, as in Excel's General format, and dates show as
  `YYYY-MM-DD`, in workbooks of either date system. Text is cut to the terminal
  width of its column, wide characters included.
- The look of tuilook: the formula bar and grid sit in titled boxes, with lines
  between the grid's columns and a thick border around whichever is in use. The
  grid's title names the file and sheet and marks unsaved changes. Colours follow
  the terminal's theme: bold cyan headers, with the cursor's row and column in
  yellow, a reversed cursor and selection, and errors in red. The status line
  shows a coloured badge for the mode and hints for the keys.
- Help: `?` lists the keys of the current mode in a dialog, or `F1` while editing
  a cell, and any key closes it.
- Finding and jumping: `/` finds text in the sheet, ignoring case, and `n` and `N`
  go to the next and previous match. `Ctrl-G` goes to a cell or a row.
- Mouse: click a cell or a sheet's tab, and use the wheel to move the cursor. Hold
  `Shift` to select text in the terminal.
- Reloading: `F5` reads the workbook from disk again, staying on the same cell.
- Opening large workbooks without waiting: a workbook appears straight away and
  its cells fill in behind the scenes, the visible sheet first, with progress on
  the status line. Moving and searching work while it loads, and `Esc` stops it,
  returning to the last workbook if there was one.
- Saving without waiting: a save runs in the background, so moving, searching and
  copying keep working while editing waits for it. Quitting during a save waits
  for it too, and stays open if it fails.
- New workbooks: `tcalc` with no arguments opens a blank workbook. Saving it
  opens a dialog for browsing folders and naming the file, which asks before
  overwriting one.
- Opening workbooks from inside the app: `Ctrl-O` browses for a file, or takes a
  typed path.
- Vim motions: tuicalc starts in normal mode. `h` `j` `k` `l` and the arrow keys
  move, and most motions take a count such as `5j`. `w`, `b` and `e` move between
  runs of filled cells in a row, `{` and `}` between gaps in a column. `0`, `^`
  and `$` go along the row, `gg` and `G` to rows, `H`, `M` and `L` around the
  screen, and `Ctrl-D`, `Ctrl-U`, `Ctrl-F`, `Ctrl-B` and `zz`, `zt`, `zb` scroll.
- Several sheets: tabs along the bottom show the sheets, keeping the current one
  on screen. `gt`, `F6` and `Ctrl-PgDn` show the next, `gT`, `Shift-F6` and
  `Ctrl-PgUp` the previous, and each sheet remembers where its cursor was.
- Insert mode: `i` and `a` edit a cell with the caret at either end, `s` replaces
  it, `=` starts a formula, and `o` and `O` edit the cell below or above. `Esc`
  keeps what was typed, `Enter`, `Tab` and the up and down arrows keep it and
  move, and `Ctrl-C` drops it. Numbers, `TRUE` and `FALSE` and errors such as
  `#N/A` are recognised, and `'` makes the rest text. Date cells take dates in
  the form they show them.
- Visual modes: `v` selects a rectangle of cells and `V` whole rows, which every
  motion extends and `o` turns around. `d`, `y` and `c` delete, yank or change the
  selection, and the status line names the mode you are in.
- Operators: `d`, `y` and `c` work with motions, as in `d$`, `y3j` and `cw`, and on
  whole rows as `dd`, `yy` and `cc`. `x`, `X`, `D`, `C` and `S` are there too. `p`
  and `P` paste what was last yanked or deleted, adjusting formulas' relative
  references.
- Undo and redo: `u` and `Ctrl-R`, or `Ctrl-Z` and `Ctrl-Y`, go back and forth
  through commands, returning to the sheet and cell of each one. Anything one
  command changed is undone together.
- Saving: `Ctrl-S` changes only the edited cells in the file and keeps everything
  else, such as charts and styles. A workbook with formulas recalculates when
  Excel next opens it. A failed save leaves the file as it was, and a file that
  another program changed since it was opened is not overwritten.
- Quitting: `q`, `Ctrl-Q` or `Ctrl-C` quits, asking in a dialog whether to save or
  discard unsaved edits, as opening or reloading a workbook does too. It stays
  open if the save fails. `ZZ` saves and quits, and `ZQ` quits without asking.
- Cells in array formulas and data tables, which Excel only changes as a whole,
  are read-only and say why.

[Unreleased]: https://github.com/christianhelle/tuicalc/commits/main
