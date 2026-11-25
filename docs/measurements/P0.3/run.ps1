# P0.3 / PS1 one-command run on a Windows development machine. Certifies nothing (AGENTS rule 3).
#
#   pwsh -File docs/measurements/P0.3/run.ps1 [-Label windows]
#
# Builds the spike (release, offline, versions from spike/Cargo.lock), snapshots conditions,
# runs the 20-deck extractor and the hostile set, then the PB3 index timing with the schedule
# fixed below, snapshots conditions again and records the binaries' SHA-256. Honours an
# existing CARGO_TARGET_DIR; otherwise builds into spike/target (git-ignored).
param([string]$Label = "windows")
$ErrorActionPreference = "Stop"

# Schedule, fixed before the first timed run.
$Parts = 500; $Reps = 10; $Iters = 200; $Warmup = 20

$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$results = Join-Path $here "results"
New-Item -ItemType Directory -Force $results | Out-Null
if (-not $env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR = Join-Path $here "spike/target" }
$bin = Join-Path $env:CARGO_TARGET_DIR "release"

Push-Location (Join-Path $here "spike")
try {
    cargo build --offline --locked --release --bins
    if ($LASTEXITCODE -ne 0) { throw "build failed" }
} finally { Pop-Location }

$hashes = Get-FileHash -Algorithm SHA256 (Join-Path $bin "extract.exe"), (Join-Path $bin "index_bench.exe") |
    ForEach-Object { "{0}  {1}" -f $_.Hash.ToLower(), (Split-Path -Leaf $_.Path) }
$hashes | Set-Content (Join-Path $results "binaries-$Label.sha256")

& (Join-Path $here "conditions.ps1") | Set-Content (Join-Path $results "conditions-before-$Label.txt")

& (Join-Path $bin "extract.exe") $here
if ($LASTEXITCODE -ne 0) { throw "extraction expectations failed" }

& (Join-Path $bin "index_bench.exe") --work (Join-Path $env:CARGO_TARGET_DIR "p03-work") `
    --out (Join-Path $results "index-$Label") --parts $Parts --reps $Reps --iters $Iters --warmup $Warmup |
    Tee-Object (Join-Path $results "index-$Label.txt")
if ($LASTEXITCODE -ne 0) { throw "index bench failed" }

& (Join-Path $here "conditions.ps1") | Set-Content (Join-Path $results "conditions-after-$Label.txt")
