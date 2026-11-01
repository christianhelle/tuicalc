# Rebuilds the sample workbooks in this folder. Needs Windows and Excel, except for
# foreign-writer.xlsx, which is written by hand to look like the output of other tools.
#
# Excel runs invisibly. Only Excel processes started by this script are ever stopped.
$ErrorActionPreference = 'Stop'

$missing = [Type]::Missing
$xlOpenXMLWorkbook = 51
$xlLine = 4
$xlColumnClustered = 51
$xlRowField = 1
$xlColumnField = 2
$xlSum = -4157
$xlCellValue = 1
$xlGreater = 5
$xlValidateDecimal = 2
$xlBetween = 1
$xlSheetHidden = 0
$xlDatabase = 1
$xlSrcRange = 1
$xlYes = 1

function Add-Sheet($book, [string]$name) {
    $sheet = $book.Worksheets.Add($missing, $book.Worksheets.Item($book.Worksheets.Count))
    $sheet.Name = $name
    $sheet
}

function Save-Book($excel, $book, [string]$fileName) {
    $book.Worksheets.Item(1).Activate()
    # Keeps the author's name out of files that get committed.
    $book.RemovePersonalInformation = $true
    $path = Join-Path $PSScriptRoot $fileName
    if (Test-Path $path) { Remove-Item $path -Confirm:$false }
    $book.SaveAs($path, $xlOpenXMLWorkbook)
    $book.Close($false)
    Write-Output "wrote $fileName"
}

# A cash flow model: named ranges, cross-sheet references, shared formulas, merged cells,
# frozen panes, conditional formatting, validation, a chart, and a hidden sheet.
function New-FinancialModel($excel) {
    $book = $excel.Workbooks.Add()

    $inputs = $book.Worksheets.Item(1)
    $inputs.Name = 'Assumptions'
    $inputs.Range('A1').Value2 = 'Assumptions'
    $inputs.Range('A1').Font.Bold = $true
    $inputs.Range('A2').Value2 = 'Annual growth'
    $inputs.Range('B2').Value2 = 0.05
    $inputs.Range('A3').Value2 = 'Tax rate'
    $inputs.Range('B3').Value2 = 0.25
    $inputs.Range('A4').Value2 = 'Discount rate'
    $inputs.Range('B4').Value2 = 0.1
    $inputs.Range('A5').Value2 = 'First month'
    $inputs.Range('B5').Value2 = 45292
    $inputs.Range('B5').NumberFormat = 'yyyy-mm-dd'
    $inputs.Range('A6').Value2 = 'Opening revenue'
    $inputs.Range('B6').Value2 = 1000
    $inputs.Range('B2:B4').NumberFormat = '0.0%'
    $inputs.Columns.Item(1).ColumnWidth = 20
    [void]$book.Names.Add('Growth', '=Assumptions!$B$2')
    [void]$book.Names.Add('TaxRate', '=Assumptions!$B$3')
    [void]$book.Names.Add('DiscountRate', '=Assumptions!$B$4')
    [void]$inputs.Range('B3').Validation.Add($xlValidateDecimal, 1, $xlBetween, '0', '1')

    $plan = Add-Sheet $book 'Monthly Plan'
    $plan.Range('A1').Value2 = 'Monthly plan'
    $plan.Range('A1:F1').Merge()
    $plan.Range('A1').HorizontalAlignment = -4108
    $plan.Range('A1').Font.Size = 14
    $plan.Range('A1').Font.Bold = $true
    $headers = 'Month', 'Revenue', 'Costs', 'Profit', 'Tax', 'Net'
    for ($i = 0; $i -lt $headers.Count; $i++) { $plan.Cells.Item(3, $i + 1).Value2 = $headers[$i] }
    $plan.Range('A3:F3').Font.Bold = $true
    $plan.Range('A3:F3').Interior.Color = 0xE6D5B8
    $plan.Range('A4').Formula = '=Assumptions!$B$5'
    $plan.Range('A5:A15').Formula = '=EDATE(A4,1)'
    $plan.Range('A4:A15').NumberFormat = 'mmm yyyy'
    $plan.Range('B4').Formula = '=Assumptions!$B$6'
    $plan.Range('B5:B15').Formula = '=B4*(1+Growth/12)'
    $plan.Range('C4:C15').Formula = '=B4*0.6'
    $plan.Range('D4:D15').Formula = '=B4-C4'
    $plan.Range('E4:E15').Formula = '=D4*TaxRate'
    $plan.Range('F4:F15').Formula = '=D4-E4'
    $plan.Range('A16').Value2 = 'Total'
    $plan.Range('B16:F16').Formula = '=SUM(B4:B15)'
    $plan.Range('A16:F16').Font.Bold = $true
    $plan.Range('B4:F16').NumberFormat = '#,##0.00 [$USD]'
    $plan.Columns.Item(1).ColumnWidth = 12
    $plan.Range('B:F').ColumnWidth = 14
    [void]$plan.Range('D4:D15').FormatConditions.Add($xlCellValue, $xlGreater, '=420')
    $plan.Range('D4:D15').FormatConditions.Item(1).Interior.Color = 0xCEEFC6
    $plan.Activate()
    $excel.ActiveWindow.SplitRow = 3
    $excel.ActiveWindow.SplitColumn = 1
    $excel.ActiveWindow.FreezePanes = $true
    $chart = $plan.Shapes.AddChart2(227, $xlLine, 500, 40, 420, 240)
    $chart.Chart.SetSourceData($plan.Range('A3:B15'))
    $chart.Chart.HasTitle = $true
    $chart.Chart.ChartTitle.Text = 'Revenue by month'

    $lookups = Add-Sheet $book 'Lookups'
    $lookups.Range('A1').Value2 = 'Band'
    $lookups.Range('B1').Value2 = 'Label'
    $lookups.Range('A2').Value2 = 0
    $lookups.Range('B2').Value2 = 'Small'
    $lookups.Range('A3').Value2 = 5000
    $lookups.Range('B3').Value2 = 'Medium'
    $lookups.Range('A4').Value2 = 10000
    $lookups.Range('B4').Value2 = 'Large'
    $lookups.Visible = $xlSheetHidden

    $summary = Add-Sheet $book 'Summary'
    $summary.Range('A1').Value2 = 'Total net'
    $summary.Range('B1').Formula = "='Monthly Plan'!F16"
    $summary.Range('A2').Value2 = 'Net present value'
    $summary.Range('B2').Formula = "=NPV(DiscountRate/12,'Monthly Plan'!F4:F15)"
    $summary.Range('A3').Value2 = 'Size band'
    $summary.Range('B3').Formula = "=VLOOKUP('Monthly Plan'!B16,Lookups!A2:B4,2,TRUE)"
    $summary.Range('A4').Value2 = 'Profitable'
    $summary.Range('B4').Formula = '=B1>0'
    $summary.Range('B1:B2').NumberFormat = '#,##0.00'
    $summary.Columns.Item(1).ColumnWidth = 20
    $summary.Range('A1').AddComment('Sum of the Net column on the plan') | Out-Null
    [void]$summary.Hyperlinks.Add($summary.Range('A6'), 'https://example.com/model', $missing, $missing, 'Model notes')

    Save-Book $excel $book 'financial-model.xlsx'
}

