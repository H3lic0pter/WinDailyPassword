//! Safe wrappers around the Windows APIs used by the rotation workflow.
//!
//! Password strings cross an FFI boundary only inside this module. This module
//! also reads local account status, hides console input, and serializes writes.

use std::ffi::c_void;
use std::io::{self, Write};
use std::ptr;

/// Opaque Windows object handle used by console, token, and mutex APIs.
type Handle = *mut c_void;

/// Layout of the level-one account information returned by `NetUserGetInfo`.
#[repr(C)]
struct UserInfo1 {
    /// Account name pointer owned by the NetAPI buffer.
    name: *mut u16,
    /// Password pointer reserved by the Windows structure layout.
    password: *mut u16,
    /// Age of the current password in seconds.
    password_age: u32,
    /// Windows account privilege level.
    privilege: u32,
    /// Home-directory pointer owned by the NetAPI buffer.
    home_dir: *mut u16,
    /// Comment pointer owned by the NetAPI buffer.
    comment: *mut u16,
    /// Account flags, including the disabled bit.
    flags: u32,
    /// Logon-script pointer owned by the NetAPI buffer.
    script_path: *mut u16,
}

/// Layout of the local time returned by `GetLocalTime`.
#[repr(C)]
pub struct SystemTime {
    /// Local calendar year.
    pub year: u16,
    /// One-based local month.
    pub month: u16,
    /// Windows weekday number.
    pub day_of_week: u16,
    /// One-based local day of month.
    pub day: u16,
    /// Local hour in 24-hour form.
    pub hour: u16,
    /// Local minute.
    pub minute: u16,
    /// Local second.
    pub second: u16,
    /// Local milliseconds.
    pub milliseconds: u16,
}

// Kernel APIs for time, console input, handles, and process-wide locking.
#[link(name = "kernel32")]
unsafe extern "system" {
    /// Writes the current local system time into a caller-owned structure.
    fn GetLocalTime(time: *mut SystemTime);
    /// Writes the computer name into a caller-owned UTF-16 buffer.
    fn GetComputerNameW(buffer: *mut u16, length: *mut u32) -> i32;
    /// Returns a process standard handle.
    fn GetStdHandle(which: u32) -> Handle;
    /// Reads the current input-console flags.
    fn GetConsoleMode(handle: Handle, mode: *mut u32) -> i32;
    /// Sets input-console flags, including password echo behavior.
    fn SetConsoleMode(handle: Handle, mode: u32) -> i32;
    /// Returns the calling thread's last Windows error code.
    fn GetLastError() -> u32;
    /// Closes a token or mutex handle.
    fn CloseHandle(handle: Handle) -> i32;
    /// Opens or creates the named rotation mutex.
    fn CreateMutexW(attributes: *const c_void, initial_owner: i32, name: *const u16) -> Handle;
    /// Attempts to acquire a mutex without waiting.
    fn WaitForSingleObject(handle: Handle, timeout: u32) -> u32;
    /// Releases the named rotation mutex.
    fn ReleaseMutex(handle: Handle) -> i32;
}

// NetAPI calls for local account inspection and password changes.
#[link(name = "netapi32")]
unsafe extern "system" {
    /// Changes an account password after checking the previous password.
    fn NetUserChangePassword(
        domain: *const u16,
        user: *const u16,
        old_password: *const u16,
        new_password: *const u16,
    ) -> u32;
    /// Loads one account's level-one information from the local SAM.
    fn NetUserGetInfo(
        server: *const u16,
        user: *const u16,
        level: u32,
        buffer: *mut *mut u8,
    ) -> u32;
    /// Frees a buffer allocated by a NetAPI function.
    fn NetApiBufferFree(buffer: *mut c_void) -> u32;
}

// Authentication API used to confirm an interrupted password change.
#[link(name = "advapi32")]
unsafe extern "system" {
    /// Authenticates a local account and returns a token on success.
    fn LogonUserW(
        user: *const u16,
        domain: *const u16,
        password: *const u16,
        logon_type: u32,
        provider: u32,
        token: *mut Handle,
    ) -> i32;
}

