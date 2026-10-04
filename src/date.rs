//! Validates and formats dates used by the password rule and rotation state.

use std::fmt;

/// A valid Gregorian calendar date stored without a time zone or clock time.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct Date {
    /// Four-digit calendar year.
    pub year: u16,
    /// One-based calendar month.
    pub month: u8,
    /// One-based day of the month.
    pub day: u8,
}

/// Parsing and reading valid calendar dates.
impl Date {
    /// Parses a `YYYYMMDD` value and rejects impossible calendar dates.
    pub fn parse(value: &str) -> Result<Self, String> {
        if value.len() != 8 || !value.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(format!("invalid date {value:?}: expected YYYYMMDD"));
        }
        let year = value[0..4]
            .parse::<u16>()
            .map_err(|error| error.to_string())?;
        let month = value[4..6]
            .parse::<u8>()
            .map_err(|error| error.to_string())?;
        let day = value[6..8]
            .parse::<u8>()
            .map_err(|error| error.to_string())?;
        let days = match month {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 if year % 400 == 0 || (year % 4 == 0 && year % 100 != 0) => 29,
            2 => 28,
            _ => 0,
        };
        if year == 0 || day == 0 || day > days {
            return Err(format!("invalid calendar date {value:?}"));
        }
        Ok(Self { year, month, day })
    }

    #[cfg(windows)]
    /// Reads the Windows local calendar date at the time of the call.
    pub fn today() -> Result<Self, String> {
        let time = crate::windows::local_time();
        Self::parse(&format!("{:04}{:02}{:02}", time.year, time.month, time.day))
    }
}

/// Stable text representation used by state files.
impl fmt::Display for Date {
    /// Writes the date in the state-file `YYYYMMDD` format.
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(output, "{:04}{:02}{:02}", self.year, self.month, self.day)
    }
}

#[cfg(test)]
/// Tests calendar validation and ordering used by rollback checks.
mod tests {
    use super::Date;

    #[test]
    /// Leap days are accepted only for leap years.
    fn validates_calendar_dates() {
        assert!(Date::parse("20240229").is_ok());
        assert!(Date::parse("20250229").is_err());
        assert!(Date::parse("20251301").is_err());
        assert!(Date::parse("20250100").is_err());
    }

    #[test]
    /// Lexical date ordering also follows calendar ordering.
    fn dates_sort_in_calendar_order() {
        assert!(Date::parse("20261231").unwrap() < Date::parse("20270101").unwrap());
    }
}
