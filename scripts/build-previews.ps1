<#
.SYNOPSIS
全 GUI プラグインの VST3 bundle と、単独 GUI プレビュー exe をまとめて生成する。

.DESCRIPTION
プレビュー exe は nih-plug の standalone ラッパーを使い、VST3 ホストを介さずに
実プラグインと同じ editor を開く。開発・目視確認専用であり、配布物には含めない。

.PARAMETER Only
crate ディレクトリ名を指定すると、その 1 つだけを処理する (例: -Only nimbus)。

.PARAMETER SkipBundle
VST3 bundle の生成を省略し、プレビュー exe だけを作る。

.EXAMPLE
.\scripts\build-previews.ps1
.EXAMPLE
.\scripts\build-previews.ps1 -Only nimbus
#>
param(
    [string]$Only,
    [switch]$SkipBundle
)

$ErrorActionPreference = 'Stop'
$cargo = Join-Path $env:USERPROFILE '.cargo\bin\cargo.exe'
if (-not (Test-Path $cargo)) { throw "cargo が見つかりません: $cargo" }

# crate ディレクトリ名 = package 名 (fandango- 接頭辞) / exe 名 (_preview 接尾辞)
$crates = @(
    'crush', 'drawl', 'finalist', 'gate', 'nice-bulk', 'nimbus', 'raindrop',
    'rotate', 'spread', 'tilt', 'trip-delay', 'velvet'
)

if ($Only) {
    if ($crates -notcontains $Only) {
        throw "未知の crate: $Only (対象: $($crates -join ', '))"
    }
    $crates = @($Only)
}

$packages = $crates | ForEach-Object { "fandango-$_" }

foreach ($crate in $crates) {
    $package = "fandango-$crate"
    $bin = ($crate -replace '-', '_') + '_preview'
    Write-Output "[preview] $package -> $bin.exe"
    & $cargo build -p $package --release --features standalone-preview --bin $bin
    if ($LASTEXITCODE -ne 0) { throw "プレビュー exe のビルドに失敗: $package" }
}

if (-not $SkipBundle) {
    $bundleArgs = @('nih-plug', 'bundle')
    foreach ($package in $packages) { $bundleArgs += @('-p', $package) }
    $bundleArgs += '--release'
    Write-Output "[bundle] $($packages -join ', ')"
    & $cargo @bundleArgs
    if ($LASTEXITCODE -ne 0) { throw 'VST3 bundle の生成に失敗' }
}

Write-Output ''
Write-Output 'Preview exe: target\release\<crate>_preview.exe'
foreach ($crate in $crates) {
    $bin = ($crate -replace '-', '_') + '_preview'
    Write-Output ("  {0,-22} target\release\{1}.exe" -f $crate, $bin)
}
if (-not $SkipBundle) {
    Write-Output 'VST3:        target\bundled\fandango - <name>.vst3'
}
