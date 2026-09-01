<#
.SYNOPSIS
  fandango リリース成果物（VST3 zip と foobar2000 zip）を作り直す。

.DESCRIPTION
  リリース当日に走らせる。バンドルと DLL を焼き直してから zip を組むので、
  成果物のファイル日時が実行時刻に揃う。

  作るもの:
    dist\fandango-1.0.0-win-vst3.zip   VST3 12 本 + LICENSE + licenses\ + INSTALL.txt
    dist\fandango-fb2k-1.0.0.zip       fb2k コンポーネント + GPL-3 + WTL 例外 + SDK 系ライセンス

  **fb2k 側は VST3 とライセンス構成が違う。** fb2k の DLL は第三者 Rust crate を
  1 つも含まない（crates\fb2k-ffi に [dependencies] が無い）ため licenses\texts\ は同梱せず、
  代わりに foobar2000 SDK / pfc / libPPUI / WTL の 4 件を入れる。
  WTL は Ms-PL で GPL-3 と非互換なので fb2k\LICENSE-EXCEPTION.md の追加許諾も同梱する。

.PARAMETER SkipFb2k
  foobar2000 側を作らない。MSVC Build Tools が無い環境用。
#>
[CmdletBinding()]
param(
    [switch]$SkipFb2k
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

$version = '1.0.0'
$cargo = Join-Path $env:USERPROFILE '.cargo\bin\cargo.exe'
if (-not (Test-Path $cargo)) { throw "cargo が見つからない: $cargo" }

$sdk = 'D:\wrk\fb2k-sdk'
$msbuild = 'C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\MSBuild\Current\Bin\MSBuild.exe'

$crates = @(
    'fandango-spread', 'fandango-raindrop', 'fandango-gate', 'fandango-drawl',
    'fandango-trip-delay', 'fandango-velvet', 'fandango-rotate', 'fandango-crush',
    'fandango-tilt', 'fandango-nice-bulk', 'fandango-nimbus', 'fandango-finalist'
)

function Step($msg) { Write-Host "==> $msg" -ForegroundColor Cyan }

# ---------------------------------------------------------------- VST3

Step 'VST3 バンドルを焼き直す'
$bundleArgs = @('nih-plug', 'bundle')
foreach ($c in $crates) { $bundleArgs += @('-p', $c) }
$bundleArgs += '--release'
& $cargo @bundleArgs
if ($LASTEXITCODE -ne 0) { throw "bundle 失敗 (exit $LASTEXITCODE)" }

$bundled = Get-ChildItem 'target\bundled' -Filter '*.vst3' -Directory
if ($bundled.Count -ne 12) { throw "VST3 バンドルが 12 本でない: $($bundled.Count) 本" }

Step 'VST3 zip を組む'
$stage = 'tmp\zipstage'
Remove-Item -Recurse -Force $stage -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force "$stage\VST3" | Out-Null
Copy-Item 'dist\INSTALL.txt' $stage
Copy-Item 'LICENSE' $stage
Copy-Item 'licenses' $stage -Recurse
Copy-Item 'target\bundled\*.vst3' "$stage\VST3" -Recurse

New-Item -ItemType Directory -Force 'dist' | Out-Null
$vst3Zip = "dist\fandango-$version-win-vst3.zip"
Remove-Item -Force $vst3Zip -ErrorAction SilentlyContinue
Compress-Archive -Path "$stage\*" -DestinationPath $vst3Zip -CompressionLevel Optimal

# ---------------------------------------------------------------- foobar2000

if (-not $SkipFb2k) {
    if (-not (Test-Path $msbuild)) { throw "MSBuild が見つからない: $msbuild`n別マシンなら vswhere -property installationPath で導入先を確認する" }
    if (-not (Test-Path $sdk)) { throw "foobar2000 SDK が見つからない: $sdk" }

    Step 'Rust staticlib を焼き直す'
    & $cargo build --release -p fandango-fb2k-ffi
    if ($LASTEXITCODE -ne 0) { throw "staticlib 失敗 (exit $LASTEXITCODE)" }

    Step 'C++ コンポーネントを焼き直す'
    & $msbuild 'fb2k\foo_dsp_fandango\foo_dsp_fandango.sln' `
        /p:Configuration=Release /p:Platform=x64 /p:PlatformToolset=v143 /m
    if ($LASTEXITCODE -ne 0) { throw "MSBuild 失敗 (exit $LASTEXITCODE)" }

    $dll = 'fb2k\foo_dsp_fandango\Release\foo_dsp_fandango.dll'
    if (-not (Test-Path $dll)) { throw "DLL が出ていない: $dll" }

    Step 'fb2k-component を作る'
    # .fb2k-component は DLL 1 つだけを入れた zip。
    New-Item -ItemType Directory -Force 'fb2k\dist' | Out-Null
    $component = 'fb2k\dist\foo_dsp_fandango.fb2k-component'
    Remove-Item -Force $component -ErrorAction SilentlyContinue
    Compress-Archive -Path $dll -DestinationPath $component -CompressionLevel Optimal

    Step 'fb2k zip を組む'
    $fstage = 'tmp\fb2kstage'
    Remove-Item -Recurse -Force $fstage -ErrorAction SilentlyContinue
    New-Item -ItemType Directory -Force "$fstage\licenses" | Out-Null
    Copy-Item 'dist\INSTALL-fb2k.txt' "$fstage\INSTALL.txt"
    Copy-Item 'LICENSE' $fstage
    Copy-Item 'fb2k\LICENSE-EXCEPTION.md' $fstage
    Copy-Item 'licenses\NON-CARGO.md' "$fstage\licenses"
    Copy-Item $component $fstage

    # SDK 側のライセンス本文。リポジトリ外にあるので実物から写す。
    $texts = @{
        "$sdk\sdk-license.txt"                = 'foobar2000-SDK.txt'
        "$sdk\pfc\pfc-license.txt"            = 'pfc.txt'
        "$sdk\libPPUI\libPPUI-license.txt"    = 'libPPUI.txt'
        "$sdk\wtl10\MS-PL.txt"                = 'WTL-MS-PL.txt'
    }
    foreach ($src in $texts.Keys) {
        if (-not (Test-Path $src)) { throw "ライセンス本文が無い: $src" }
        Copy-Item $src (Join-Path "$fstage\licenses" $texts[$src])
    }

    $fbZip = "dist\fandango-fb2k-$version.zip"
    Remove-Item -Force $fbZip -ErrorAction SilentlyContinue
    Compress-Archive -Path "$fstage\*" -DestinationPath $fbZip -CompressionLevel Optimal
}

# ---------------------------------------------------------------- 結果

Step '結果'
Get-ChildItem 'dist' -Filter '*.zip' |
    Select-Object Name, @{n = 'MB'; e = { [math]::Round($_.Length / 1MB, 2) } }, LastWriteTime |
    Format-Table -AutoSize
