# tuicalc

View and edit Excel `.xlsx` workbooks from the terminal.

```
cargo run --release -- path/to/book.xlsx
```

## Keys

| Key | Action |
| --- | --- |
| Arrows, Tab, Shift+Tab, Enter | Move |
| PgUp, PgDn, Home, End | Move by page or to the row's edge |
| Ctrl+Home, Ctrl+End | Jump to A1 or the last used cell |
| Any character | Replace the cell and start editing |
| F2 | Edit the cell's contents |
| Enter, Tab, Up, Down | Confirm the edit and move |
| Esc | Cancel the edit |
| Delete, Backspace | Clear the cell |
| Ctrl+Z, Ctrl+Y | Undo, redo |
| F6, Shift+F6, Ctrl+PgDn, Ctrl+PgUp | Next or previous sheet |
| Ctrl+S | Save |
| Ctrl+Q, Ctrl+C | Quit |

## Entering values

Numbers, `TRUE`/`FALSE` and error literals such as `#N/A` are recognised. A leading `=` makes a
formula, and a leading `'` forces text. Date-formatted cells show and accept `YYYY-MM-DD`,
`YYYY-MM-DD HH:MM:SS` and `HH:MM:SS`.

## How saving works

Saving patches the original package rather than regenerating it. Parts the edit doesn't touch are
copied byte for byte, and an edited worksheet changes only the affected cells, so charts, styles,
pivot tables and macros survive. On a sheet with edits, shared formulas are expanded into ordinary
formulas. The save replaces the file atomically, and is refused if another program changed the
file after it was opened.

## Limitations

- **No formula engine.** tuicalc shows the results Excel last stored. A new formula shows its text,
  and cells depending on an edit keep their old result until Excel opens the file and recalculates
  it, which saved workbooks request.
- **Number formats** other than dates aren't applied: currency and percentages show as plain numbers.
- **Column widths** are fixed in the grid. The formula bar shows a cell's full contents.
- Array formula and data table regions are read-only.
- New workbooks, Save As, and adding or removing sheets, rows or columns aren't supported yet.
- Inserting a cell beside cells that omit their `r` attribute, which Excel never writes, can shift
  those cells.

## Development

```
cargo test
```

Tests run against four public seams: workbook file I/O, the workbook model, app state driven by
key events, and the screen rendered to an in-memory terminal.