/// Encodes a Rust string as a null-terminated UTF-16 Windows string.
fn wide(value: &str) -> Vec<u16> {
    // Add \0 to the char* end
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Reads the computer's local time without changing the system clock.
pub fn local_time() -> SystemTime {
    let mut time = SystemTime {
        year: 0,
        month: 0,
        day_of_week: 0,
        day: 0,
        hour: 0,
        minute: 0,
        second: 0,
        milliseconds: 0,
    };
    unsafe { GetLocalTime(&mut time) };
    time
}

/// Returns the local computer name as a null-terminated UTF-16 string.
fn computer_name() -> Result<Vec<u16>, String> {
    let mut buffer = vec![0u16; 256];
    let mut length = buffer.len() as u32;
    if unsafe { GetComputerNameW(buffer.as_mut_ptr(), &mut length) } == 0 {
        return Err(format!("GetComputerNameW failed: {}", unsafe {
            GetLastError()
        }));
    }
    buffer.truncate(length as usize);
    buffer.push(0);
    Ok(buffer)
}

/// Distinguishes local setup failures from NetAPI status codes.
pub enum ChangeError {
    /// The computer name could not be read before calling NetAPI.
    Setup(String),
    /// `NetUserChangePassword` returned a Windows status code.
    Windows(u32),
}

/// Changes the real password of one local account using its old password.
///
/// This function changes Windows account state on success. It does not write
/// the per-account rotation file; the caller handles that journal separately.
pub fn change_password(account: &str, old: &str, new: &str) -> Result<(), ChangeError> {
    let machine = computer_name().map_err(ChangeError::Setup)?;
    let account = wide(account);
    let old = wide(old);
    let new = wide(new);
    let status = unsafe {
        NetUserChangePassword(
            machine.as_ptr(),
            account.as_ptr(),
            old.as_ptr(),
            new.as_ptr(),
        )
    };
    if status == 0 {
        Ok(())
    } else {
        Err(ChangeError::Windows(status))
    }
}

/// Reports whether a named local account exists and is enabled.
pub fn local_user_enabled(account: &str) -> Result<bool, String> {
    let account = wide(account);
    let mut buffer = ptr::null_mut();
    let status = unsafe { NetUserGetInfo(ptr::null(), account.as_ptr(), 1, &mut buffer) };
    let enabled = status == 0
        && !buffer.is_null()
        && unsafe { (*(buffer as *const UserInfo1)).flags & 0x0002 == 0 };
    if !buffer.is_null() {
        unsafe { NetApiBufferFree(buffer.cast()) };
    }
    match status {
        0 => Ok(enabled),
        2221 => Ok(false),
        _ => Err(format!("NetUserGetInfo failed: Windows status {status}")),
    }
}

/// Checks whether Windows accepts a password for interactive local logon.
///
/// The returned token is closed immediately. Failed logons can affect the
/// machine's account lockout policy.
pub fn password_matches(account: &str, password: &str) -> Result<bool, String> {
    let machine = computer_name()?;
    let account = wide(account);
    let password = wide(password);
    let mut token = ptr::null_mut();
    let result = unsafe {
        LogonUserW(
            account.as_ptr(),
            machine.as_ptr(),
            password.as_ptr(),
            2,
            0,
            &mut token,
        )
    };
    if result == 0 {
        return Ok(false);
    }
    unsafe { CloseHandle(token) };
    Ok(true)
}

/// Restores console flags after a hidden password prompt.
struct ConsoleModeGuard {
    /// Console input handle whose mode was changed.
    handle: Handle,
    /// Original flags restored when the guard is dropped.
    mode: u32,
}

/// Restores console state when the guard leaves scope.
impl Drop for ConsoleModeGuard {
    /// Restores echo and other console flags even if input handling fails.
    fn drop(&mut self) {
        unsafe { SetConsoleMode(self.handle, self.mode) };
    }
}

/// Reads a current account password from a console without echoing characters.
pub fn read_password(prompt: &str) -> Result<String, String> {
    let handle = unsafe { GetStdHandle((-10i32) as u32) }; // Console Input handle
    if handle.is_null() || handle as isize == -1 {
        return Err("enroll requires an interactive Windows console".into());
    }
    let mut mode = 0;
    if unsafe { GetConsoleMode(handle, &mut mode) } == 0 {
        return Err("enroll requires an interactive Windows console".into());
    }
    print!("{prompt}");
    io::stdout().flush().map_err(|error| error.to_string())?;
    if unsafe { SetConsoleMode(handle, mode & !0x0004) } == 0 {
        // Disable ENABLE_ECHO_INPUT 0x0004
        return Err(format!("SetConsoleMode failed: {}", unsafe {
            GetLastError()
        }));
    }
    let guard = ConsoleModeGuard { handle, mode };
    let mut password = String::new();
    let result = io::stdin().read_line(&mut password);
    drop(guard);
    println!();
    result.map_err(|error| error.to_string())?;
    while password.ends_with(['\r', '\n']) {
        password.pop();
    }
    if password.is_empty() {
        return Err("empty current password is not supported".into());
    }
    Ok(password)
}

/// A named mutex preventing concurrent enrollment and rotation processes.
pub struct RotationLock(Handle);

/// Named-mutex acquisition for password-changing commands.
impl RotationLock {
    /// Acquires the global rotation mutex without waiting for another process.
    pub fn acquire() -> Result<Self, String> {
        let name = wide("Global\\WinPasswordLock-Rotation");
        let handle = unsafe { CreateMutexW(ptr::null(), 0, name.as_ptr()) };
        if handle.is_null() {
            return Err(format!("CreateMutexW failed: {}", unsafe {
                GetLastError()
            }));
        }
        match unsafe { WaitForSingleObject(handle, 0) } {
            0 | 0x80 => Ok(Self(handle)),
            0x102 => {
                unsafe { CloseHandle(handle) };
                Err("another rotation process is running".into())
            }
            value => {
                unsafe { CloseHandle(handle) };
                Err(format!("WaitForSingleObject failed: {value}"))
            }
        }
    }
}

/// Releases the process-wide rotation mutex at scope exit.
impl Drop for RotationLock {
    /// Releases and closes the mutex when an operation finishes.
    fn drop(&mut self) {
        unsafe {
            ReleaseMutex(self.0);
            CloseHandle(self.0);
        }
    }
}
