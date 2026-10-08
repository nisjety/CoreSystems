# Authors the two "real Office" fixtures that tests/test_chat_extract.py reads.
#
# The hand-built zips in that test file prove the code against what we THINK
# Excel and PowerPoint write. These two files are written by Excel and
# PowerPoint themselves (COM automation), so the extractor is also proven
# against what they really store. Re-run after changing a value; the test file
# asserts these exact values.
#
#   powershell -File make-real-fixtures.ps1 [-Only xlsx|pptx]
#
# No customer data and no personal names: every value below is invented, and the
# author fields Office stamps into each package are overwritten afterwards (the
# script then scans the package and refuses to finish if a name is left).
# Keep the files small.
#
# Excel runs invisibly. PowerPoint cannot add a chart without a window to host
# the chart's data sheet, so its deck gets a minimised one. Both apps are quit
# at the end only if this script started them; the script never closes an Excel
# or PowerPoint that was already open.
#
# --- regneark.xlsx -----------------------------------------------------------
#  Budsjett (visible)   A1:D1 merged "Budsjett 2024"; row 2 headers
#                       Frist | Beloep | Post | Andel
#                       rows 3-9: A = date (d.m.yyyy), B = amount, C = post,
#                       D = =B3/$B$10 formatted 0.0 %
#                       A3 = 15.03.2024 (serial 45366); B10 = =SUM(B3:B9) = 35000
#                       D10 = =SUM(D3:D9) = 100.0 %
#                       a column chart over B3:B9
#  Sjekk (visible)      B1 =Budsjett!B10>30000 -> TRUE, B2 =1/0 -> #DIV/0!,
#                       B3 0.5 as h:mm, B4 45292.5 as dd.mm.yyyy hh:mm,
#                       B5 0.255 as 0.0 %, B6 =0.1+0.2 (stored with float noise)
#  Skjult (hidden)      A1 "Skjult notat", A2 "Intern kode 7731"
#
# --- presentasjon.pptx -------------------------------------------------------
#  Slide 1  title "Kvartalsrapport Q1", subtitle "Intern gjennomgang", no notes
#  Slide 2  title "Resultater", a 3x3 table (Region | Omsetning | Vekst;
#           Nord | 120 | 5 %; Sor | 95 | 3 %), notes "Husk aa nevne at
#           Sor-regionen har ny leder fra mars."
#  Slide 3  title "Utvikling", a column chart "Omsetning per kvartal"
#           (Omsetning: 100, 120, 140 over Q1, Q2, Q3), a picture whose alt text
#           is "Firmalogo (test)"
#  Slide 4  hidden; title "Reserveslide", body "Ikke vis denne"
#
# (Norwegian letters are written with [char] codes so the script reads the same
# in Windows PowerShell 5.1 and PowerShell 7 whatever the file's encoding.)

param([ValidateSet('both', 'xlsx', 'pptx')][string]$Only = 'both')

$ErrorActionPreference = 'Stop'
$here = $PSScriptRoot
$oe = [char]0x00F8   # o-slash
$aa = [char]0x00E5   # a-ring

# Office processes that were already running; the script never touches those.
$existingExcel = @(Get-Process -Name EXCEL -ErrorAction SilentlyContinue | ForEach-Object { $_.Id })

function Release($o) { if ($null -ne $o) { [void][System.Runtime.InteropServices.Marshal]::ReleaseComObject($o) } }

Add-Type -AssemblyName System.IO.Compression, System.IO.Compression.FileSystem

# Office writes the signed-in user's name into docProps/core.xml (creator and
# lastModifiedBy), and a chart's embedded data workbook carries its own copy. A
# fixture committed to the repo must not carry a real name, so both are
# overwritten and the result is scanned.
function Set-CoreAuthors([System.IO.Compression.ZipArchive]$zip) {
  $entry = $zip.GetEntry('docProps/core.xml')
  if ($null -ne $entry) {
    $reader = New-Object System.IO.StreamReader($entry.Open(), [System.Text.Encoding]::UTF8)
    $xml = $reader.ReadToEnd()
    $reader.Dispose()
    $xml = [regex]::Replace($xml, '(<dc:creator>)[^<]*(</dc:creator>)', '${1}Verevon fixture${2}')
    $xml = [regex]::Replace($xml, '(<cp:lastModifiedBy>)[^<]*(</cp:lastModifiedBy>)', '${1}Verevon fixture${2}')
    $entry.Delete()
    $fresh = $zip.CreateEntry('docProps/core.xml')
    $writer = New-Object System.IO.StreamWriter($fresh.Open(), (New-Object System.Text.UTF8Encoding($false)))
    $writer.Write($xml)
    $writer.Dispose()
  }
  foreach ($nested in @($zip.Entries | Where-Object { $_.FullName -match '/embeddings/[^/]+\.xlsx$' })) {
    $memory = New-Object System.IO.MemoryStream
    $source = $nested.Open()
    $source.CopyTo($memory)
    $source.Dispose()
    $inner = New-Object System.IO.Compression.ZipArchive($memory, [System.IO.Compression.ZipArchiveMode]::Update, $true)
    Set-CoreAuthors $inner
    $inner.Dispose()
    $name = $nested.FullName
    $nested.Delete()
    $fresh = $zip.CreateEntry($name)
    $target = $fresh.Open()
    $memory.Position = 0
    $memory.CopyTo($target)
    $target.Dispose()
    $memory.Dispose()
  }
}

