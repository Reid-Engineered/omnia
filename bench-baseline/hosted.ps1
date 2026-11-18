# T2.6 deterministic gate and baseline capture. Run from the repository root.
param(
    [string]$Bench = './target/debug/omnia-bench.exe',
    [string]$OutputDirectory = 'target/bench-reports',
    [switch]$Capture
)

$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $false
New-Item -ItemType Directory -Path $OutputDirectory -Force | Out-Null

$failed = $false
foreach ($sheet in '1k', '10k') {
    foreach ($script in 'pan', 'zoom') {
        $case = "$sheet-$script"
        $report = Join-Path $OutputDirectory "$case.json"
        & $Bench run --headless --sheet $sheet --script $script --size 1280x800 `
            --warmup 600 --frames 600 --json $report
        if ($LASTEXITCODE -ne 0) { throw "bench run failed for $case ($LASTEXITCODE)" }
        if (!$Capture) {
            $baseline = Join-Path $PSScriptRoot "$case.json"
            $comparison = Join-Path $OutputDirectory "$case-compare.json"
            & $Bench compare $baseline $report --max-regression 10 `
                --metrics 'allocs_per_frame.ours,primitives' --json $comparison
            if ($LASTEXITCODE -ne 0) { $failed = $true }
        }
    }
}
if ($failed) { exit 1 }
