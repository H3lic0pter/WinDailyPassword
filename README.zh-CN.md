# WinPasswordLock

[English documentation](README.md)

本项目为 Windows 本地账户按本机日期轮换**真实账户密码**。仅处理配置中明确列出的账户。一个独立的本地管理员账户保持固定强密码，供故障恢复。

架构与 API 注释均为英文 Rustdoc。运行 `cargo doc --no-deps --open` 查看公开架构；运行 `cargo doc --no-deps --document-private-items --open` 查看内部函数。

核心规则在 [`src/password_rule.rs`](src/password_rule.rs) 的 `password_for(account, date)`。目前仅返回 `MMDD`，如 10 月 4 日返回 `1004`。这是可预测的临时规则，不适合保护真实账户；多账户配置也会因密码重复而被程序拒绝。正式使用前改成私密且按账户区分的规则，不要在聊天、日志或版本库中公开秘密。函数须对同一账户和日期始终返回同一密码；不同账户、不同日期应产生不同密码。账户改名后，先用恢复账户处理状态与规则映射。

## 查看和创建账户

在你自己打开的 PowerShell 中查看当前进程身份和全部本地账户：

```powershell
whoami
Get-LocalUser | Select-Object Name, Enabled
```

`whoami` 显示 `计算机名\账户名`。Codex 执行命令时可能显示沙盒账户，不能据此判断你的桌面登录账户。`Get-LocalUser` 包含禁用账户。

`Administrator`、`DefaultAccount`、`Guest`、`WDAGUtilityAccount` 为 Windows 内置账户；`CodexSandboxOffline`、`CodexSandboxOnline` 属于 Codex 执行环境。只配置你确实需要轮换的个人账户。

以管理员身份打开 PowerShell，创建备用管理员，名称应尚未被占用：

```powershell
$recoveryPassword = Read-Host '输入备用账户的固定强密码' -AsSecureString
New-LocalUser -Name 'RescueAdmin' -Password $recoveryPassword -PasswordNeverExpires
$adminGroup = Get-LocalGroup -SID 'S-1-5-32-544'
Add-LocalGroupMember -Group $adminGroup -Member 'RescueAdmin'
```

SID 可跨 Windows 显示语言定位 Administrators 组。创建普通本地账户时，用它自己的密码执行 `New-LocalUser`，省略加入管理员组的命令。实际登录备用账户一次，确认可用，并离线保存其密码。参见 [Microsoft 创建账户文档](https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.localaccounts/new-localuser?view=powershell-5.1)。

## 使用前

1. 建立独立的本地管理员恢复账户，设固定强密码。**实际登录一次，确认可用**。不要将它写入 `account=` 行。
2. 复制 `config.example.txt`，填入恢复账户和需要轮换的本地账户。`state_dir` 保持为 `C:\ProgramData\WinPasswordLock\state`。不要把系统账户、沙盒账户或不需要轮换的账户加入名单。
3. 正式使用前替换临时的 `password_for`。电脑需要能在无人登录时计算上次日期和当天密码，所以规则必须确定性、完全离线。实现中不要输出密码。
4. 在提升权限的 PowerShell 中构建：`cargo build --release`。先用 `cargo test` 检查日期和状态处理。

配置示例，账户名按实际替换：

```ini
recovery_account=RescueAdmin
state_dir=C:\ProgramData\WinPasswordLock\state
account=YourLocalAccount
```

## 初始化每个账户

在提升权限的交互式 PowerShell 中执行，下面路径和账户名按实际情况替换：

```powershell
$app = 'C:\RIP_D\Codes\CPP\WinPasswordLock\target\release\win-password-lock.exe'
$config = 'C:\RIP_D\Codes\CPP\WinPasswordLock\config.local.txt'
& $app status --config $config
& $app enroll --config $config YourLocalAccount
& $app check --config $config
```

`enroll` 会隐藏输入现有密码，然后通过 Windows 的 `NetUserChangePassword` 把账户密码改为规则生成的当天密码。每个 `account=` 都要分别初始化。`check` 仅检查，不改密码；所有账户完成初始化后才会通过。账户密码策略可能拒绝规则生成的密码，错误会带 Windows 状态码。

## 注册定时任务

确认当天密码可实际登录后，在提升权限的 PowerShell 中执行：

```powershell
.\scripts\Install-Task.ps1 -Executable .\target\release\win-password-lock.exe -Config .\config.local.txt
```

脚本会确认恢复账户属于本地 Administrators 组，将程序和配置复制到受限的 `C:\ProgramData\WinPasswordLock`，检查所有账户状态，然后注册以 `SYSTEM` 身份运行的开机及每日 00:00 任务。安装脚本本身不改密码。规则代码修改后，重新构建并重跑安装脚本，才会更新已安装程序。卸载任务用 `scripts/Uninstall-Task.ps1`；卸载不会把账户密码改回旧值。