# 1,500 rows of sales in a table with a calculated column, summarised by a pivot table and chart.
function New-SalesPivot($excel) {
    $book = $excel.Workbooks.Add()
    $data = $book.Worksheets.Item(1)
    $data.Name = 'Sales'

    $regions = 'North', 'South', 'East', 'West'
    $products = 'Widget', 'Gadget', 'Gizmo', 'Doohickey', 'Thingamajig'
    $rows = 1500
    $values = New-Object 'object[,]' ($rows + 1), 5
    $titles = 'Date', 'Region', 'Product', 'Units', 'Price'
    for ($c = 0; $c -lt 5; $c++) { $values[0, $c] = $titles[$c] }
    # A fixed generator keeps the data the same on every rebuild.
    [long]$seed = 42
    for ($r = 1; $r -le $rows; $r++) {
        $next = {
            $script:seed = ($script:seed * 1103515245 + 12345) % 2147483648
            $script:seed
        }
        $script:seed = $seed
        $values[$r, 0] = [double](45292 + ((& $next) % 366))
        $values[$r, 1] = $regions[(& $next) % 4]
        $values[$r, 2] = $products[(& $next) % 5]
        $values[$r, 3] = [double](1 + ((& $next) % 50))
        $values[$r, 4] = [double](5 + ((& $next) % 200)) / 4
        $seed = $script:seed
    }
    for ($r = 0; $r -le $rows; $r++) {
        for ($c = 0; $c -lt 5; $c++) {
            $cell = $data.Cells.Item($r + 1, $c + 1)
            # PowerShell ties a COM assignment to the type first seen on that line of script.
            if ($values[$r, $c] -is [string]) {
                $cell.Value2 = [string]$values[$r, $c]
            } else {
                $cell.Value2 = [double]$values[$r, $c]
            }
        }
    }
    $data.Range('F1').Value2 = 'Revenue'
    $table = $data.ListObjects.Add($xlSrcRange, $data.Range('A1').Resize($rows + 1, 6), $missing, $xlYes)
    $table.Name = 'SalesTable'
    $table.TableStyle = 'TableStyleMedium2'
    $table.ListColumns.Item(6).DataBodyRange.Formula = '=[@Units]*[@Price]'
    $table.ListColumns.Item(1).DataBodyRange.NumberFormat = 'yyyy-mm-dd'
    $table.ListColumns.Item(5).DataBodyRange.NumberFormat = '0.00'
    $table.ListColumns.Item(6).DataBodyRange.NumberFormat = '#,##0.00'
    $data.Columns.Item(1).ColumnWidth = 12
    $data.Columns.Item(3).ColumnWidth = 14

    $report = Add-Sheet $book 'Report'
    $report.Range('A1').Value2 = 'Revenue by region and product'
    $report.Range('A1').Font.Bold = $true
    $cache = $book.PivotCaches().Create($xlDatabase, 'SalesTable')
    $pivot = $cache.CreatePivotTable($report.Range('A3'), 'SalesPivot')
    $pivot.PivotFields('Region').Orientation = $xlRowField
    $pivot.PivotFields('Product').Orientation = $xlColumnField
    [void]$pivot.AddDataField($pivot.PivotFields('Revenue'), 'Total revenue', $xlSum)
    $chart = $report.Shapes.AddChart2(201, $xlColumnClustered, 20, 160, 520, 260)
    $chart.Chart.SetSourceData($pivot.TableRange1)

    $check = Add-Sheet $book 'Checks'
    $check.Range('A1').Value2 = 'Table rows'
    $check.Range('B1').Formula = '=ROWS(SalesTable[Date])'
    $check.Range('A2').Value2 = 'Table revenue'
    $check.Range('B2').Formula = '=SUM(SalesTable[Revenue])'
    $check.Range('A3').Value2 = 'North revenue'
    $check.Range('B3').Formula = '=SUMIFS(SalesTable[Revenue],SalesTable[Region],"North")'

    Save-Book $excel $book 'sales-pivot.xlsx'
}

