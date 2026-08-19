$ErrorActionPreference = 'Continue'

$OpenViKeyClsid = '{741B179E-BF99-4EA2-BDDF-B14AD06E7A60}'
$OpenViKeyTipMarker = '741B179E-BF99-4EA2-BDDF-B14AD06E7A60'
$ComPath = "Registry::HKEY_CURRENT_USER\Software\Classes\CLSID\$OpenViKeyClsid"
$Package = Join-Path (Split-Path -Parent $PSScriptRoot) 'dist\OpenViKey-preview\OpenViKey.exe'

$Loaded = @(
    Get-Process -ErrorAction SilentlyContinue | ForEach-Object {
        $Process = $_
        try {
            $Modules = $Process.Modules | Where-Object { $_.ModuleName -eq 'openvikey_win_tsf.dll' }
            foreach ($Module in $Modules) {
                [pscustomobject]@{
                    Process = $Process.ProcessName
                    Id = $Process.Id
                    Dll = $Module.FileName
                }
            }
        } catch {
            # Higher-integrity/system processes may deny module enumeration.
        }
    }
)

$Tips = @(
    Get-WinUserLanguageList | ForEach-Object { $_.InputMethodTips }
)
$ProductProcesses = @(
    Get-Process -Name OpenViKey, openvikey-win -ErrorAction SilentlyContinue
)

$Checks = [ordered]@{
    PackageExists = Test-Path $Package
    ComClassAbsent = -not (Test-Path $ComPath)
    LanguageTipAbsent = -not [bool]($Tips -match $OpenViKeyTipMarker)
    TsfDllNotLoaded = $Loaded.Count -eq 0
    ProductNotRunningYet = $ProductProcesses.Count -eq 0
}

Write-Host '=== OpenViKey standalone preflight ==='
foreach ($Entry in $Checks.GetEnumerator()) {
    $Label = if ($Entry.Value) { 'PASS' } else { 'FAIL' }
    $Color = if ($Entry.Value) { 'Green' } else { 'Red' }
    Write-Host ("[{0}] {1}" -f $Label, $Entry.Key) -ForegroundColor $Color
}

Write-Host ''
Write-Host 'Windows input methods:'
$Tips | ForEach-Object { Write-Host "  $_" }

if ($Loaded.Count -gt 0) {
    Write-Host ''
    Write-Host 'Processes still loading the historical TSF DLL:' -ForegroundColor Yellow
    $Loaded | Format-Table -AutoSize
}

Write-Host ''
Write-Host "Standalone package: $Package"

if (($Checks.Values | Where-Object { -not $_ }).Count -gt 0) {
    Write-Host ''
    Write-Host 'Preflight has failures. Do not start typing tests yet; report this output in the new session.' -ForegroundColor Red
    exit 1
}

Write-Host ''
Write-Host 'Machine is ready for the standalone test.' -ForegroundColor Green
exit 0
