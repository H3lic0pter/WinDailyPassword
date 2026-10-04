//! Offline rotation of real Windows local-account passwords.
//!
//! # Architecture
//!
//! 1. [`config`] selects accounts and the state directory.
//! 2. [`date`] validates the computer's local date.
//! 3. [`password_rule`] derives one password per account and date.
//! 4. [`state`] records committed and pending dates.
//! 5. [`windows`] calls native Windows APIs.
//! 6. [`run_cli`] dispatches the four command-line operations.
//!
//! # Rotation flow
//!
//! `check` validates every configured account and password without modifying
//! Windows. `enroll` changes one known current password and creates its state
//! file. `rotate` records a pending target date, changes the Windows password
//! with the previous password, then commits the new date. A later run first
//! resolves any pending date left by an interrupted change.
//!
//! The scheduled task invokes `rotate` at midnight and startup. Windows may
//! accept the previous password until that task runs. The temporary `MMDD`
//! rule is predictable and repeats every year.

#![deny(missing_docs, rustdoc::broken_intra_doc_links)]

/// Account selection and state-directory configuration.
pub mod config;
/// Validated local calendar dates.
pub mod date;
/// User-defined password derivation rule.
pub mod password_rule;
/// Durable per-account rotation state.
pub mod state;
#[cfg(windows)]
/// Native Windows account, console, and synchronization operations.
pub mod windows;

#[cfg(not(windows))]
compile_error!("WinPasswordLock only supports Windows");

use config::Config;
use date::Date;
use state::State;
use std::env;
use std::path::{Path, PathBuf};

/// Holds validated passwords and state for one rotation operation.
struct AccountPlan {
    /// Name of the managed local account.
    account: String,
    /// State file assigned to the account.
    path: PathBuf,
    /// Committed date and optional pending date.
    state: State,
    /// Password derived for the committed date.
    current_password: String,
    /// Password derived for an interrupted pending date.
    pending_password: Option<String>,
    /// Password derived for the current local date.
    today_password: String,
}

/// Validates the value returned by the user-supplied password rule.
fn password_for(account: &str, date: Date) -> Result<String, String> {
    let password = password_rule::password_for(account, date)?;
    if password.is_empty() || password.contains('\0') {
        return Err(format!(
            "rule returned an empty or invalid password for {account:?} on {date}"
        ));
    }
    Ok(password)
}

/// Derives today's passwords in configuration order
fn derive_today_passwords(config: &Config, today: Date) -> Result<Vec<String>, String> {
    config
        .accounts
        .iter()
        .map(|account| password_for(account, today))
        .collect()
}

/// Checks that the recovery and managed accounts exist and are enabled.
fn check_local_accounts(config: &Config) -> Result<(), String> {
    for account in std::iter::once(&config.recovery_account).chain(&config.accounts) {
        if !windows::local_user_enabled(account)? {
            return Err(format!(
                "local account {account:?} does not exist or is disabled"
            ));
        }
    }
    Ok(())
}

/// Prepares every account before any password change is attempted.
fn prepare(config: &Config, today: Date) -> Result<Vec<AccountPlan>, String> {
    check_local_accounts(config)?;
    let today_passwords = derive_today_passwords(config, today)?;
    let mut plans = Vec::new();
    for (account, today_password) in config.accounts.iter().zip(today_passwords) {
        // account's info file path
        let path = state::path_for(&config.state_dir, account);
        let state = state::load(&path)?
            .ok_or_else(|| format!("{account:?} is not enrolled; run enroll first"))?;
        if state.current > today || state.pending.is_some_and(|pending| pending > today) {
            return Err(format!(
                "clock moved backward for {account:?}; stored date is later than {today}"
            ));
        }

        let current_password = password_for(account, state.current)?;
        let pending_password = state
            .pending
            .map(|date| password_for(account, date))
            .transpose()?;
        // two password cannot be same
        if pending_password
            .as_ref()
            .is_some_and(|password| password == &current_password)
        {
            return Err(format!(
                "rule returned identical passwords for consecutive dates for {account:?}"
            ));
        }
        let last_password = pending_password.as_ref().unwrap_or(&current_password);
        let last_date = state.pending.unwrap_or(state.current);
        if last_date < today && last_password == &today_password {
            return Err(format!(
                "rule returned identical passwords for different dates for {account:?}"
            ));
        }
        plans.push(AccountPlan {
            account: account.clone(),
            path,
            state,
            current_password,
            pending_password,
            today_password,
        });
    }
    Ok(plans)
}

/// Applies a change or confirms that an interrupted change already succeeded.
fn change_or_confirm(account: &str, old: &str, new: &str) -> Result<(), String> {
    match windows::change_password(account, old, new) {
        Ok(()) => Ok(()),
        Err(windows::ChangeError::Setup(error)) => Err(error),
        Err(windows::ChangeError::Windows(status)) if status == 86 || status == 1323 => {
            if windows::password_matches(account, new)? {
                Ok(())
            } else {
                Err(format!(
                    "password change failed for {account:?}: Windows status {status}"
                ))
            }
        }
        Err(windows::ChangeError::Windows(status)) => Err(format!(
            "password change failed for {account:?}: Windows status {status}"
        )),
    }
}

