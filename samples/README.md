# Sample workbooks

Complex workbooks for trying tuicalc by hand and for the tests in `tests/samples.rs`, in a folder for
each format: `xlsx/`, `ods/` and `fods/`.

## Excel

These are in `xlsx/`.

| File | What makes it hard |
| --- | --- |
| `financial-model.xlsx` | Named ranges, cross-sheet formulas, shared formulas, a hidden sheet, merged cells, frozen panes, conditional formatting, validation, a comment, a hyperlink and a chart |
| `sales-pivot.xlsx` | 1,500 rows in a table with a calculated column, structured references, a pivot table with its cache, and a chart |
| `formula-edge-cases.xlsx` | Legacy array formulas, a spilled dynamic array, a data table, every error value, shared formulas filled both ways, and a sheet name that needs XML escaping |
| `text-and-layout.xlsx` | Rich text, Japanese, Arabic, emoji, multi-line text, text that looks like numbers or formulas, 5,000 characters in one cell, hidden rows and columns, and a cell at `XFD1048576` |
| `dates-1904.xlsx` | The 1904 date system, with date, time, elapsed time, currency, scientific and fraction formats side by side |
| `foreign-writer.xlsx` | Written by hand to look like other tools' output: namespace prefixes, inline strings, cells without references, parts outside `xl/`, and no styles or shared strings |

All but `foreign-writer.xlsx` were saved by real Excel, so they contain what Excel really writes.

## OpenDocument

These are in `ods/` and `fods/`. They're the same workbooks converted by real programs, and the tests
check that they hold the same cells as the `.xlsx` files they came from.

| File | Written by |
| --- | --- |
| `ods/financial-model.ods`, `ods/formula-edge-cases.ods`, `ods/text-and-layout.ods`, `ods/dates-1904.ods` | LibreOffice |
| `fods/text-and-layout.fods` | LibreOffice, as flat XML |
| `ods/financial-model-excel.ods` | Excel, which marks names and qualifies references differently |

## Rebuilding

To rebuild all of them, run `generate.ps1`, which needs Windows, Excel and LibreOffice. `-OpenDocumentOnly`
rebuilds only the OpenDocument files. The data is the same on every run, but the files won't be
byte-identical because the programs record timestamps and generated IDs.

## Giant workbooks

`generate.ps1 -Giant` writes the same six workbooks to `xlsx/giant/`, with millions of cells added, for
trying how tuicalc performs. It takes about a minute. Git ignores the folder because the files are
far too big to commit, and the tests don't use them.

| File | What it adds | Cells | Size |
| --- | --- | --- | --- |
| `sales-pivot.xlsx` | 1,000,000 rows in the table and pivot table | 6.0 million | 52 MB |
| `dates-1904.xlsx` | `Timeline`: 1,000,000 rows of dates, times and currency | 5.0 million | 54 MB |
| `financial-model.xlsx` | `Ledger`: 500,000 rows with formulas using the named ranges | 5.0 million | 48 MB |
| `formula-edge-cases.xlsx` | `Bulk`: 300,000 rows of shared formulas and errors | 2.4 million | 20 MB |
| `text-and-layout.xlsx` | `Bulk text`: 500,000 rows of different text in several scripts | 2.0 million | 20 MB |
| `foreign-writer.xlsx` | `Bulk`: 500,000 rows of prefixed inline strings with no cell references | 2.0 million | 5 MB |
