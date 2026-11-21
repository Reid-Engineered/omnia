# A1.1: Doc::fork time and size on 10k. Non-reference record (certifies nothing).
# Ten repetitions of the one frozen case (fork_10k), condition snapshots before and after each,
# the binary hashed before and after. One case, so rotation is trivial.
# Usage: pwsh docs/measurements/A1.1/run.ps1 docs/measurements/A1.1/windows
param([string]$OutputDirectory)
$ErrorActionPreference = 'Stop'
if (-not $OutputDirectory) { throw 'Specify a fresh output directory (e.g. docs/measurements/A1.1/windows).' }
if (Test-Path -LiteralPath $OutputDirectory) { throw "Output directory already exists: $OutputDirectory" }

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..\..\..')).Path
Push-Location $repoRoot
try {
    $buildOutput = & cargo bench -p omnia-bench --bench fork --no-run --locked --message-format=json
    if ($LASTEXITCODE -ne 0) { throw 'Build failed before timing.' }
    $executables = @()
    foreach ($line in $buildOutput) {
        if ($line -and $line.StartsWith('{')) {
            $msg = $line | ConvertFrom-Json
            if ($msg.reason -eq 'compiler-artifact' -and $msg.target.name -eq 'fork' -and $msg.executable) {
                $executables += $msg.executable
            }
        }
    }
    if ($executables.Count -ne 1) { throw "Expected 1 fork executable, found $($executables.Count)" }
    $binary = $executables[0]

    New-Item -ItemType Directory -Path $OutputDirectory | Out-Null
    $rawDir = Join-Path $OutputDirectory 'raw'
    $critDir = Join-Path $OutputDirectory 'criterion'
    New-Item -ItemType Directory -Path $rawDir | Out-Null
    New-Item -ItemType Directory -Path $critDir | Out-Null

    function Save-Conditions([string]$Path) {
        $battery = Get-CimInstance Win32_Battery -ErrorAction SilentlyContinue
        $video = @(Get-CimInstance Win32_VideoController)
        $conditions = [ordered]@{
            utc = (Get-Date).ToUniversalTime().ToString('o')
            cpu = (Get-CimInstance Win32_Processor).Name
            ram_bytes = (Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory
            gpu = @($video | ForEach-Object { $_.Name })
            os = (Get-CimInstance Win32_OperatingSystem | Select-Object Caption, Version, BuildNumber)
            panel = @($video | Select-Object Name, CurrentHorizontalResolution, CurrentVerticalResolution, CurrentRefreshRate)
            dpi = (Get-ItemProperty 'HKCU:/Control Panel/Desktop/WindowMetrics').AppliedDPI
            battery_status = if ($battery) { $battery.BatteryStatus } else { $null }
            power_online = if ($battery) { $battery.BatteryStatus -ne 1 } else { $null }
            power_scheme = (powercfg /getactivescheme | Out-String).Trim()
            free_bytes = (Get-PSDrive C).Free
            governor = 'not applicable on Windows'
            processes = @(Get-Process | Sort-Object CPU -Descending | Select-Object -First 25 Name, Id, @{Name='CpuSec';Expression={if ($null -ne $_.CPU) { [math]::Round($_.CPU, 2) } else { 0 }}})
        }
        $conditions | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $Path
    }

    $binHashBefore = (Get-FileHash -LiteralPath $binary -Algorithm SHA256).Hash
    $headBefore = (& git rev-parse HEAD).Trim()
    [ordered]@{
        utc = (Get-Date).ToUniversalTime().ToString('o')
        rustc = (& rustc -Vv | Out-String).Trim()
        cargo = (& cargo -V | Out-String).Trim()
        binary = $binary
        binary_sha256 = $binHashBefore
        git_head = $headBefore
        git_status = @(& git status --short -- . ':!docs/measurements/A1.1/windows*')
        source_hashes = @(
            Get-FileHash -Algorithm SHA256 `
                'crates/omnia-bench/Cargo.toml',
                'crates/omnia-bench/src/generate.rs',
                'crates/omnia-bench/benches/fork.rs',
                'crates/omnia-doc/src/doc.rs',
                'docs/measurements/A1.1/run.ps1',
                'docs/measurements/A1.1/summarize.py' |
                Select-Object Path, Hash
        )
    } | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $rawDir 'machine.json')

    $orderLines = @()
    $env:CRITERION_HOME = $critDir
    for ($pass = 1; $pass -le 10; $pass++) {
        $tag = ('{0:D2}-fork_10k' -f $pass)
        Save-Conditions (Join-Path $rawDir "$tag.before.json")
        $orderLines += "$(Get-Date -Format o) $tag"
        $env:OMNIA_FORK_CSV = Join-Path $rawDir "$tag.csv"
        $prevEap = $ErrorActionPreference
        $ErrorActionPreference = 'Continue'
        & $binary --bench --exact fork_10k --noplot > (Join-Path $rawDir "$tag.stdout.txt") 2> (Join-Path $rawDir "$tag.stderr.txt")
        $code = $LASTEXITCODE
        $ErrorActionPreference = $prevEap
        Save-Conditions (Join-Path $rawDir "$tag.after.json")
        if ($code -ne 0) { throw "Invocation failed ($code): $tag" }
    }
    Remove-Item Env:OMNIA_FORK_CSV -ErrorAction SilentlyContinue
    Remove-Item Env:CRITERION_HOME -ErrorAction SilentlyContinue

    $orderLines | Set-Content -LiteralPath (Join-Path $rawDir 'order.txt')
    if ($binHashBefore -ne (Get-FileHash -LiteralPath $binary -Algorithm SHA256).Hash) { throw 'Binary SHA-256 changed during campaign.' }
    if ($headBefore -ne (& git rev-parse HEAD).Trim()) { throw 'Git HEAD changed during campaign.' }

    $archivePath = Join-Path $OutputDirectory 'raw.tar.gz'
    & tar -czf $archivePath -C $OutputDirectory raw
    if ($LASTEXITCODE -ne 0) { throw 'tar archive failed.' }
    & python (Join-Path $PSScriptRoot 'summarize.py') $rawDir | Set-Content -LiteralPath (Join-Path $OutputDirectory 'summary.txt')
    Remove-Item -Recurse -Force $rawDir, $critDir
} finally {
    Pop-Location
}
