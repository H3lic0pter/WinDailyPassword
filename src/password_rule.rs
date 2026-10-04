//! Derives a deterministic password from an account and local calendar date.
//!
//! The current `MMDD` implementation is a temporary example. It ignores the
//! account, repeats every year, and is not safe for real account protection.

use crate::date::Date;

/// Temporary rule: use the local month and day as a four-digit password.
/// Replace this with a private rule before deployment; accounts may share values.
///
/// # Example
///
/// ```
/// use win_password_lock::date::Date;
/// use win_password_lock::password_rule::password_for;
///
/// let date = Date::parse("20261004").unwrap();
/// assert_eq!(password_for("Alice", date).unwrap(), "1004");
/// ```
pub fn password_for(_account: &str, date: Date) -> Result<String, String> {
    Ok(format!("{:02}{:02}", date.month, date.day))
}

#[cfg(test)]
/// Tests the visible format of the temporary rule.
mod tests {
    use super::password_for;
    use crate::date::Date;

    #[test]
    /// Month and day always occupy two digits each.
    fn formats_month_and_day_with_leading_zeroes() {
        assert_eq!(
            password_for("Alice", Date::parse("20261004").unwrap()).unwrap(),
            "1004"
        );
        assert_eq!(
            password_for("Alice", Date::parse("20260102").unwrap()).unwrap(),
            "0102"
        );
    }
}
