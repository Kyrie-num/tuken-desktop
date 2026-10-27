# Embedded in the signed/build-distributed helper; never loaded from a writable
# script path. Rust prefixes only the fixed Boolean $prepare assignment.
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$WarningPreference = 'SilentlyContinue'
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)
$modules = Join-Path $env:TUKEN_SYSTEM_DIRECTORY 'WindowsPowerShell\v1.0\Modules'
Import-Module (Join-Path $modules 'CimCmdlets\CimCmdlets.psd1') -ErrorAction Stop
Import-Module (Join-Path $modules 'Microsoft.PowerShell.Management\Microsoft.PowerShell.Management.psd1') -ErrorAction Stop
Import-Module (Join-Path $modules 'Microsoft.PowerShell.Utility\Microsoft.PowerShell.Utility.psd1') -ErrorAction Stop

function Read-Platform {
    $feature = Get-CimInstance Win32_OptionalFeature -Filter "Name='VirtualMachinePlatform'"
    $computer = Get-CimInstance Win32_ComputerSystem
    $processors = @(Get-CimInstance Win32_Processor)
    $os = Get-CimInstance Win32_OperatingSystem
    $services = @(Get-Service -Name vmcompute,hns,vfpext -ErrorAction SilentlyContinue)
    [ordered]@{
        feature = if ($null -eq $feature) { 0 } else { [int]$feature.InstallState }
        firmware = @($processors | Where-Object { $_.VirtualizationFirmwareEnabled }).Count -gt 0
        hypervisor = [bool]$computer.HypervisorPresent
        services = $services.Count -eq 3
        virtual_machine = ($computer.Manufacturer + ' ' + $computer.Model) -match 'VirtualBox|VMware|Virtual Machine|KVM|QEMU|Parallels|Xen|Bochs'
        boot_id = $os.LastBootUpTime.ToUniversalTime().ToString('o')
        reboot = $false
        error = $null
    }
}

try {
    $state = Read-Platform
    if ($prepare -and $state.feature -ne 0) {
        Import-Module (Join-Path $modules 'Dism\Dism.psd1') -ErrorAction Stop
        $feature = Get-WindowsOptionalFeature -Online -FeatureName VirtualMachinePlatform
        $pending = [string]$feature.State -in @('EnablePending', 'DisablePending')
        $changed = $false
        if (-not $pending -and [string]$feature.State -ne 'Enabled') {
            $result = Enable-WindowsOptionalFeature -Online -FeatureName VirtualMachinePlatform -All -NoRestart
            $pending = [bool]$result.RestartNeeded
            $changed = $true
        }
        # A manually disabled hypervisor prevents VMP from working after reboot.
        # Missing boot setting means the Windows default; change only explicit Off.
        $bcd = Join-Path $env:TUKEN_SYSTEM_DIRECTORY 'bcdedit.exe'
        $boot = (& $bcd /enum '{current}' 2>&1 | Out-String)
        if ($LASTEXITCODE -eq 0 -and $boot -match '(?im)^\s*hypervisorlaunchtype\s+Off\s*$') {
            & $bcd /set '{current}' hypervisorlaunchtype auto | Out-Null
            if ($LASTEXITCODE -ne 0) { throw 'Windows refused to enable the hypervisor at boot.' }
            $changed = $true
        }
        $state = Read-Platform
        # An enabled feature can still be waiting for its first boot, including
        # when it was enabled outside Tuken. Don't offer the same repair again.
        $awaitingBoot = $state.feature -eq 1 -and $state.firmware -and (-not $state.hypervisor -or -not $state.services)
        $state.reboot = $pending -or $changed -or $awaitingBoot
    }
    $state | ConvertTo-Json -Compress
} catch {
    if ($null -eq $state) {
        $state = [ordered]@{ feature = 0; firmware = $false; hypervisor = $false; services = $false; virtual_machine = $false; boot_id = ''; reboot = $false; error = $null }
    }
    if ($prepare -and ($pending -or $changed)) { $state.reboot = $true }
    $detail = $_.Exception.Message
    $state.error = $detail.Substring(0, [Math]::Min(600, $detail.Length))
    $state | ConvertTo-Json -Compress
}
