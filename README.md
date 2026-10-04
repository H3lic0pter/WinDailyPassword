# WinPasswordLock

[中文文档](README.zh-CN.md)

WinPasswordLock rotates actual Windows local-account passwords using the computer's local date, entirely offline. Only explicitly configured accounts are managed. A separate local administrator keeps a fixed strong password for manual recovery.

All architecture and API comments are in English. Run `cargo doc --no-deps --open` for public documentation, or `cargo doc --no-deps --document-private-items --open` to include internal functions.

The rule is `password_for(account, date)` in [`src/password_rule.rs`](src/password_rule.rs). Its temporary implementation returns `MMDD`: October 4 becomes `1004`. This is predictable, repeats annually, and is unsuitable for real accounts. Multiple managed accounts may share the same daily password.

Replace it with a private deterministic rule before deployment. The same account and date must always produce the same password; different dates for that account should produce different passwords. Passwords may be identical across accounts. Keep secrets out of chat, logs, and version control. Account renaming requires updating the rule and state mapping. Changing the rule for previously applied dates can prevent rotation.

## Inspect and create accounts

Run in your own PowerShell window:

```powershell
whoami
Get-LocalUser | Select-Object Name, Enabled
```

`whoami` shows the process identity as `computer\account`. Codex may run under its sandbox account, which does not identify your desktop login. `Get-LocalUser` includes disabled accounts.

`Administrator`, `DefaultAccount`, `Guest`, and `WDAGUtilityAccount` are built-in Windows accounts. `CodexSandboxOffline` and `CodexSandboxOnline` belong to the Codex execution environment. Only configure personal accounts you intend to rotate; exclude system and sandbox accounts.

Open PowerShell as administrator. Choose an unused account name:

```powershell
$recoveryPassword = Read-Host 'Enter a fixed strong recovery password' -AsSecureString
New-LocalUser -Name 'RescueAdmin' -Password $recoveryPassword -PasswordNeverExpires
$adminGroup = Get-LocalGroup -SID 'S-1-5-32-544'
Add-LocalGroupMember -Group $adminGroup -Member 'RescueAdmin'
```

The SID identifies the Administrators group regardless of Windows display language. To create an ordinary local account, use `New-LocalUser` with its own password and omit adding it to the administrator group. **Sign in to the recovery account once to verify it works.** Keep its password available offline. See [New-LocalUser](https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.localaccounts/new-localuser?view=powershell-5.1).

## Before use

1. Create a separate local administrator recovery account with a fixed strong password. **Sign in once to verify it works.** Never include it in an `account=` line.
2. Create your `config.local.txt` using `config.example.txt` as a reference. Specify the recovery account and the local accounts to rotate. Keep `state_dir` at `C:\ProgramData\WinPasswordLock\state`. Exclude system accounts, sandbox accounts, and accounts that do not need rotation.
3. Replace the temporary `password_rule` before deployment. The computer must calculate both the previous date's password and today's password without anyone signed in, so the rule must be deterministic and fully offline. Do not print passwords.
4. After editing the rule in `password_rule.rs`, build in elevated PowerShell with `cargo build --release`. First run `cargo test` to check date and state handling.

Replace the example account name with your actual account name; omit the angle brackets. Your `config.local.txt` should resemble:

```ini
recovery_account=RescueAdmin
state_dir=C:\ProgramData\WinPasswordLock\state
account=<YourLocalAccount>
```

Repeat `account=` for each managed account.

## Enroll each account

From the project directory, run in elevated interactive PowerShell. Adjust paths and account names as needed:

```powershell
$app = '.\target\release\win-password-lock.exe'
$config = '.\config.local.txt'
& $app status --config $config
& $app enroll --config $config YourLocalAccount
& $app check --config $config
```

`enroll` hides input of the existing password and immediately changes the real account password to today's derived value through `NetUserChangePassword`. Enroll each account separately. `check` does not change passwords; all configured accounts must be enrolled for it to pass. Windows password policy may reject a derived password; errors include the Windows status code.

## Install scheduled rotation

Verify today's password and the recovery account both work. From the project directory, run in elevated PowerShell:

```powershell
.\scripts\Install-Task.ps1 -Executable ".\target\release\win-password-lock.exe" -Config ".\config.local.txt"
```

