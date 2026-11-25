$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Windows.Forms

$machine = Get-CimInstance Win32_ComputerSystem
$os = Get-CimInstance Win32_OperatingSystem
$cpu = Get-CimInstance Win32_Processor
$power = [System.Windows.Forms.SystemInformation]::PowerStatus
$primary = [System.Windows.Forms.Screen]::PrimaryScreen
$windowsVersion = Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion'

[ordered]@{
    utc = [DateTime]::UtcNow.ToString('o')
    model = $machine.Model
    ram_bytes = $machine.TotalPhysicalMemory
    os = $os | Select-Object Caption, Version, BuildNumber, OSArchitecture
    windows_revision = $windowsVersion.UBR
    windows_release = $windowsVersion.DisplayVersion
    cpu = $cpu | Select-Object Name, NumberOfCores, NumberOfLogicalProcessors
    adapters = @(Get-CimInstance Win32_VideoController | Select-Object Name, DriverVersion,
        CurrentHorizontalResolution, CurrentVerticalResolution, CurrentRefreshRate)
    primary_display = @{
        width = $primary.Bounds.Width
        height = $primary.Bounds.Height
    }
    power_source = $power.PowerLineStatus.ToString()
    power_profile = (powercfg /GetActiveScheme | Out-String).Trim()
    governor = 'Windows processor power management'
    processes = @(Get-Process | Group-Object ProcessName | ForEach-Object { $_.Name })
} | ConvertTo-Json -Depth 5