# Formula storage that trips up parsers: legacy and spilled arrays, a data table, every error
# value, shared formulas filled in both directions, and a sheet name that needs XML escaping.
function New-FormulaEdgeCases($excel) {
    $book = $excel.Workbooks.Add()

    $arrays = $book.Worksheets.Item(1)
    $arrays.Name = 'Arrays'
    $arrays.Range('A1').Value2 = 1
    $arrays.Range('A2').Value2 = 2
    $arrays.Range('A3').Value2 = 3
    $arrays.Range('B1:B3').FormulaArray = '=A1:A3*10'
    $arrays.Range('C1').FormulaArray = '=SUM(A1:A3*A1:A3)'
    $arrays.Range('E1').Formula2 = '=SEQUENCE(4,2)'
    $arrays.Range('H1').Value2 = 2
    $arrays.Range('I2').Formula = '=H1*10'
    $arrays.Range('H3').Value2 = 1
    $arrays.Range('H4').Value2 = 5
    $arrays.Range('H5').Value2 = 9
    [void]$arrays.Range('H2:I5').Table($missing, $arrays.Range('H1'))

    $errors = Add-Sheet $book 'Errors'
    $formulas = '=1/0', '=NA()', '=INDEX(A1:A2,5)', '=1+"x"', '=NOSUCHFUNCTION()', '=SQRT(-1)', '=SUM(A1:A2 B1:B2)'
    for ($i = 0; $i -lt $formulas.Count; $i++) { $errors.Cells.Item($i + 1, 1).Formula = $formulas[$i] }
    $errors.Range('C1').Value2 = $true
    $errors.Range('C2').Value2 = $false
    $errors.Range('C3').Formula = '=TRUE'
    $errors.Range('C4').Formula = '="te"&"xt"'
    $errors.Range('C5').Formula = '=""'
    $errors.Range('D1').Formula = '=IFERROR(A1,"caught")'

    $shared = Add-Sheet $book 'Shared'
    for ($r = 1; $r -le 6; $r++) {
        $shared.Cells.Item($r, 1).Value2 = [double]$r
        $shared.Cells.Item($r, 2).Value2 = [double]($r * 2)
    }
    $shared.Range('C1:C6').Formula = '=A1+B1'
    $shared.Range('D1:D6').Formula = '=$A$1+A1+$B1+B$1'
    $shared.Range('E1:H1').Formula = '=A1*2'
    $shared.Range('E8:H8').Formula = '=SUM(A:A)'
    $shared.Range('E9:E12').Formula = '=SUM(1:1)'
    $shared.Range('E2:E6').Formula = '="A1"&LOG10(A2)&IF(A2>1,"$B$2",A1)'

    $odd = Add-Sheet $book 'R&D <2024> "é"'
    $odd.Range('A1').Value2 = 'Referenced from another sheet'
    $back = Add-Sheet $book 'Cross refs'
    $back.Range('A1').Formula = "='R&D <2024> ""é""'!A1"
    $back.Range('A2:A4').Formula = "=Shared!C1+Arrays!A1"

    Save-Book $excel $book 'formula-edge-cases.xlsx'
}

