# Installs a scheduled task only after account and rule preflight succeeds.
# This script never calls the password-changing rotate command.
param(
    [Parameter(Mandatory = $true)][string]$Executable,
    [Parameter(Mandatory = $true)][string]$Config
)

$ErrorActionPreference = 'Stop'
$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
$principal = [Security.Principal.WindowsPrincipal]::new($identity)
if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    throw 'Run this script from an elevated PowerShell window.'
}

$executablePath = (Resolve-Path -LiteralPath $Executable).Path
$configPath = (Resolve-Path -LiteralPath $Config).Path
$lines = Get-Content -LiteralPath $configPath -Encoding UTF8
$recoveryLine = @($lines | Where-Object { $_ -match '^\s*recovery_account\s*=' })
$stateLine = @($lines | Where-Object { $_ -match '^\s*state_dir\s*=' })
if ($recoveryLine.Count -ne 1 -or $stateLine.Count -ne 1) {
    throw 'Config must contain one recovery_account and one state_dir.'
}
$recovery = ($recoveryLine[0] -split '=', 2)[1].Trim()
$stateDir = ($stateLine[0] -split '=', 2)[1].Trim()
$installDir = Join-Path $env:ProgramData 'WinPasswordLock'
$requiredStateDir = Join-Path $installDir 'state'
if ([IO.Path]::GetFullPath($stateDir).TrimEnd('\') -ine [IO.Path]::GetFullPath($requiredStateDir).TrimEnd('\')) {
    throw "state_dir must be $requiredStateDir"
}

$recoveryUser = Get-LocalUser -Name $recovery -ErrorAction Stop
if (-not $recoveryUser.Enabled) {
    throw 'Recovery account is disabled.'
}
$administratorsSid = [Security.Principal.SecurityIdentifier]::new('S-1-5-32-544')
$administrators = @(Get-LocalGroupMember -SID $administratorsSid -ErrorAction Stop)
if (-not ($administrators | Where-Object { $_.SID.Value -eq $recoveryUser.SID.Value })) {
    throw 'Recovery account must belong to the local Administrators group.'
}

New-Item -ItemType Directory -Path $installDir -Force | Out-Null
New-Item -ItemType Directory -Path $requiredStateDir -Force | Out-Null
$installedExe = Join-Path $installDir 'win-password-lock.exe'
$installedConfig = Join-Path $installDir 'config.txt'
Copy-Item -LiteralPath $executablePath -Destination $installedExe -Force
Copy-Item -LiteralPath $configPath -Destination $installedConfig -Force

# Restricts installed files to SYSTEM and local Administrators.
function Protect-Path([string]$Path) {
    $item = Get-Item -LiteralPath $Path -Force
    $acl = Get-Acl -LiteralPath $Path
    $acl.SetAccessRuleProtection($true, $false)
    foreach ($rule in @($acl.Access)) {
        $acl.PurgeAccessRules($rule.IdentityReference)
    }
    $acl.SetOwner($administratorsSid)
    $inheritance = if ($item.PSIsContainer) {
        [Security.AccessControl.InheritanceFlags]'ContainerInherit, ObjectInherit'
    } else {
        [Security.AccessControl.InheritanceFlags]::None
    }
    foreach ($sidValue in @('S-1-5-18', 'S-1-5-32-544')) {
        $sid = [Security.Principal.SecurityIdentifier]::new($sidValue)
        $rule = [Security.AccessControl.FileSystemAccessRule]::new(
            $sid,
            [Security.AccessControl.FileSystemRights]::FullControl,
            $inheritance,
            [Security.AccessControl.PropagationFlags]::None,
            [Security.AccessControl.AccessControlType]::Allow
        )
        $acl.AddAccessRule($rule)
    }
    Set-Acl -LiteralPath $Path -AclObject $acl
}
Get-ChildItem -LiteralPath $installDir -Recurse -Force |
    Sort-Object { $_.FullName.Length } -Descending |
    ForEach-Object { Protect-Path $_.FullName }
Protect-Path $installDir

& $installedExe check --config $installedConfig
if ($LASTEXITCODE -ne 0) {
    throw 'Preflight failed. Task was not registered.'
}

$action = New-ScheduledTaskAction -Execute $installedExe -Argument ('rotate --config "{0}"' -f $installedConfig)
$triggers = @(
    New-ScheduledTaskTrigger -AtStartup
    New-ScheduledTaskTrigger -Daily -At '00:00'
)
$taskPrincipal = New-ScheduledTaskPrincipal -UserId 'SYSTEM' -LogonType ServiceAccount -RunLevel Highest
$settings = New-ScheduledTaskSettingsSet -StartWhenAvailable -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -MultipleInstances IgnoreNew -ExecutionTimeLimit (New-TimeSpan -Minutes 10)
Register-ScheduledTask -TaskName 'WinPasswordLock-Rotate' -Action $action -Trigger $triggers -Principal $taskPrincipal -Settings $settings -Force | Out-Null
Write-Host 'Registered WinPasswordLock-Rotate. No password was changed by this script.'