The script verifies recovery-administrator membership, copies the executable and configuration into the protected `C:\ProgramData\WinPasswordLock` directory, checks state, and registers `WinPasswordLock-Rotate` under `SYSTEM` at startup and daily at 00:00. Installation itself does not change passwords. After editing the rule, rebuild and rerun installation to update the installed executable. Remove the task with `scripts/Uninstall-Task.ps1`; this does not restore an old password.

The installed executable supports `status`, `check`, and `rotate`. **`rotate` changes real passwords.**

## Recover through the backup account

The program does not automatically reset passwords through the recovery account. It preserves that account as a Windows administrator login for manual recovery.

1. Sign in to `RescueAdmin` and open elevated PowerShell.
2. If the task is installed, disable future runs and stop its running instance:

   ```powershell
   Disable-ScheduledTask -TaskName 'WinPasswordLock-Rotate'
   Stop-ScheduledTask -TaskName 'WinPasswordLock-Rotate'
   ```

3. If the managed account's password is known, sign in to that account and use Windows **Change password**. If it is lost, the recovery administrator can reset it:

   ```powershell
   $newPassword = Read-Host 'Enter a new password for the managed account' -AsSecureString
   Set-LocalUser -Name 'YourLocalAccount' -Password $newPassword
   ```

4. Verify the managed account accepts the new password. Keep rotation disabled until its state is reconciled.

An administrator reset may make previously DPAPI-protected data unrecoverable. Prefer changing the password with the old password when available. See [Set-LocalUser](https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.localaccounts/set-localuser?view=powershell-5.1) and [DPAPI limitations](https://learn.microsoft.com/en-us/windows/win32/seccrypto/example-c-program-using-cryptprotectdata).

To resume after an external password change, back up and move only that account's state file outside `state_dir`, then run `enroll` with the new password. State filenames are `account-<hex of lowercase account name UTF-8 bytes>.state`; see [`state::path_for`](src/state.rs). Enrollment requires the original state file to be absent and immediately applies today's password. Run `check`, verify sign-in, then enable the task with `Enable-ScheduledTask -TaskName 'WinPasswordLock-Rotate'`. Preserve other accounts' state; do not guess passwords by editing dates.

## Uninstall

Removing the task stops future scheduled rotation. **It does not restore an old password.**

1. In elevated PowerShell, disable and stop the installed task using the recovery commands above. From the project directory, unregister it:

   ```powershell
   .\scripts\Uninstall-Task.ps1
   ```

   The script only removes the task. Passwords, installed files, and state remain.

2. Keep the rule and state available. Inspect installed state:

   ```powershell
   & 'C:\ProgramData\WinPasswordLock\win-password-lock.exe' status --config 'C:\ProgramData\WinPasswordLock\config.txt'
   ```

   `current` is the last committed password date. Once rotation stops, passwords do not follow today's date. If `pending` exists, an interrupted change may have applied either the `current` or `pending` password; `current` alone is not authoritative.

3. Sign in to each managed account with its existing password and use Windows **Change password** to set a fixed strong password. If the password is lost, use the recovery procedure above.
4. Verify each new password works, then delete `C:\ProgramData\WinPasswordLock` if no longer needed. The project directory may also be deleted. Keep the recovery account until recovery is no longer needed; uninstall does not remove it.

If you enrolled accounts without installing a task, start at step 3. Merely building or viewing documentation does not change passwords.

## Behavior and limits

- Uses the computer's local date. After downtime, startup rotation advances directly from the recorded date to today. Clock rollback stops rotation.
- Each account has independent state. A pending date is saved before changing the password to support recovery after interruption.
- Startup and midnight tasks may run late. The old password remains valid until a change succeeds; there is no strict midnight expiration guarantee.
- Changes local-account passwords, not Windows Hello PINs, BitLocker passwords, or Microsoft-account passwords.
- An inferred mental date rule may expose future passwords. Rotation rejects an old password only when the new value differs.

The program uses the old password to perform a [NetUserChangePassword](https://learn.microsoft.com/en-us/windows/win32/api/lmaccess/nf-lmaccess-netuserchangepassword) change. Effects of frequent rotation on DPAPI/EFS and all application credentials have not been fully verified. Test encrypted files and saved credentials on a disposable account and back up important data before deployment.