# Awkward contents and layout: rich text, scripts of all kinds, text that looks like something
# else, cells at the far corner of the sheet, hidden rows and columns.
function New-TextAndLayout($excel) {
    $book = $excel.Workbooks.Add()
    $text = $book.Worksheets.Item(1)
    $text.Name = 'Text'
    $text.Range('A:A').NumberFormat = '@'
    $text.Range('A1').Value2 = '007'
    $text.Range('A2').Value2 = '=NOT A FORMULA'
    $text.Range('A3').Value2 = 'TRUE'
    $text.Range('A4').Value2 = '1e5'
    $text.Range('B1').Value2 = 'plain and bold and red'
    $rich = $text.Range('B1')
    $rich.Characters(11, 4).Font.Bold = $true
    $rich.Characters(19, 3).Font.Color = 0x0000FF
    $text.Range('B2').Value2 = [string]::Join('', [char[]](0x65E5, 0x672C, 0x8A9E, 0x306E, 0x30C6, 0x30AD, 0x30B9, 0x30C8))
    $text.Range('B3').Value2 = [char]::ConvertFromUtf32(0x1F4CA) + ' chart ' + [char]::ConvertFromUtf32(0x1F680)
    $text.Range('B4').Value2 = [string]::Join('', [char[]](0x0645, 0x0631, 0x062D, 0x0628, 0x0627))
    $text.Range('B5').Value2 = "line one`nline two`nline three"
    $text.Range('B5').WrapText = $true
    $text.Range('B6').Value2 = '  surrounded by spaces  '
    $text.Range('B7').Value2 = 'markup <b>&amp;</b> "quotes" ''apostrophes'''
    $text.Range('B8').Value2 = 'literal _x0041_ escape'
    $text.Range('B9').Value2 = ('0123456789' * 500)
    $text.Columns.Item(2).ColumnWidth = 40

    $layout = Add-Sheet $book 'Layout'
    $layout.Range('A1').Value2 = 'visible'
    $layout.Range('A2').Value2 = 'in hidden row'
    $layout.Rows.Item(2).Hidden = $true
    $layout.Range('B1').Value2 = 'in hidden column'
    $layout.Columns.Item(2).Hidden = $true
    $layout.Range('D4:F6').Merge()
    $layout.Range('D4').Value2 = 'merged block'
    $layout.Range('A8:C8').Interior.Color = 0x99CCFF
    $layout.Range('A10').Value2 = 'hello'
    $layout.Range('A10').AddComment('A note on this cell') | Out-Null
    $layout.Range('Z100').Value2 = 'further out'
    $layout.Range('XFD1048576').Value2 = 'last cell on the sheet'
    $layout.Rows.Item(3).RowHeight = 40
    $layout.Range('A1').EntireRow.Group() | Out-Null

    Save-Book $excel $book 'text-and-layout.xlsx'
}