/// Completes a pending change, then rotates one account to today's date.
fn rotate_one(mut plan: AccountPlan, today: Date) -> Result<(), String> {
    if let (Some(pending_date), Some(pending_password)) =
        (plan.state.pending, plan.pending_password.take())
    {
        change_or_confirm(&plan.account, &plan.current_password, &pending_password)?;
        plan.state = State {
            current: pending_date,
            pending: None,
        };
        state::save(&plan.path, plan.state)?;
        plan.current_password = pending_password;
        println!(
            "{}: recovered pending rotation to {}",
            plan.account, pending_date
        );
    }
    if plan.state.current == today {
        println!("{}: already current", plan.account);
        return Ok(());
    }
    state::save(
        &plan.path,
        State {
            current: plan.state.current,
            pending: Some(today),
        },
    )?;
    change_or_confirm(&plan.account, &plan.current_password, &plan.today_password)?;
    state::save(
        &plan.path,
        State {
            current: today,
            pending: None,
        },
    )?;
    println!("{}: rotated to {}", plan.account, today);
    Ok(())
}

/// Enrolls one account by changing its known current password to today's value.
fn enroll(config: &Config, account: &str, today: Date) -> Result<(), String> {
    check_local_accounts(config)?;
    if !config
        .accounts
        .iter()
        .any(|name| name.eq_ignore_ascii_case(account))
    {
        return Err(format!("{account:?} is not listed in config"));
    }
    let account = config
        .accounts
        .iter()
        .find(|name| name.eq_ignore_ascii_case(account))
        .unwrap();
    let path = state::path_for(&config.state_dir, account);
    if state::load(&path)?.is_some() {
        return Err(format!("{account:?} is already enrolled"));
    }
    let passwords = derive_today_passwords(config, today)?;
    let index = config
        .accounts
        .iter()
        .position(|name| name == account)
        .unwrap();
    let new_password = &passwords[index];
    let old_password = // read old password from console
        windows::read_password(&format!("Current Windows password for {account}: "))?;
    if old_password != *new_password {
        change_or_confirm(account, &old_password, new_password)?;
    } else if !windows::password_matches(account, new_password)? {
        return Err(format!(
            "Windows did not accept the current password for {account:?}"
        ));
    }
    state::save(
        &path,
        State {
            current: today,
            pending: None,
        },
    )?;
    println!("{account}: enrolled on {today}");
    Ok(())
}

/// Prints enrollment state without calculating or revealing passwords.
fn status(config: &Config, today: Date) -> Result<(), String> {
    println!(
        "Local date: {today}; recovery account: {}",
        config.recovery_account
    );
    for account in &config.accounts {
        let path = state::path_for(&config.state_dir, account);
        match state::load(&path)? {
            Some(state) => println!(
                "{account}: current={} pending={:?}",
                state.current, state.pending
            ),
            None => println!("{account}: not enrolled"),
        }
    }
    Ok(())
}

/// Parses command-line arguments and runs one requested operation.
///
/// `status` and `check` are read-only. `enroll` and `rotate` can change real
/// Windows account passwords.
pub fn run_cli() -> Result<(), String> {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.len() < 3 || args[1] != "--config" {
        return Err(
            "usage: win-password-lock <status|check|enroll|rotate> --config <path> [account]"
                .into(),
        );
    }
    let command = args[0].as_str();
    let config = Config::load(Path::new(&args[2]))?;
    let today = Date::today()?;
    match command {
        "status" if args.len() == 3 => status(&config, today),
        "check" if args.len() == 3 => {
            let plans = prepare(&config, today)?;
            for plan in plans {
                println!(
                    "{}: current={} pending={:?} target={}",
                    plan.account, plan.state.current, plan.state.pending, today
                );
            }
            Ok(())
        }
        "enroll" if args.len() == 4 => {
            let _lock = windows::RotationLock::acquire()?;
            enroll(&config, &args[3], today)
        }
        "rotate" if args.len() == 3 => {
            let _lock = windows::RotationLock::acquire()?;
            let plans = prepare(&config, today)?;
            for plan in plans {
                rotate_one(plan, today)?;
            }
            Ok(())
        }
        _ => Err("invalid command arguments".into()),
    }
}

#[cfg(test)]
/// Regression tests for password preparation without Windows password changes.
mod tests {
    use super::{Config, Date, derive_today_passwords};
    use std::path::PathBuf;

    #[test]
    /// Distinct accounts may receive the same password from the temporary rule.
    fn allows_shared_daily_passwords() {
        let config = Config {
            recovery_account: "RescueAdmin".into(),
            accounts: vec!["Alice".into(), "Bob".into()],
            state_dir: PathBuf::from(r"C:\ProgramData\WinPasswordLock\state"),
        };
        let passwords = derive_today_passwords(&config, Date::parse("20261004").unwrap())
            .expect("shared passwords should be accepted");
        assert_eq!(passwords, vec!["1004", "1004"]);
    }
}
