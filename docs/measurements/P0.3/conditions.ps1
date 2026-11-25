# Prints a condition snapshot for a P0.3 run on Windows (hardware, power, load).
$ErrorActionPreference = "Continue"
"timestamp: " + (Get-Date -Format o)
$os = Get-CimInstance Win32_OperatingSystem
"os: {0} {1} (build {2}), {3}" -f $os.Caption, $os.Version, $os.BuildNumber, $os.OSArchitecture
"memory_total_gb: {0:N1}" -f ($os.TotalVisibleMemorySize / 1MB)
"memory_free_gb: {0:N1}" -f ($os.FreePhysicalMemory / 1MB)
foreach ($c in Get-CimInstance Win32_Processor) {
    "cpu: {0} ({1} cores / {2} logical, max {3} MHz, current {4} MHz)" -f $c.Name.Trim(), $c.NumberOfCores, $c.NumberOfLogicalProcessors, $c.MaxClockSpeed, $c.CurrentClockSpeed
}
foreach ($g in Get-CimInstance Win32_VideoController) {
    "gpu: {0} (driver {1}), {2}x{3} @ {4} Hz" -f $g.Name, $g.DriverVersion, $g.CurrentHorizontalResolution, $g.CurrentVerticalResolution, $g.CurrentRefreshRate
}
$b = Get-CimInstance Win32_Battery
if ($b) {
    $src = if ($b.BatteryStatus -eq 2) { "AC" } else { "battery" }
    "power_source: {0} (BatteryStatus={1}, charge={2}%)" -f $src, $b.BatteryStatus, $b.EstimatedChargeRemaining
} else { "power_source: AC (no battery)" }
"power_scheme: " + ((powercfg /getactivescheme) -join " ").Trim()
"governor: n/a on Windows (power scheme above)"
$procs = Get-Process
"process_count: " + $procs.Count
# Process names are recorded as the first 8 hex digits of SHA-256(name), so the snapshot
# names no other company's products (AGENTS rule 1) but before/after still compare.
"top_cpu_processes (name = first 8 hex digits of SHA-256(name)):"
$sha = [System.Security.Cryptography.SHA256]::Create()
$procs | Sort-Object CPU -Descending | Select-Object -First 8 | ForEach-Object {
    $hash = -join ($sha.ComputeHash([System.Text.Encoding]::UTF8.GetBytes($_.ProcessName))[0..3] | ForEach-Object { $_.ToString("x2") })
    "  proc-{0} cpu_s={1:N1} ws_mb={2:N0}" -f $hash, $_.CPU, ($_.WorkingSet64 / 1MB)
}
$d = Get-PSDrive -Name C
"disk_c_free_gb: {0:N1}" -f ($d.Free / 1GB)
"rustc: " + (rustc --version)
