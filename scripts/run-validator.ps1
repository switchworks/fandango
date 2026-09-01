# target/bundled/*.vst3 を Steinberg VST3 validator に通す。
#
# validator.exe は SDK に同梱されていない。ホスティングサンプルをビルドして作る:
#   cmake -B tmp/vst3-build -S <SDK> -G "Visual Studio 17 2022" -A x64 `
#         -DSMTG_ENABLE_VST3_HOSTING_EXAMPLES=ON -DSMTG_ENABLE_VST3_PLUGIN_EXAMPLES=OFF `
#         -DSMTG_CREATE_PLUGIN_LINK=OFF
#   cmake --build tmp/vst3-build --config Release --target validator
# 生成先: tmp/vst3-build/bin/Release/validator.exe

param(
    [string]$Validator = "tmp/vst3-build/bin/Release/validator.exe",
    [string]$BundleDir = "target/bundled",
    [string]$LogDir    = "tmp/validator-logs"
)

$ErrorActionPreference = "Stop"

if (-not (Test-Path $Validator)) {
    throw "validator が見つかりません: $Validator （上のコメントの手順でビルドしてください）"
}
if (-not (Test-Path $LogDir)) {
    New-Item -ItemType Directory -Path $LogDir | Out-Null
}

$failed = 0
foreach ($bundle in Get-ChildItem -Path $BundleDir -Filter *.vst3) {
    $log = Join-Path $LogDir "$($bundle.BaseName).log"
    & $Validator $bundle.FullName *> $log
    $exit = $LASTEXITCODE
    $result = (Select-String -Path $log -Pattern "^Result:" | Select-Object -First 1).Line
    if ($exit -ne 0 -or -not $result -or $result -match "[1-9][0-9]* tests failed") {
        $failed++
        Write-Host ("FAIL  {0,-24} exit={1} {2}" -f $bundle.BaseName, $exit, $result)
    } else {
        Write-Host ("ok    {0,-24} {1}" -f $bundle.BaseName, $result)
    }
}

Write-Host ""
if ($failed -gt 0) {
    Write-Host "$failed 本が失敗。ログ: $LogDir"
    exit 1
}
Write-Host "全 bundle が validator を通過。ログ: $LogDir"