function Clear-PackageAuthors($path) {
  $zip = [System.IO.Compression.ZipFile]::Open($path, 'Update')
  try { Set-CoreAuthors $zip } finally { $zip.Dispose() }
}

# Scan every XML part, and every embedded workbook, for the Windows user name
# and the Office user name.
function Test-ZipForNames([System.IO.Compression.ZipArchive]$zip, $needles, $label) {
  foreach ($entry in $zip.Entries) {
    if ($entry.FullName -match '\.(xml|rels)$') {
      $reader = New-Object System.IO.StreamReader($entry.Open(), [System.Text.Encoding]::UTF8)
      $text = $reader.ReadToEnd()
      $reader.Dispose()
      foreach ($needle in $needles) {
        if ($needle -and $text.IndexOf($needle, [StringComparison]::OrdinalIgnoreCase) -ge 0) {
          throw "$label still names a person in $($entry.FullName); fixtures must hold no personal names."
        }
      }
    }
    elseif ($entry.FullName -match '\.xlsx$') {
      $memory = New-Object System.IO.MemoryStream
      $source = $entry.Open()
      $source.CopyTo($memory)
      $source.Dispose()
      $inner = New-Object System.IO.Compression.ZipArchive($memory, [System.IO.Compression.ZipArchiveMode]::Read)
      Test-ZipForNames $inner $needles "$label/$($entry.FullName)"
      $inner.Dispose()
      $memory.Dispose()
    }
  }
}

function Assert-NoPersonalNames($path) {
  $needles = @([Environment]::UserName)
  $officeName = (Get-ItemProperty 'HKCU:\Software\Microsoft\Office\Common\UserInfo' -ErrorAction SilentlyContinue).UserName
  if ($officeName) { $needles += $officeName }
  $zip = [System.IO.Compression.ZipFile]::OpenRead($path)
  try { Test-ZipForNames $zip $needles $path } finally { $zip.Dispose() }
}

