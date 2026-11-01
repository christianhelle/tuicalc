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
- Commands: `:w`, `:q`, `:q!`, `:wq` and `:x`, or `ZZ` and `ZQ`, save and quit, and
  `:25` or `:C7` jump to a row or cell.
- Undo and redo: `u` and `Ctrl-R`, or `Ctrl-Z` and `Ctrl-Y`, go back and forth
  through commands, returning to the sheet and cell of each one. Anything one
  command changed is undone together.
- Saving: `Ctrl-S` changes only the edited cells in the file and keeps everything
  else, such as charts and styles. A workbook with formulas recalculates when
  Excel next opens it. A failed save leaves the file as it was, and a file that
  another program changed since it was opened is not overwritten.
- Quitting: `Ctrl-Q` or `Ctrl-C` asks whether to save or discard unsaved edits
  first, and stays open if the save fails.
- Cells in array formulas and data tables, which Excel only changes as a whole,
  are read-only and say why.

[Unreleased]: https://github.com/christianhelle/tuicalc/commits/main
