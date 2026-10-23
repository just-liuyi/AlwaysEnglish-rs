//! Single-instance support.
//!
//! The first instance owns a named mutex and listens on a named pipe. A second
//! instance detects the mutex, asks the first one (through the pipe) to show its
//! window, allows it to take the foreground, and exits.

use std::io::Write;
use std::time::Duration;

use windows::Win32::Foundation::{
    CloseHandle, ERROR_ACCESS_DENIED, ERROR_ALREADY_EXISTS, ERROR_PIPE_CONNECTED, GetLastError, HANDLE,
};
use windows::Win32::Storage::FileSystem::{PIPE_ACCESS_INBOUND, ReadFile};
use windows::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS,
    PIPE_TYPE_BYTE, PIPE_WAIT,
};
use windows::Win32::System::RemoteDesktop::ProcessIdToSessionId;
use windows::Win32::System::Threading::{CreateMutexW, GetCurrentProcessId};
use windows::Win32::UI::WindowsAndMessaging::{ASFW_ANY, AllowSetForegroundWindow};
use windows::core::HSTRING;

const SHOW_COMMAND: &[u8] = b"show";

fn instance_id() -> String {
    let mut session = 0u32;
    unsafe {
        let _ = ProcessIdToSessionId(GetCurrentProcessId(), &mut session);
    }
    let user: String = std::env::var("USERNAME")
        .unwrap_or_default()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect();
    format!("AlwaysEnglish-rs-{session}-{user}")
}

fn pipe_name() -> String {
    format!(r"\\.\pipe\{}", instance_id())
}

/// Keeps the instance mutex alive for the lifetime of the process.
pub struct InstanceGuard(HANDLE);

impl Drop for InstanceGuard {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }
}

pub enum Instance {
    Primary(InstanceGuard),
    Secondary,
}

pub fn acquire() -> Instance {
    let name = HSTRING::from(format!("Local\\{}", instance_id()));
    unsafe {
        match CreateMutexW(None, true, &name) {
            Ok(handle) => {
                if GetLastError() == ERROR_ALREADY_EXISTS {
                    let _ = CloseHandle(handle);
                    Instance::Secondary
                } else {
                    Instance::Primary(InstanceGuard(handle))
                }
            }
            // The mutex exists but belongs to an elevated instance.
            Err(e) if e.code() == ERROR_ACCESS_DENIED.to_hresult() => Instance::Secondary,
            // Fail open: better two instances than none.
            Err(_) => Instance::Primary(InstanceGuard(HANDLE::default())),
        }
    }
}

/// Called by a second instance: asks the running one to show its window.
pub fn notify_primary() -> bool {
    unsafe {
        // We are the process the user just launched, so we may hand the foreground over.
        let _ = AllowSetForegroundWindow(ASFW_ANY);
    }
    let name = pipe_name();
    for _ in 0..30 {
        match std::fs::OpenOptions::new().write(true).open(&name) {
            Ok(mut pipe) => return pipe.write_all(SHOW_COMMAND).is_ok(),
            Err(_) => std::thread::sleep(Duration::from_millis(100)),
        }
    }
    false
}

/// Starts a background thread that calls `on_show` whenever another instance is launched.
pub fn listen(on_show: impl Fn() + Send + 'static) {
    let name = HSTRING::from(pipe_name());
    std::thread::Builder::new()
        .name("single-instance".into())
        .spawn(move || {
            loop {
                let pipe = unsafe {
                    CreateNamedPipeW(
                        &name,
                        PIPE_ACCESS_INBOUND,
                        PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                        1,
                        0,
                        256,
                        0,
                        None,
                    )
                };
                if pipe.is_invalid() {
                    std::thread::sleep(Duration::from_secs(1));
                    continue;
                }
                let connected =
                    unsafe { ConnectNamedPipe(pipe, None).is_ok() || GetLastError() == ERROR_PIPE_CONNECTED };
                if connected {
                    let mut buf = [0u8; 64];
                    let mut read = 0u32;
                    let ok = unsafe { ReadFile(pipe, Some(&mut buf), Some(&mut read), None).is_ok() };
                    if ok && buf[..read as usize].starts_with(SHOW_COMMAND) {
                        on_show();
                    }
                }
                unsafe {
                    let _ = DisconnectNamedPipe(pipe);
                    let _ = CloseHandle(pipe);
                }
            }
        })
        .expect("failed to start the single-instance listener");
}