# ---------------------------------------------------------------- Excel -----
if ($Only -ne 'pptx') {
$excel = New-Object -ComObject Excel.Application
$excel.Visible = $false
$excel.DisplayAlerts = $false
try {
  # NumberFormat takes the Excel UI locale's decimal separator (a Norwegian Excel rejects '0.0%');
  # the file itself always stores the invariant form, 0.0%.
  $pct = '0' + $excel.International(3) + '0%'
  # Date and time codes are localised too (a Norwegian Excel reads 'yyyy' as literal y's and writes
  # them into the file), so build them from the letters this Excel uses.
  $dayCode = $excel.International(21); $monthCode = $excel.International(20); $yearCode = $excel.International(19)
  $hourCode = $excel.International(22); $minuteCode = $excel.International(23)
  $dateFmt = "${dayCode}.${monthCode}.${yearCode}${yearCode}${yearCode}${yearCode}"
  $timeFmt = "${hourCode}:${minuteCode}${minuteCode}"
  $dateTimeFmt = "${dayCode}${dayCode}.${monthCode}${monthCode}.${yearCode}${yearCode}${yearCode}${yearCode} ${hourCode}${hourCode}:${minuteCode}${minuteCode}"
  $wb = $excel.Workbooks.Add()
  while ($wb.Worksheets.Count -lt 3) { [void]$wb.Worksheets.Add([Type]::Missing, $wb.Worksheets.Item($wb.Worksheets.Count)) }

  $ws = $wb.Worksheets.Item(1)
  $ws.Name = 'Budsjett'
  $ws.Range('A1').Value2 = 'Budsjett 2024'
  $ws.Range('A1:D1').Merge()
  $ws.Range('A2').Value2 = 'Frist'
  $ws.Range('B2').Value2 = "Bel${oe}p"
  $ws.Range('C2').Value2 = 'Post'
  $ws.Range('D2').Value2 = 'Andel'
  $dates = '2024-03-15', '2024-04-01', '2024-04-30', '2024-05-15', '2024-06-01', '2024-06-30', '2024-07-15'
  $amounts = 12000, 4500, 9000, 2500, 3000, 1800, 2200
  $posts = 'Leie', "Str${oe}m", "L${oe}nn", 'Forsikring', 'Kontor', 'Programvare', 'Annet'
  for ($i = 0; $i -lt 7; $i++) {
    $r = 3 + $i
    $ws.Range("A$r").Value2 = [double]([datetime]$dates[$i]).ToOADate()
    $ws.Range("A$r").NumberFormat = $dateFmt
    $ws.Range("B$r").Value2 = [double]$amounts[$i]
    $ws.Range("C$r").Value2 = $posts[$i]
    $ws.Range("D$r").Formula = "=B$r/`$B`$10"
    $ws.Range("D$r").NumberFormat = $pct
  }
  $ws.Range('A10').Value2 = 'Sum'
  $ws.Range('B10').Formula = '=SUM(B3:B9)'
  $ws.Range('D10').Formula = '=SUM(D3:D9)'
  $ws.Range('D10').NumberFormat = $pct
  $shape = $ws.Shapes.AddChart2(201, 51, 330, 20, 300, 180)   # xlColumnClustered
  $shape.Chart.SetSourceData($ws.Range('B3:B9'))
  $shape.Chart.HasTitle = $true
  $shape.Chart.ChartTitle.Text = 'Budsjett per post'

  $check = $wb.Worksheets.Item(2)
  $check.Name = 'Sjekk'
  $check.Range('A1').Value2 = "Over grense?"
  $check.Range('B1').Formula = '=Budsjett!B10>30000'
  $check.Range('A2').Value2 = 'Feil'
  $check.Range('B2').Formula = '=1/0'
  $check.Range('A3').Value2 = 'Tid'
  $check.Range('B3').Value2 = 0.5
  $check.Range('B3').NumberFormat = $timeFmt
  $check.Range('A4').Value2 = 'Dato og tid'
  $check.Range('B4').Value2 = 45292.5
  $check.Range('B4').NumberFormat = $dateTimeFmt
  $check.Range('A5').Value2 = 'Andel'
  $check.Range('B5').Value2 = 0.255
  $check.Range('B5').NumberFormat = $pct
  $check.Range('A6').Value2 = "St${oe}y"
  $check.Range('B6').Formula = '=0.1+0.2'

  $hidden = $wb.Worksheets.Item(3)
  $hidden.Name = 'Skjult'
  $hidden.Range('A1').Value2 = 'Skjult notat'
  $hidden.Range('A2').Value2 = 'Intern kode 7731'
  $hidden.Visible = 0   # xlSheetHidden

  $wb.Worksheets.Item(1).Activate()
  $path = Join-Path $here 'regneark.xlsx'
  if (Test-Path $path) { Remove-Item $path }
  $wb.SaveAs($path, 51)   # xlOpenXMLWorkbook
  $wb.Close($false)
  Release $wb
  Clear-PackageAuthors $path
  Assert-NoPersonalNames $path
  Write-Host "wrote $path"
}
finally {
  $new = @(Get-Process -Name EXCEL -ErrorAction SilentlyContinue | Where-Object { $existingExcel -notcontains $_.Id })
  # Only quit an instance this script started; never close someone's own Excel.
  if ($new.Count -gt 0) { $excel.Quit() }
  Release $excel
  [GC]::Collect(); [GC]::WaitForPendingFinalizers()
}
}

