param(
    [string]$TargetDir = 'target-standalone-package',
    [string]$OutputDir = 'dist\OpenViKey-preview'
)

$ErrorActionPreference = 'Stop'
$Root = Split-Path -Parent $PSScriptRoot
$Target = Join-Path $Root $TargetDir
$Output = Join-Path $Root $OutputDir
$env:CARGO_TARGET_DIR = $Target

Push-Location $Root
try {
    cargo build -p openvikey-win --bin openvikey-win --release
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed with exit code $LASTEXITCODE" }

    New-Item -ItemType Directory -Force -Path $Output | Out-Null
    $SourceExe = Join-Path $Target 'release\openvikey-win.exe'
    $SourceIcon = Join-Path $Root 'crates\openvikey-win\assets\openvikey.ico'
    $ProductExe = Join-Path $Output 'OpenViKey.exe'
    $ProductIcon = Join-Path $Output 'OpenViKey.ico'
    Copy-Item -Force $SourceExe $ProductExe
    Copy-Item -Force $SourceIcon $ProductIcon

    @'
OpenViKey standalone preview

1. Tắt UniKey/EVKey hoặc bộ gõ hook khác.
2. Chạy OpenViKey.exe. Icon V/E xuất hiện ở system tray.
3. Click trái icon để đổi V/E; click phải để chọn Telex/VNI, gợi ý, autostart, xem rule hoặc Thoát.
4. OpenViKey không đăng ký TSF và không xuất hiện trong Win+Space.

Preview dùng development lexicon và plaintext local development stores. Không dùng với dữ liệu nhạy cảm ngoài các password-field gates đã hỗ trợ.
'@ | Set-Content (Join-Path $Output 'README.txt') -Encoding UTF8
    Copy-Item -Force (Join-Path $Root 'STANDALONE-TEST-GUIDE.txt') (Join-Path $Output 'HUONG-DAN-CHAY-THU.txt')

    $Sha = [System.Security.Cryptography.SHA256]::Create()
    $Stream = [System.IO.File]::OpenRead($ProductExe)
    try {
        $Hash = ([System.BitConverter]::ToString($Sha.ComputeHash($Stream))).Replace('-', '')
    } finally {
        $Stream.Dispose()
        $Sha.Dispose()
    }
    "package=$Output"
    "exe=$ProductExe"
    "icon=$ProductIcon"
    "sha256=$Hash"
} finally {
    Pop-Location
}
