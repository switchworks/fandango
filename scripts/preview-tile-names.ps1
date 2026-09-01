<#
.SYNOPSIS
  12 タイル画像へ crate 名を重ねた 1 枚を出す。フォント選びの試行用。

.DESCRIPTION
  `plans/promo_video_v01.md` §4 型 B の見え方をフォントを替えながら確認するためのもの。
  タイル画像 (tmp/shots/tile.png) は事前に生成しておくこと:

      ~/.cargo/bin/cargo run -p fandango-shots -- tile --out tmp/shots/tile.png

  本スクリプトは ffmpeg の drawtext で重ねるだけで、shots も GUI も変更しない。
  本番の C4 は上向きワイプが要るため shots 側 (egui) で描く (§10.2)。これは静止の下見用。

.EXAMPLE
  .\scripts\preview-tile-names.ps1
  .\scripts\preview-tile-names.ps1 -Font "C:\Windows\Fonts\YuGothB.ttc" -Size 64 -Border 8
  .\scripts\preview-tile-names.ps1 -Font "C:\Windows\Fonts\impact.ttf" -Out tmp\shots\try_impact.png
#>
[CmdletBinding()]
param(
    # 試したいフォントファイル。.ttf / .otf / .ttc
    [string]$Font = "D:\wrk\fandango\assets\fonts\WDXLLubrifontJPN-Regular.ttf",

    # 文字の高さ (px)。タイルは 400x200 なので 48-96 が実用域。80 が確定値
    [int]$Size = 80,

    # 縁取りの太さ (px)。0 で縁取り無し
    [int]$Border = 9,

    # 文字色
    [string]$Color = "white",

    # 縁取り色。既定はタイルの地色
    [string]$BorderColor = "0x171717",

    # 入力のタイル画像
    [string]$Tile = "tmp\shots\tile.png",

    # 出力
    [string]$Out = "tmp\shots\tile_named.png",

    # ffmpeg の実体
    [string]$Ffmpeg = "C:\ffmpeg\bin\ffmpeg.exe"
)

$ErrorActionPreference = "Stop"

if (-not (Test-Path $Ffmpeg))  { throw "ffmpeg が無い: $Ffmpeg" }
if (-not (Test-Path $Font))    { throw "フォントが無い: $Font" }
if (-not (Test-Path $Tile))    {
    throw "タイル画像が無い: $Tile`n先に生成する: cargo run -p fandango-shots -- tile --out $Tile"
}

# --- crates/shots/src/tile.rs の実測値 (2026-08-26) ---
$TILE_W   = 400
$TILE_H   = 200
$GAP      = 64
$HEADER_H = 208

# 並びは Target enum 順 (アルファベット順)。copy.toml の crate 順ではない
$names = @(
    "CRUSH",  "DRAWL", "FINALIST",   "GATE",
    "NICE-BULK", "NIMBUS", "RAINDROP", "ROTATE",
    "SPREAD", "TILT",  "TRIP-DELAY", "VELVET"
)

# ffmpeg のフィルタ引数用にパスを直す: C:\a\b.ttf -> C\:/a/b.ttf
$fontArg = ($Font -replace '\\', '/') -replace '^([A-Za-z]):', '$1\:'

$filters = New-Object System.Collections.Generic.List[string]
for ($i = 0; $i -lt $names.Count; $i++) {
    $col = $i % 4
    $row = [math]::Floor($i / 4)
    $x0  = $GAP + $col * ($TILE_W + $GAP)
    $y0  = $HEADER_H + $row * ($TILE_H + $GAP)

    # タイル中央寄せ。tw/th は drawtext が描く文字の実寸
    $cx = "$x0+($TILE_W-tw)/2"
    $cy = "$y0+($TILE_H-th)/2"

    $t = $names[$i]
    # `${}` で囲まないと PowerShell が `$Color:borderw` をドライブ修飾変数として解釈する
    $filters.Add("drawtext=fontfile='$fontArg':text='$t':fontsize=${Size}" +
                 ":fontcolor=${Color}:borderw=${Border}:bordercolor=${BorderColor}" +
                 ":x=${cx}" + ":y=${cy}")
}

$vf = [string]::Join(",", $filters)

$outDir = Split-Path -Parent $Out
if ($outDir -and -not (Test-Path $outDir)) { New-Item -ItemType Directory -Force $outDir | Out-Null }

& $Ffmpeg -hide_banner -loglevel error -y -i $Tile -vf $vf $Out
if ($LASTEXITCODE -ne 0) { throw "ffmpeg が失敗した (exit $LASTEXITCODE)" }

Write-Host "出力: $Out"
Write-Host "  font   : $Font"
Write-Host "  size   : $Size"
Write-Host "  border : $Border ($BorderColor)"
