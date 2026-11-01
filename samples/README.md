# Sample workbooks

Complex `.xlsx` files for trying tuicalc by hand and for the tests in `tests/samples.rs`.

| File | What makes it hard |
| --- | --- |
| `financial-model.xlsx` | Named ranges, cross-sheet formulas, shared formulas, a hidden sheet, merged cells, frozen panes, conditional formatting, validation, a comment, a hyperlink and a chart |
| `sales-pivot.xlsx` | 1,500 rows in a table with a calculated column, structured references, a pivot table with its cache, and a chart |
| `formula-edge-cases.xlsx` | Legacy array formulas, a spilled dynamic array, a data table, every error value, shared formulas filled both ways, and a sheet name that needs XML escaping |
| `text-and-layout.xlsx` | Rich text, Japanese, Arabic, emoji, multi-line text, text that looks like numbers or formulas, 5,000 characters in one cell, hidden rows and columns, and a cell at `XFD1048576` |
| `dates-1904.xlsx` | The 1904 date system, with date, time, elapsed time, currency, scientific and fraction formats side by side |
| `foreign-writer.xlsx` | Written by hand to look like other tools' output: namespace prefixes, inline strings, cells without references, parts outside `xl/`, and no styles or shared strings |

All but `foreign-writer.xlsx` were saved by real Excel, so they contain what Excel really writes.

To rebuild them, run `generate.ps1`, which needs Windows and Excel. The data is the same on every
run, but the files won't be byte-identical because Excel records timestamps and generated IDs.
