param([Parameter(Mandatory=$true)][string]$Source, [Parameter(Mandatory=$true)][string]$Destination)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
$sourceImage = [System.Drawing.Image]::FromFile($Source)
try {
  foreach ($entry in @(@('StoreLogo.png',50,50),@('Square44x44Logo.png',44,44),@('Square150x150Logo.png',150,150),@('Wide310x150Logo.png',310,150))) {
    $bitmap = [System.Drawing.Bitmap]::new($entry[1], $entry[2])
    $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
    try {
      $graphics.Clear([System.Drawing.Color]::Transparent)
      $graphics.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
      $size = [Math]::Min($entry[1], $entry[2])
      $graphics.DrawImage($sourceImage, [int](($entry[1]-$size)/2), [int](($entry[2]-$size)/2), $size, $size)
      $bitmap.Save((Join-Path $Destination $entry[0]), [System.Drawing.Imaging.ImageFormat]::Png)
    } finally { $graphics.Dispose(); $bitmap.Dispose() }
  }
} finally { $sourceImage.Dispose() }
