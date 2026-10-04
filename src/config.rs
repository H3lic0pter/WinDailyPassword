//! Parses the explicit local-account allowlist and recovery-account exclusion.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

/// Configuration shared by status, validation, enrollment, and rotation.
pub struct Config {
    /// Local administrator kept outside daily rotation for recovery.
    pub recovery_account: String,
    /// Local accounts whose real passwords are rotated.
    pub accounts: Vec<String>,
    /// Directory containing one state file per managed account.
    pub state_dir: PathBuf,
}

/// File loading and value validation for [`Config`].
impl Config {
    /// Loads and validates a UTF-8 configuration file.
    pub fn load(path: &Path) -> Result<Self, String> {
        let text =
            fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
        Self::parse(&text)
    }

    /// Parses configuration text without accessing the filesystem.
    fn parse(text: &str) -> Result<Self, String> {
        let mut recovery_account = None;
        let mut state_dir = None;
        let mut accounts = Vec::new();
        // Remove BOM
        for (index, raw_line) in text.trim_start_matches('\u{feff}').lines().enumerate() {
            let line = raw_line.trim();
            // Remove comment
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let (key, value) = line
                .split_once('=')
                .ok_or_else(|| format!("config line {} needs key=value", index + 1))?;
            let value = value.trim();
            if value.is_empty() || value.chars().any(|ch| ch.is_control()) {
                return Err(format!("invalid value on config line {}", index + 1));
            }
            match key.trim() {
                "recovery_account" if recovery_account.is_none() => {
                    recovery_account = Some(value.to_owned())
                }
                "state_dir" if state_dir.is_none() => state_dir = Some(PathBuf::from(value)),
                "account" => accounts.push(value.to_owned()),
                other => {
                    return Err(format!(
                        "unknown or repeated config key {other:?} on line {}",
                        index + 1
                    ));
                }
            }
        }
        let recovery_account = recovery_account.ok_or("missing recovery_account")?;
        let state_dir: PathBuf = state_dir.ok_or("missing state_dir")?;
        if !state_dir.is_absolute() {
            return Err("state_dir must be an absolute path".into());
        }
        if accounts.is_empty() {
            return Err("at least one account= entry is required".into());
        }
        let mut seen = HashSet::new();
        for account in &accounts {
            let folded = account.to_lowercase();
            if folded == recovery_account.to_lowercase() {
                return Err("recovery account cannot be managed".into());
            }
            if !seen.insert(folded) {
                return Err(format!("duplicate account {account:?}"));
            }
        }
        Ok(Self {
            recovery_account,
            accounts,
            state_dir,
        })
    }
}

#[cfg(test)]
/// Tests configuration constraints that prevent accidental account selection.
mod tests {
    use super::Config;

    #[test]
    /// The recovery account cannot also be scheduled for rotation.
    fn rejects_recovery_account_in_rotation_set() {
        let input = "recovery_account=Rescue\nstate_dir=C:\\state\naccount=rescue\n";
        assert!(Config::parse(input).is_err());
    }

    #[test]
    /// Distinct local account names remain in configuration order.
    fn accepts_distinct_accounts() {
        let input = "recovery_account=Rescue\nstate_dir=C:\\state\naccount=Alice\naccount=Bob\n";
        assert_eq!(Config::parse(input).unwrap().accounts.len(), 2);
    }
}
