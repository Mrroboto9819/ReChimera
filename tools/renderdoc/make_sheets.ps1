Add-Type -AssemblyName System.Drawing
$texDir = "C:\Users\flast\AppData\Local\Temp\claude\C--Users-flast-Documents-project-ReChimera\93ec6b94-f8d0-40f9-948a-8679999b3bdf\scratchpad\renderdoc_export_live\textures"
$outDir = "C:\Users\flast\AppData\Local\Temp\claude\C--Users-flast-Documents-project-ReChimera\93ec6b94-f8d0-40f9-948a-8679999b3bdf\scratchpad\renderdoc_export_live\sheets"
New-Item -ItemType Directory -Force $outDir | Out-Null
$files = Get-ChildItem $texDir -Filter "*.png" | Sort-Object Name
$thumb = 96; $cols = 8; $perSheet = 64
$font = New-Object System.Drawing.Font("Consolas", 8)
$brush = [System.Drawing.Brushes]::Yellow
$bg = [System.Drawing.Brushes]::Black
$sheetIdx = 0
for ($start = 0; $start -lt $files.Count; $start += $perSheet) {
    $chunk = $files[$start..([Math]::Min($start + $perSheet - 1, $files.Count - 1))]
    $rows = [Math]::Ceiling($chunk.Count / $cols)
    $cellH = $thumb + 14
    $bmp = New-Object System.Drawing.Bitmap ($cols * $thumb), ($rows * $cellH)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.FillRectangle($bg, 0, 0, $bmp.Width, $bmp.Height)
    for ($i = 0; $i -lt $chunk.Count; $i++) {
        $f = $chunk[$i]
        $x = ($i % $cols) * $thumb
        $y = [Math]::Floor($i / $cols) * $cellH
        try {
            $img = [System.Drawing.Image]::FromFile($f.FullName)
            $g.DrawImage($img, $x, $y, $thumb, $thumb)
            $img.Dispose()
        } catch {}
        $id = ($f.Name -split "_")[1]
        $g.DrawString($id, $font, $brush, $x, $y + $thumb)
    }
    $g.Dispose()
    $out = Join-Path $outDir ("sheet_{0:D2}.png" -f $sheetIdx)
    $bmp.Save($out, [System.Drawing.Imaging.ImageFormat]::Png)
    $bmp.Dispose()
    $sheetIdx++
}
Write-Output "sheets: $sheetIdx"
