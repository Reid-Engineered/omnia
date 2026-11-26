# S0.3 / SS1 one-command run on a Windows development machine. Certifies nothing (AGENTS rule 3).
#
#   pwsh -File docs/measurements/S0.3/run.ps1 -NgspiceLib <path-to-libngspice-0.dll> [-Label windows]
#
# Builds the standalone spike crate (release, locked), snapshots conditions before/after,
# audits DLL imports with objdump/ldd, records SHA-256 hashes of the spike binaries and DLL,
# and runs the 10-repetition rotating SS1 benchmark schedule.
#
# Every run writes to a fresh results/runs/<label>-<timestamp>/ claimed atomically via an
# exclusive `.claim` file (FileMode.CreateNew) and never overwrites committed campaign files.
param(
    [Parameter(Mandatory = $false)]
    [string]$NgspiceLib = "",
    [string]$Label = "windows",
    [int]$Reps = 10,
    [int]$StartupIters = 10,
    [int]$StreamIters = 3,
    [int]$CancelIters = 5,
    [int]$BatchSize = 256
)
$ErrorActionPreference = "Stop"

$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$utf8NoBom = New-Object System.Text.UTF8Encoding $false

function Write-Utf8Lf([string]$Path, [string[]]$Lines) {
    $text = (($Lines -join "`n").TrimEnd("`r", "`n")) + "`n"
    [System.IO.File]::WriteAllText($Path, $text, $utf8NoBom)
}

function New-RunDirectory([string]$Path) {
    [System.IO.Directory]::CreateDirectory($Path) | Out-Null
    $claim = [System.IO.File]::Open((Join-Path $Path ".claim"), [System.IO.FileMode]::CreateNew)
    $claim.Dispose()
}

if ($env:S03_CLAIM_SELFTEST) {
    $probe = Join-Path ([System.IO.Path]::GetTempPath()) ("s03-claim-" + [guid]::NewGuid())
    New-RunDirectory $probe
    try {
        New-RunDirectory $probe
        "claim self-test: FAIL (second claim succeeded)"
        exit 1
    } catch {
        "claim self-test: PASS (second claim refused)"
    } finally {
        Remove-Item -Recurse -Force $probe
    }
    exit 0
}

if (-not $NgspiceLib) {
    if ($env:NGSPICE_LIB) {
        $NgspiceLib = $env:NGSPICE_LIB
    } else {
        throw "Provide -NgspiceLib <path-to-libngspice-0.dll> or set NGSPICE_LIB."
    }
}
$NgspiceLib = [System.IO.Path]::GetFullPath($NgspiceLib)
if (-not (Test-Path $NgspiceLib)) {
    throw "ngspice shared library not found: $NgspiceLib"
}

$results = [System.IO.Path]::GetFullPath(
    (Join-Path $here ("results/runs/{0}-{1}" -f $Label, (Get-Date -Format "yyyyMMddTHHmmss"))))
New-RunDirectory $results

if (-not $env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR = Join-Path $here "spike/target" }
$bin = Join-Path $env:CARGO_TARGET_DIR "release"

Push-Location (Join-Path $here "spike")
try {
    cargo build --locked --release --bins
    if ($LASTEXITCODE -ne 0) { throw "spike cargo build failed" }
} finally { Pop-Location }

$workerBin = Join-Path $bin "omnia-ngspice-worker.exe"
$benchBin = Join-Path $bin "bench_ss1.exe"

$hashes = Get-FileHash -Algorithm SHA256 $workerBin, $benchBin, $NgspiceLib |
    ForEach-Object { "{0}  {1}" -f $_.Hash.ToLower(), (Split-Path -Leaf $_.Path) }
Write-Utf8Lf (Join-Path $results "binaries-$Label.sha256") $hashes

if (Test-Path "C:\msys64\ucrt64\bin") {
    $env:PATH = "C:\msys64\ucrt64\bin;$env:PATH"
}

# Log only basename and byte size (no host-specific temp directory paths).
$libItem = Get-Item $NgspiceLib
$auditLines = @("=== library ===", ("{0} bytes  {1}" -f $libItem.Length, $libItem.Name))
if (Test-Path "C:\msys64\ucrt64\bin\objdump.exe") {
    $auditLines += "=== objdump -p (DLL Name) ==="
    $auditLines += (& "C:\msys64\ucrt64\bin\objdump.exe" -p $NgspiceLib | Select-String "DLL Name:" | ForEach-Object { $_.Line.Trim() })
}
if (Test-Path "C:\msys64\usr\bin\ldd.exe") {
    $auditLines += "=== ldd ==="
    $auditLines += (& "C:\msys64\usr\bin\ldd.exe" $NgspiceLib | ForEach-Object { $_.Trim() })
}
Write-Utf8Lf (Join-Path $results "dll-audit-$Label.txt") $auditLines

Write-Utf8Lf (Join-Path $results "conditions-before-$Label.txt") (& (Join-Path $here "conditions.ps1"))

$benchOut = & $benchBin `
    --lib $NgspiceLib `
    --worker $workerBin `
    --out (Join-Path $results "ss1-$Label") `
    --reps $Reps `
    --startup-iters $StartupIters `
    --stream-iters $StreamIters `
    --cancel-iters $CancelIters `
    --batch-size $BatchSize
if ($LASTEXITCODE -ne 0) { throw "bench_ss1 failed" }
$benchOut | ForEach-Object { Write-Host $_ }
Write-Utf8Lf (Join-Path $results "ss1-$Label.txt") $benchOut

Write-Utf8Lf (Join-Path $results "conditions-after-$Label.txt") (& (Join-Path $here "conditions.ps1"))
Write-Host "Results written to $results"
