param([string]$Bench = "./target/release/omnia-bench.exe", [int]$Repeats=3, [string]$Size="3072x1728", [string]$Results="docs/measurements/T0.4")
$ErrorActionPreference = "Continue"
$PSNativeCommandUseErrorActionPreference = $false
$dir = $Results
New-Item -ItemType Directory -Force $dir | Out-Null
$cases = @()
foreach ($n in 5000,11300,20000,60000,100000) {$cases += ,@("s1",$n,"fixed")}
foreach ($n in 25000,70000) {$cases += ,@("s1",$n,"quads-only")}
foreach ($n in 1000,5000) {foreach ($m in "square","rounded","sprite") {$cases += ,@("s2",$n,$m)}}
foreach ($n in 1000,5000,15000) {foreach ($m in "fixed","sweep") {$cases += ,@("s3",$n,$m)}}
foreach ($n in 100,500,1500,5000) {$cases += ,@("s4",$n,"fixed")}
foreach ($n in 2000,6000,12000) {foreach ($m in "fixed","sweep","direct","direct-sweep") {$cases += ,@("s5",$n,$m)}}
foreach ($n in 20000,60000) {foreach ($m in "cached","repaint") {$cases += ,@("s6",$n,$m)}}
for ($r=1; $r -le $Repeats; $r++) {
    if (!(Test-Path "$dir/baseline-$r.json")) {
    & $Bench run --sheet t01 --frames 600 --size $Size --json "$dir/baseline-$r.json" > "$dir/baseline-$r.stdout.log" 2>&1
    if ($LASTEXITCODE -ne 0) {throw "baseline failed"}
    }
    foreach ($case in $cases) {
        $s,$n,$m = $case
        $key = "$s-$n-$m-$r"
        if (Test-Path "$dir/$key.json") {continue}
        Write-Output "Running $key"
        & $Bench spike $s --count $n --mode $m --frames 600 --size $Size --json "$dir/$key.json" > "$dir/$key.stdout.log" 2>&1
        if ($LASTEXITCODE -ne 0) {throw "$key failed: $LASTEXITCODE"}
    }
}
& $Bench spike s3 --mode quality --json "$dir/s3-quality.json" > "$dir/s3-quality.stdout.log" 2>&1
if ($LASTEXITCODE -ne 0) {throw "quality failed"}
& $Bench spike s8 --size $Size --json "$dir/s8.json" > "$dir/s8.stdout.log" 2>&1
if ($LASTEXITCODE -ne 0) {throw "S8 failed"}