# The 1904 date system with date, time and other number formats side by side.
function New-Dates1904($excel) {
    $book = $excel.Workbooks.Add()
    $book.Date1904 = $true
    $sheet = $book.Worksheets.Item(1)
    $sheet.Name = 'Formats'
    $sheet.Range('A1').Value2 = 'Format'
    $sheet.Range('B1').Value2 = 'Value'
    $samples = @(
        @('yyyy-mm-dd', 43538),
        @('m/d/yy', 43538),
        @('dddd, mmmm d, yyyy', 43538),
        @('yyyy-mm-dd hh:mm:ss', 43538.75),
        @('h:mm AM/PM', 0.5),
        @('[h]:mm', 1.5),
        @('mm:ss', 0.00125),
        @('0.00%', 0.125),
        @('#,##0.00 [$EUR]', 1234.5),
        @('0.00E+00', 123456),
        @('# ?/?', 2.5),
        @('"days: "0', 7),
        @('[Red]0;[Blue]-0', -3),
        @('General', 3.25)
    )
    for ($i = 0; $i -lt $samples.Count; $i++) {
        $row = $i + 2
        $sheet.Cells.Item($row, 1).NumberFormat = '@'
        $sheet.Cells.Item($row, 1).Value2 = $samples[$i][0]
        $sheet.Cells.Item($row, 2).Value2 = [double]$samples[$i][1]
        $format = $samples[$i][0]
        if ($format -eq 'General') { continue }
        try {
            $sheet.Cells.Item($row, 2).NumberFormat = $format
        } catch {
            # Some Excel locales reject elapsed hours unless written with their own hour letter.
            $sheet.Cells.Item($row, 2).NumberFormatLocal = $format -replace '\[h\]', '[t]'
        }
    }
    $sheet.Range('D2').Formula = '=DATE(2023,3,15)'
    $sheet.Range('D2').NumberFormat = 'yyyy-mm-dd'
    $sheet.Range('D3').Formula = '=B2+1'
    $sheet.Range('D3').NumberFormat = 'yyyy-mm-dd'
    $sheet.Columns.Item(1).ColumnWidth = 24
    $sheet.Columns.Item(2).ColumnWidth = 24
    Save-Book $excel $book 'dates-1904.xlsx'
}

# Written directly, the way non-Excel tools do: namespace prefixes, inline strings, cells and rows
# without references, parts outside xl/, an absolute relationship target, no styles or strings.
function New-ForeignWriter {
    Add-Type -AssemblyName System.IO.Compression
    $parts = [ordered]@{
        '[Content_Types].xml' = '<?xml version="1.0" encoding="UTF-8"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Override PartName="/data/book.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/data/first.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/><Override PartName="/data/second.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/></Types>'
        '_rels/.rels' = '<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="wb" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="/data/book.xml"/></Relationships>'
        'data/book.xml' = '<?xml version="1.0" encoding="UTF-8"?><x:workbook xmlns:x="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><x:sheets><x:sheet name="Prefixed" sheetId="1" r:id="one"/><x:sheet name="No references" sheetId="2" r:id="two"/></x:sheets></x:workbook>'
        'data/_rels/book.xml.rels' = '<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="one" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="first.xml"/><Relationship Id="two" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="/data/second.xml"/></Relationships>'
        'data/first.xml' = '<?xml version="1.0" encoding="UTF-8"?><x:worksheet xmlns:x="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><x:sheetData><x:row r="1"><x:c r="A1" t="inlineStr"><x:is><x:t>inline &amp; prefixed</x:t></x:is></x:c><x:c r="B1"><x:v>1.5</x:v></x:c></x:row><x:row r="3"><x:c r="A3"><x:f>B1*2</x:f><x:v>3</x:v></x:c><x:c r="B3" t="b"><x:v>1</x:v></x:c></x:row></x:sheetData></x:worksheet>'
        'data/second.xml' = '<?xml version="1.0" encoding="UTF-8"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row><c t="inlineStr"><is><t>first</t></is></c><c><v>2</v></c></row><row/><row><c><v>3</v></c></row></sheetData></worksheet>'
    }
    $path = Join-Path $PSScriptRoot 'foreign-writer.xlsx'
    if (Test-Path $path) { Remove-Item $path -Confirm:$false }
    $zip = [IO.Compression.ZipFile]::Open($path, 'Create')
    try {
        foreach ($name in $parts.Keys) {
            $writer = [IO.StreamWriter]::new($zip.CreateEntry($name).Open(), [Text.UTF8Encoding]::new($false))
            $writer.Write($parts[$name])
            $writer.Dispose()
        }
    } finally {
        $zip.Dispose()
    }
    Write-Output 'wrote foreign-writer.xlsx'
}

Add-Type -AssemblyName System.IO.Compression.FileSystem
New-ForeignWriter

$existing = @(Get-Process excel -ErrorAction SilentlyContinue | ForEach-Object Id)
$excel = New-Object -ComObject Excel.Application
$excel.Visible = $false
$excel.DisplayAlerts = $false
try {
    New-FinancialModel $excel
    New-SalesPivot $excel
    New-FormulaEdgeCases $excel
    New-TextAndLayout $excel
    New-Dates1904 $excel
} finally {
    $excel.Quit()
    [void][Runtime.InteropServices.Marshal]::ReleaseComObject($excel)
    [GC]::Collect()
    [GC]::WaitForPendingFinalizers()
    Start-Sleep -Milliseconds 500
    Get-Process excel -ErrorAction SilentlyContinue |
        Where-Object { $existing -notcontains $_.Id } |
        Stop-Process -Force
}
