//! Thin wrappers around the Win32 APIs the app needs.

use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM, WPARAM};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx, IPersistFile,
    STGM_READ,
};
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, GetKeyboardLayout, GetKeyboardLayoutList, HKL, INPUT, INPUT_0, INPUT_KEYBOARD,
    KEYBD_EVENT_FLAGS, KEYBDINPUT, KEYEVENTF_KEYUP, KLF_ACTIVATE, LoadKeyboardLayoutW, SendInput,
    VK_CAPITAL,
};
use windows::Win32::UI::Shell::{IShellLinkW, ShellExecuteW, ShellLink};
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowThreadProcessId, IsIconic, PostMessageW, SW_RESTORE,
    SW_SHOWNORMAL, SetForegroundWindow, ShowWindow, WM_INPUTLANGCHANGEREQUEST,
};
use windows::core::{HSTRING, Interface, PCWSTR, PWSTR, w};

/// Primary language id of English (United States).
pub const LANG_EN_US: usize = 0x0409;

pub fn init_com() {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }
}

/// The foreground window together with its thread id and process id.
#[derive(Clone, Copy)]
pub struct Foreground {
    pub hwnd: HWND,
    pub thread_id: u32,
    pub process_id: u32,
}

pub fn foreground() -> Option<Foreground> {
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_invalid() {
            return None;
        }
        let mut process_id = 0;
        let thread_id = GetWindowThreadProcessId(hwnd, Some(&mut process_id));
        (thread_id != 0).then_some(Foreground { hwnd, thread_id, process_id })
    }
}

/// Full image path of a process (works for most elevated processes too).
pub fn process_path(pid: u32) -> Option<String> {
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = vec![0u16; 1024];
        let mut len = buf.len() as u32;
        let res = QueryFullProcessImageNameW(handle, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len);
        let _ = CloseHandle(handle);
        res.ok()?;
        Some(String::from_utf16_lossy(&buf[..len as usize]))
    }
}

fn layout_language(hkl: HKL) -> usize {
    (hkl.0 as usize) & 0xFFFF
}

fn installed_layouts() -> Vec<HKL> {
    unsafe {
        let count = GetKeyboardLayoutList(None);
        if count <= 0 {
            return Vec::new();
        }
        let mut list = vec![HKL::default(); count as usize];
        let n = GetKeyboardLayoutList(Some(&mut list));
        list.truncate(n.max(0) as usize);
        list
    }
}

/// An installed English (US) keyboard layout, preferring the plain US layout.
pub fn installed_english_layout() -> Option<HKL> {
    let layouts = installed_layouts();
    layouts
        .iter()
        .copied()
        .find(|h| (h.0 as usize & 0xFFFF_FFFF) == 0x0409_0409)
        .or_else(|| layouts.iter().copied().find(|h| layout_language(*h) == LANG_EN_US))
}

pub fn is_english_installed() -> bool {
    installed_english_layout().is_some()
}

/// The layout to request: an installed English (US) layout, or the US layout loaded on demand
/// (what the original version did with `LoadKeyboardLayout("00000409", KLF_ACTIVATE)`).
pub fn english_layout() -> Option<HKL> {
    installed_english_layout().or_else(|| unsafe { LoadKeyboardLayoutW(w!("00000409"), KLF_ACTIVATE).ok() })
}

pub fn thread_layout_is_english(thread_id: u32) -> bool {
    unsafe { layout_language(GetKeyboardLayout(thread_id)) == LANG_EN_US }
}

/// Asks the window to switch its input language.
pub fn request_layout(hwnd: HWND, hkl: HKL) {
    unsafe {
        let _ = PostMessageW(Some(hwnd), WM_INPUTLANGCHANGEREQUEST, WPARAM(0), LPARAM(hkl.0 as isize));
    }
}

pub fn caps_lock_on() -> bool {
    unsafe { (GetKeyState(VK_CAPITAL.0 as i32) & 1) != 0 }
}

/// Presses and releases Caps Lock.
pub fn toggle_caps_lock() {
    let key = |flags: KEYBD_EVENT_FLAGS| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT { wVk: VK_CAPITAL, wScan: 0x3a, dwFlags: flags, time: 0, dwExtraInfo: 0 },
        },
    };
    let inputs = [key(KEYBD_EVENT_FLAGS(0)), key(KEYEVENTF_KEYUP)];
    unsafe {
        SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
    }
}

/// Opens a URL, file or `ms-settings:` page with the shell.
pub fn shell_open(target: &str) {
    unsafe {
        ShellExecuteW(None, w!("open"), &HSTRING::from(target), PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL);
    }
}

pub fn open_language_settings() {
    shell_open("ms-settings:regionlanguage");
}

/// Resolves a `.lnk` shortcut to the path it points to.
pub fn resolve_shortcut(lnk: &str) -> Option<String> {
    unsafe {
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).ok()?;
        let file: IPersistFile = link.cast().ok()?;
        file.Load(&HSTRING::from(lnk), STGM_READ).ok()?;
        let mut buf = vec![0u16; 32768];
        link.GetPath(&mut buf, std::ptr::null_mut(), 0).ok()?;
        let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        let path = String::from_utf16_lossy(&buf[..len]);
        (!path.is_empty()).then_some(path)
    }
}

/// Whether Windows apps currently use the dark theme.
pub fn system_uses_dark_theme() -> bool {
    windows_registry::CURRENT_USER
        .open(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize")
        .and_then(|k| k.get_u32("AppsUseLightTheme"))
        .map(|v| v == 0)
        .unwrap_or(false)
}

/// Whether the Windows display language is Chinese.
pub fn system_language_is_chinese() -> bool {
    let lang = unsafe { windows::Win32::Globalization::GetUserDefaultUILanguage() };
    (lang & 0x3ff) == 0x04
}


pub fn bring_to_front(hwnd: HWND) {
    unsafe {
        if IsIconic(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }
        let _ = SetForegroundWindow(hwnd);
    }
}

/// The Win32 handle of a Slint window, if it has been created.
pub fn hwnd_of(window: &slint::Window) -> Option<HWND> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let handle = window.window_handle();
    let raw = handle.window_handle().ok()?.as_raw();
    match raw {
        RawWindowHandle::Win32(h) => Some(HWND(h.hwnd.get() as *mut _)),
        _ => None,
    }
}

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const RUN_VALUE: &str = "AlwaysEnglish";

/// Registers (or removes) the app in `HKCU\...\Run`. It starts hidden in the tray.
pub fn set_auto_start(enable: bool) -> Result<(), String> {
    let key = windows_registry::CURRENT_USER.create(RUN_KEY).map_err(|e| e.to_string())?;
    if enable {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        key.set_string(RUN_VALUE, format!("\"{}\" --minimized", exe.display()))
            .map_err(|e| e.to_string())?;
    } else {
        let _ = key.remove_value(RUN_VALUE);
    }
    Ok(())
}