可手动执行已安装程序的 `status`、`check`，或 `rotate`。`rotate` 会改真实账户密码，别把它当成检查命令。

## 使用备用账户恢复

程序没有通过备用账户自动重置密码的功能。它保留一个不参与轮换的 Windows 管理员登录入口，供人工恢复。

1. 登录 `RescueAdmin`，以管理员身份打开 PowerShell。
2. 如果任务已安装，先禁用后续运行，再停止正在运行的实例：

   ```powershell
   Disable-ScheduledTask -TaskName 'WinPasswordLock-Rotate'
   Stop-ScheduledTask -TaskName 'WinPasswordLock-Rotate'
   ```

3. 如果还知道被管理账户的密码，登录该账户，在 Windows 中使用**更改密码**。如果密码已丢失，备用管理员可强制重置：

   ```powershell
   $newPassword = Read-Host '输入被管理账户的新密码' -AsSecureString
   Set-LocalUser -Name 'YourLocalAccount' -Password $newPassword
   ```

4. 确认新密码能登录。在处理好状态之前，保持轮换任务禁用。

管理员强制重置可能使之前由 DPAPI 保护的数据无法恢复；知道旧密码时优先正常更改密码。参见 [Set-LocalUser](https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.localaccounts/set-localuser?view=powershell-5.1) 和 [DPAPI 限制](https://learn.microsoft.com/en-us/windows/win32/seccrypto/example-c-program-using-cryptprotectdata)。

在程序之外设置密码后，如需恢复轮换，先备份并将**仅该账户的状态文件**移出 `state_dir`，再执行 `enroll`，输入新设置的密码。状态文件名为 `account-<小写账户名的 UTF-8 字节十六进制>.state`，见 [`state::path_for`](src/state.rs)。初始化要求原状态文件不存在，并会立即应用当天规则密码。执行 `check`、确认能登录后，才用 `Enable-ScheduledTask -TaskName 'WinPasswordLock-Rotate'` 启用任务。保留其他账户状态，不要通过修改日期猜测密码。

## 卸载

移除任务会停止之后的定时轮换，**不会恢复旧密码**。完整停用步骤：

1. 如果任务已安装，在管理员 PowerShell 中先按恢复章节的命令禁用并停止任务。然后在项目目录中注销任务：

   ```powershell
   .\scripts\Uninstall-Task.ps1
   ```

   脚本只移除任务，账户密码、安装文件和状态文件都会保留。

2. 保留规则和状态，查看已安装配置对应的状态：

   ```powershell
   & 'C:\ProgramData\WinPasswordLock\win-password-lock.exe' status --config 'C:\ProgramData\WinPasswordLock\config.txt'
   ```

   `current` 是最后提交的密码日期。停止轮换后，密码不会继续跟随今天的日期。如果存在 `pending`，中断操作可能已应用 `current` 或 `pending` 对应的密码，不能只以 `current` 为准。

3. 用现有密码登录每个被管理账户，在 Windows 中使用**更改密码**，设为固定强密码。现有密码丢失时，按备用账户恢复步骤处理。
4. 确认各账户的新密码都能登录后，再删除不需要的 `C:\ProgramData\WinPasswordLock`。项目目录也可删除。在不再需要恢复能力之前保留备用账户；卸载脚本不会删除它。

如果没有安装任务，但执行过 `enroll`，从第 3 步开始。如果只是构建或查看文档，没有改过账户密码。

## 行为与边界

- 使用 Windows **本机日期**。跨天关机后，开机任务从状态记录的上次日期直接改到当天；时钟倒退时停止，不恢复旧密码。
- 每个账户独立写入状态。改密前先记录待完成日期；中途崩溃后再运行会尝试继续。若账户密码被其他工具改动，可能需要恢复管理员人工处理。
- 定时任务在午夜或启动后运行，存在任务尚未执行的时间窗口。期间旧密码仍可能有效。因此它不能提供严格的“零点即失效”保证。
- 这是本地账户密码，不会改变 Windows Hello PIN、BitLocker 密码或 Microsoft 账户密码。
- 纯心算日期规则若被人从旧密码推知，未来密码也可能暴露。只有新旧密码不同，成功轮换才会拒绝旧密码。

修改过去日期对应的规则可能导致轮换失败；更新程序时要保持对已应用密码的兼容。

Windows 改密接口：[NetUserChangePassword](https://learn.microsoft.com/en-us/windows/win32/api/lmaccess/nf-lmaccess-netuserchangepassword)。管理员直接重置密码可能使 DPAPI 保护的数据不可恢复；本程序使用旧密码执行更改，但尚未验证频繁轮换对 DPAPI/EFS 和应用凭据的全部影响。正式使用前先在一次性本地账户上测试加密文件和已保存凭据，并备份重要数据：[DPAPI 说明](https://learn.microsoft.com/en-us/windows/win32/seccrypto/example-c-program-using-cryptprotectdata)。
