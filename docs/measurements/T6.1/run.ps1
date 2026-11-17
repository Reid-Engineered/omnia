param([string]$OutputDirectory)
$ErrorActionPreference = 'Stop'
if (-not $OutputDirectory) { throw 'Choose a new output directory for this diagnostic.' }
if (Test-Path -LiteralPath $OutputDirectory) { throw 'Keep previous runs; choose a new directory.' }
New-Item -ItemType Directory -Path $OutputDirectory | Out-Null
# Run from the repository root. Never discard/retry a timed run to get green.
# The test fixes all four cases and rotates them for ten repetitions.
cargo test -p omnia-doc --release --test add_wires_timing --no-run
if ($LASTEXITCODE -ne 0) { throw 'Build failed before timing.' }
$binary = Get-ChildItem target/release/deps/add_wires_timing-*.exe |
    Sort-Object LastWriteTime -Descending | Select-Object -First 1
Get-FileHash -LiteralPath $binary.FullName -Algorithm SHA256 |
    ConvertTo-Json | Set-Content (Join-Path $OutputDirectory 'binary.json')
function Save-Conditions([string]$Name) {
    $conditions = [ordered]@{
        utc = (Get-Date).ToUniversalTime().ToString('o')
        cpu = (Get-CimInstance Win32_Processor).Name
        gpu = @((Get-CimInstance Win32_VideoController).Name)
        os = (Get-CimInstance Win32_OperatingSystem).Version
        panel = @((Get-CimInstance Win32_VideoController) |
            Select-Object CurrentHorizontalResolution, CurrentVerticalResolution, CurrentRefreshRate)
        dpi = (Get-ItemProperty 'HKCU:/Control Panel/Desktop/WindowMetrics').AppliedDPI
        battery_status = (Get-CimInstance Win32_Battery).BatteryStatus
        power_scheme = (powercfg /getactivescheme | Out-String).Trim()
        free_bytes = (Get-PSDrive C).Free
        governor = 'not applicable on Windows'
    }
    $conditions | ConvertTo-Json -Depth 5 | Set-Content (Join-Path $OutputDirectory $Name)
}
Save-Conditions 'before-conditions.json'
& $binary.FullName --ignored --nocapture *> (Join-Path $OutputDirectory 'raw.txt')
$result = $LASTEXITCODE
Save-Conditions 'after-conditions.json'
if ($result -ne 0) { throw 'Diagnostic incomplete; retain all output.' }
