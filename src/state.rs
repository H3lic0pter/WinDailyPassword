//! Stores the committed date and a pending target for interrupted rotations.

use crate::date::Date;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

/// Persistent progress for one managed local account.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct State {
    /// Date whose derived password was last committed.
    pub current: Date,
    /// Date written before an in-progress password change.
    pub pending: Option<Date>,
}

/// Builds a case-insensitive, filesystem-safe state path for an account.
pub fn path_for(dir: &Path, account: &str) -> PathBuf {
    let mut file_name = String::from("account-");
    let normalized = account.to_lowercase();
    for byte in normalized.as_bytes() {
        file_name.push_str(&format!("{byte:02x}"));
    }
    file_name.push_str(".state");
    dir.join(file_name)
}

/// Reads and validates an account state file, if one exists.
pub fn load(path: &Path) -> Result<Option<State>, String> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("{}: {error}", path.display())),
    };
    let mut current = None;
    let mut pending = None;
    for line in text.lines() {
        if let Some(value) = line.strip_prefix("current=") {
            if current.replace(Date::parse(value)?).is_some() {
                return Err("duplicate current date in state".into());
            }
        } else if let Some(value) = line.strip_prefix("pending=") {
            if pending.replace(Date::parse(value)?).is_some() {
                return Err("duplicate pending date in state".into());
            }
        } else {
            return Err(format!("invalid state line {line:?}"));
        }
    }
    let current = current.ok_or("missing current date in state")?;
    if pending.is_some_and(|date| date <= current) {
        return Err("pending date must be after current date".into());
    }
    Ok(Some(State { current, pending }))
}

/// Writes state through a synced temporary file and replacement rename.
pub fn save(path: &Path, state: State) -> Result<(), String> {
    let parent = path.parent().ok_or("state path has no parent")?;
    fs::create_dir_all(parent).map_err(|error| format!("{}: {error}", parent.display()))?;
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|error| error.to_string())?
        .as_nanos();
    let tmp = path.with_extension(format!("tmp-{}-{nonce}", std::process::id()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&tmp)
        .map_err(|error| format!("{}: {error}", tmp.display()))?;
    let contents = match state.pending {
        Some(pending) => format!("current={}\npending={}\n", state.current, pending),
        None => format!("current={}\n", state.current),
    };
    let result = (|| {
        file.write_all(contents.as_bytes())?;
        file.sync_all()?;
        fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result.map_err(|error| format!("{}: {error}", path.display()))
}

#[cfg(test)]
/// Tests pending-state recovery and replacement writes.
mod tests {
    use super::{State, load, path_for, save};
    use crate::date::Date;

    #[test]
    /// Pending and committed values survive separate file reads.
    fn pending_state_survives_restart() {
        let dir =
            std::env::temp_dir().join(format!("win-password-lock-test-{}", std::process::id()));
        let path = path_for(&dir, "Alice");
        let state = State {
            current: Date::parse("20261003").unwrap(),
            pending: Some(Date::parse("20261004").unwrap()),
        };
        save(&path, state).unwrap();
        assert_eq!(load(&path).unwrap(), Some(state));
        let committed = State {
            current: state.pending.unwrap(),
            pending: None,
        };
        save(&path, committed).unwrap();
        assert_eq!(load(&path).unwrap(), Some(committed));
        assert_eq!(path, path_for(&dir, "alice"));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