# ----------------------------------------------------------- PowerPoint -----
if ($Only -ne 'xlsx') {
# A 8x8 PNG for the picture's alt text.
Add-Type -AssemblyName System.Drawing
$png = Join-Path $env:TEMP 'verevon-h1-logo.png'
$bmp = New-Object System.Drawing.Bitmap 8, 8
for ($x = 0; $x -lt 8; $x++) { for ($y = 0; $y -lt 8; $y++) { $bmp.SetPixel($x, $y, [System.Drawing.Color]::SteelBlue) } }
$bmp.Save($png, [System.Drawing.Imaging.ImageFormat]::Png)
$bmp.Dispose()

$existingPpt = @(Get-Process -Name POWERPNT -ErrorAction SilentlyContinue | ForEach-Object { $_.Id })
$pp = New-Object -ComObject PowerPoint.Application
try {
  # AddChart2 needs a window to host the chart data sheet, so the deck gets one; it is minimised.
  $pres = $pp.Presentations.Add(-1)
  $pp.WindowState = 2   # ppWindowMinimized
  $s1 = $pres.Slides.Add(1, 1)       # ppLayoutTitle
  $s1.Shapes.Item(1).TextFrame.TextRange.Text = 'Kvartalsrapport Q1'
  $s1.Shapes.Item(2).TextFrame.TextRange.Text = 'Intern gjennomgang'

  $s2 = $pres.Slides.Add(2, 11)      # ppLayoutTitleOnly
  $s2.Shapes.Item(1).TextFrame.TextRange.Text = 'Resultater'
  $table = $s2.Shapes.AddTable(3, 3, 50, 140, 500, 90)
  $cells = @(@('Region', 'Omsetning', 'Vekst'), @('Nord', '120', '5 %'), @(('S' + $oe + 'r'), '95', '3 %'))
  for ($r = 0; $r -lt 3; $r++) { for ($c = 0; $c -lt 3; $c++) { $table.Table.Cell($r + 1, $c + 1).Shape.TextFrame.TextRange.Text = $cells[$r][$c] } }
  $s2.NotesPage.Shapes.Placeholders.Item(2).TextFrame.TextRange.Text = "Husk ${aa} nevne at S${oe}r-regionen har ny leder fra mars."

  $s3 = $pres.Slides.Add(3, 11)
  $s3.Shapes.Item(1).TextFrame.TextRange.Text = 'Utvikling'
  $chartShape = $s3.Shapes.AddChart2(201, 51, 50, 140, 420, 280)
  $chart = $chartShape.Chart
  $chart.ChartData.ActivateChartDataWindow()   # Activate() leaves the chart blind to cell edits
  Start-Sleep -Seconds 2
  $book = $null
  for ($try = 0; $try -lt 40 -and $null -eq $book; $try++) {   # the data sheet opens asynchronously
    Start-Sleep -Milliseconds 500
    $book = $chart.ChartData.Workbook
  }
  if ($null -eq $book) { throw 'The chart data workbook did not open.' }
  $sheet = $book.Worksheets.Item(1)
  $sheet.Range('B1').Value2 = 'Omsetning'
  $sheet.Range('A2').Value2 = 'Q1'; $sheet.Range('B2').Value2 = 100
  $sheet.Range('A3').Value2 = 'Q2'; $sheet.Range('B3').Value2 = 120
  $sheet.Range('A4').Value2 = 'Q3'; $sheet.Range('B4').Value2 = 140
  if ($sheet.ListObjects.Count -gt 0) { $sheet.ListObjects.Item(1).Resize($sheet.Range('A1:B4')) }
  $sheet.Range('C1:D5').ClearContents()
  $sheet.Range('A5:B5').ClearContents()
  $chart.SetSourceData("='" + $sheet.Name + "'!`$A`$1:`$B`$4")
  Start-Sleep -Seconds 1
  $values = @($chart.SeriesCollection(1).Values)
  if ($values.Count -ne 3 -or $values[0] -ne 100 -or $values[2] -ne 140 -or $chart.SeriesCollection(1).Name -ne 'Omsetning') {
    throw 'The chart did not pick up its data; refusing to save a fixture with sample values.'
  }
  $book.Close()
  $chart.HasTitle = $true
  $chart.ChartTitle.Text = 'Omsetning per kvartal'
  $picture = $s3.Shapes.AddPicture($png, 0, -1, 490, 150, 40, 40)
  $picture.AlternativeText = 'Firmalogo (test)'

  $s4 = $pres.Slides.Add(4, 2)       # ppLayoutText
  $s4.Shapes.Item(1).TextFrame.TextRange.Text = 'Reserveslide'
  $s4.Shapes.Item(2).TextFrame.TextRange.Text = 'Ikke vis denne'
  $s4.SlideShowTransition.Hidden = -1

  # Keep the file small: drop the slide layouts no slide uses.
  $usedLayouts = @{}
  foreach ($slide in $pres.Slides) { $usedLayouts[$slide.CustomLayout.Name] = $true }
  $layouts = $pres.SlideMaster.CustomLayouts
  for ($i = $layouts.Count; $i -ge 1; $i--) {
    $layout = $layouts.Item($i)
    if (-not $usedLayouts.ContainsKey($layout.Name)) { try { $layout.Delete() } catch { } }
  }

  $path = Join-Path $here 'presentasjon.pptx'
  if (Test-Path $path) { Remove-Item $path }
  $pres.SaveAs($path, 24)            # ppSaveAsOpenXMLPresentation
  $pres.Close()
  Clear-PackageAuthors $path
  Assert-NoPersonalNames $path
  Write-Host "wrote $path"
}
finally {
  Remove-Item $png -ErrorAction SilentlyContinue
  $newPpt = @(Get-Process -Name POWERPNT -ErrorAction SilentlyContinue | Where-Object { $existingPpt -notcontains $_.Id })
  # Only quit an instance this script started; never close someone's own PowerPoint.
  if ($newPpt.Count -gt 0) { $pp.Quit() }
  Release $pp
  [GC]::Collect(); [GC]::WaitForPendingFinalizers()
  # Editing a chart's data sheet starts an Excel of its own; it can outlive PowerPoint.
  Start-Sleep -Seconds 3
  Get-Process -Name EXCEL -ErrorAction SilentlyContinue | Where-Object { $existingExcel -notcontains $_.Id } | Stop-Process -Force -ErrorAction SilentlyContinue
}
}
