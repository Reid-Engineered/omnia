# Reference-protocol re-run (plan section 2.1): interleaved repetitions, power state recorded.
# Usage: pwsh -NoProfile -File crates/omnia-bench/src/spike/measure-ref.ps1 [-Repeats 10] [-Scenes s1]
param(
    [string]$Bench = "./target/release/omnia-bench.exe",
    [int]$Repeats = 10,
    [string]$Size = "3072x1728",
    [string]$Results = "docs/measurements/T0.4-ref",
    [string[]]$Scenes = @("s1")
)
$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $false
New-Item -ItemType Directory -Force $Results | Out-Null
Add-Type -AssemblyName System.Windows.Forms

function Get-PowerState {
    $p = [System.Windows.Forms.SystemInformation]::PowerStatus
    $scheme = (powercfg /getactivescheme) -replace '^.*\((.*)\)\s*$', '$1'
    $refresh = (Get-CimInstance Win32_VideoController | ForEach-Object { "$($_.Name)=$($_.CurrentRefreshRate)Hz" }) -join "; "
    [ordered]@{
        time = (Get-Date).ToString("o")
        ac = "$($p.PowerLineStatus)"
        battery_percent = [math]::Round($p.BatteryLifePercent * 100)
        power_scheme = $scheme.Trim()
        refresh = $refresh
    }
}

$all = @()
$all += , @("baseline", 0, "")
foreach ($n in 5000, 11300, 20000, 60000, 100000) { $all += , @("s1", $n, "fixed") }
foreach ($n in 25000, 70000) { $all += , @("s1", $n, "quads-only") }
foreach ($n in 1000, 5000, 15000) { $all += , @("s3", $n, "fixed") }
foreach ($n in 100, 500, 1500, 5000) { $all += , @("s4", $n, "fixed") }
foreach ($n in 2000, 6000, 12000) { foreach ($m in "fixed", "direct") { $all += , @("s5", $n, $m) } }
foreach ($n in 20000, 60000) { foreach ($m in "cached", "repaint") { $all += , @("s6", $n, $m) } }
$cases = @($all | Where-Object { $_[0] -eq "baseline" -or $Scenes -contains $_[0] })

$log = Join-Path $Results "runs.jsonl"
for ($r = 1; $r -le $Repeats; $r++) {
    # Rotate the case order each pass so no case always runs first or last.
    $k = ($r - 1) % $cases.Count
    $order = @($cases[$k..($cases.Count - 1)]) + @(if ($k -gt 0) { $cases[0..($k - 1)] })
    foreach ($case in $order) {
        $s, $n, $m = $case
        $key = if ($s -eq "baseline") { "baseline-$r" } else { "$s-$n-$m-$r" }
        $json = Join-Path $Results "$key.json"
        if (Test-Path $json) { continue }
        $before = Get-PowerState
        Write-Output "[$r/$Repeats] $key"
        if ($s -eq "baseline") {
            & $Bench run --sheet empty --frames 600 --size $Size --json $json > "$json.log" 2>&1
        } else {
            & $Bench spike $s --count $n --mode $m --frames 600 --size $Size --json $json > "$json.log" 2>&1
        }
        $code = $LASTEXITCODE
        $entry = [ordered]@{ key = $key; pass = $r; exit = $code; before = $before; after = (Get-PowerState) }
        ($entry | ConvertTo-Json -Compress -Depth 4) | Add-Content -Encoding utf8 $log
        if ($code -ne 0) { throw "$key failed: $code" }
    }
}
