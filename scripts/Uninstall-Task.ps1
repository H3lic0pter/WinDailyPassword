# Removes the scheduled task without changing account passwords or state files.
$ErrorActionPreference = 'Stop'
$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
$principal = [Security.Principal.WindowsPrincipal]::new($identity)
if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    throw 'Run this script from an elevated PowerShell window.'
}
Unregister-ScheduledTask -TaskName 'WinPasswordLock-Rotate' -Confirm:$false -ErrorAction Stop
Write-Host 'Task removed. Account passwords and installed files were not changed.'
