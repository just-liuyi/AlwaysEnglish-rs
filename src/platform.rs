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

/// Primary language id of English. Every English variant has it in the low 10 bits of its
/// language id: US `0x0409`, UK `0x0809`, India `0x4009`, Australia `0x0C09`, ...
const LANG_ENGLISH: usize = 0x09;
const LANG_EN_US: usize = 0x0409;
const LANG_EN_GB: usize = 0x0809;

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

/// Language id of a keyboard layout (the low word of the layout handle).
pub fn langid_of(hkl: HKL) -> usize {
    (hkl.0 as usize) & 0xFFFF
}

fn primary_language(langid: usize) -> usize {
    langid & 0x3FF
}

/// Whether a language id is English of any region (US, UK, India, Australia, ...).
pub fn is_english_langid(langid: usize) -> bool {
    primary_language(langid) == LANG_ENGLISH
}

/// Picks which of the installed layouts to keep apps on.
///
/// Any English variant is accepted: only the primary language matters, not the region.
/// When several English layouts are installed, the one matching the Windows display
/// language wins, then English (US), then English (UK), otherwise the first one found.
fn pick_english_layout(langids: &[usize], display_language: usize) -> Option<usize> {
    let english: Vec<usize> = (0..langids.len()).filter(|i| is_english_langid(langids[*i])).collect();
    let first = *english.first()?;
    if is_english_langid(display_language)
        && let Some(i) = english.iter().copied().find(|i| langids[*i] == display_language)
    {
        return Some(i);
    }
    english
        .iter()
        .copied()
        .find(|i| langids[*i] == LANG_EN_US)
        .or_else(|| english.iter().copied().find(|i| langids[*i] == LANG_EN_GB))
        .or(Some(first))
}

fn display_language() -> usize {
    unsafe { windows::Win32::Globalization::GetUserDefaultUILanguage() as usize }
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

/// An installed English keyboard layout, of any region.
pub fn installed_english_layout() -> Option<HKL> {
    let layouts = installed_layouts();
    let langids: Vec<usize> = layouts.iter().map(|h| langid_of(*h)).collect();
    let index = pick_english_layout(&langids, display_language())?;
    Some(layouts[index])
}

/// Whether any English keyboard layout is installed (any region).
pub fn is_english_installed() -> bool {
    installed_english_layout().is_some()
}

/// The layout to request: the installed English layout (any region), or — only when the
/// system has no English layout at all — English (US) loaded on demand, like the original
/// version did with `LoadKeyboardLayout("00000409", KLF_ACTIVATE)`.
pub fn english_layout() -> Option<HKL> {
    installed_english_layout().or_else(|| unsafe { LoadKeyboardLayoutW(w!("00000409"), KLF_ACTIVATE).ok() })
}

/// Whether a thread's current keyboard layout is English of any region.
pub fn thread_layout_is_english(thread_id: u32) -> bool {
    unsafe { is_english_langid(langid_of(GetKeyboardLayout(thread_id))) }
}

/// Friendly name of an English layout, e.g. `English (India)` / `英语（印度）`.
pub fn english_layout_name(langid: usize, chinese: bool) -> String {
    let (en, zh) = match langid {
        0x0409 => ("English (United States)", "英语（美国）"),
        0x0809 => ("English (United Kingdom)", "英语（英国）"),
        0x0C09 => ("English (Australia)", "英语（澳大利亚）"),
        0x1009 => ("English (Canada)", "英语（加拿大）"),
        0x1409 => ("English (New Zealand)", "英语（新西兰）"),
        0x1809 => ("English (Ireland)", "英语（爱尔兰）"),
        0x1C09 => ("English (South Africa)", "英语（南非）"),
        0x2009 => ("English (Jamaica)", "英语（牙买加）"),
        0x2409 => ("English (Caribbean)", "英语（加勒比）"),
        0x2809 => ("English (Belize)", "英语（伯利兹）"),
        0x2C09 => ("English (Trinidad and Tobago)", "英语（特立尼达和多巴哥）"),
        0x3009 => ("English (Zimbabwe)", "英语（津巴布韦）"),
        0x3409 => ("English (Philippines)", "英语（菲律宾）"),
        0x3C09 => ("English (Hong Kong SAR)", "英语（中国香港）"),
        0x4009 => ("English (India)", "英语（印度）"),
        0x4409 => ("English (Malaysia)", "英语（马来西亚）"),
        0x4809 => ("English (Singapore)", "英语（新加坡）"),
        _ => ("English", "英语"),
    };
    if chinese { zh.to_owned() } else { en.to_owned() }
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

/// Whether `HKCU\...\Run` already starts this executable (e.g. set up by the installer).
pub fn auto_start_registered() -> bool {
    let Ok(exe) = std::env::current_exe() else { return false };
    let exe = exe.display().to_string().to_lowercase();
    windows_registry::CURRENT_USER
        .open(RUN_KEY)
        .and_then(|k| k.get_string(RUN_VALUE))
        .map(|v| v.to_lowercase().contains(&exe))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ZH_CN: usize = 0x0804;
    const EN_US: usize = 0x0409;
    const EN_GB: usize = 0x0809;
    const EN_IN: usize = 0x4009;
    const EN_AU: usize = 0x0C09;
    const EN_HK: usize = 0x3C09;

    #[test]
    fn every_english_region_counts_as_english() {
        for langid in [EN_US, EN_GB, EN_IN, EN_AU, EN_HK, 0x1009, 0x1C09, 0x4409] {
            assert!(is_english_langid(langid), "{langid:#06x} should be English");
        }
        for langid in [ZH_CN, 0x0411, 0x0407, 0x0411] {
            assert!(!is_english_langid(langid), "{langid:#06x} is not English");
        }
    }

    #[test]
    fn a_single_non_us_english_layout_is_used() {
        // The reported bug: English (India) or English (Hong Kong) alone used to be rejected.
        assert_eq!(pick_english_layout(&[ZH_CN, EN_IN], ZH_CN), Some(1));
        assert_eq!(pick_english_layout(&[EN_HK], ZH_CN), Some(0));
        assert_eq!(pick_english_layout(&[EN_AU, ZH_CN], ZH_CN), Some(0));
    }

    #[test]
    fn no_english_layout_at_all_is_none() {
        assert_eq!(pick_english_layout(&[ZH_CN], ZH_CN), None);
        assert_eq!(pick_english_layout(&[], ZH_CN), None);
    }

    #[test]
    fn preference_between_several_english_layouts() {
        // Display language wins when it is English and installed.
        assert_eq!(pick_english_layout(&[EN_US, EN_GB], EN_GB), Some(1));
        // Otherwise US, then UK, then whatever is there.
        assert_eq!(pick_english_layout(&[EN_IN, EN_GB, EN_US], ZH_CN), Some(2));
        assert_eq!(pick_english_layout(&[EN_IN, EN_GB], ZH_CN), Some(1));
        assert_eq!(pick_english_layout(&[EN_IN, EN_AU], ZH_CN), Some(0));
        // A Chinese display language must not make English layouts ineligible.
        assert_eq!(pick_english_layout(&[ZH_CN, EN_IN], 0x0411), Some(1));
    }

    #[test]
    fn names_are_localized() {
        assert_eq!(english_layout_name(EN_IN, false), "English (India)");
        assert_eq!(english_layout_name(EN_IN, true), "英语（印度）");
        assert_eq!(english_layout_name(0x4409, true), "英语（马来西亚）");
        assert_eq!(english_layout_name(0x04FF, true), "英语");
    }

    /// Checks the detection against the real machine: "an English layout is installed" must
    /// agree with the list Windows reports, whatever region that English layout is from.
    #[test]
    fn detection_matches_the_installed_layouts() {
        let langids: Vec<usize> = installed_layouts().iter().map(|h| langid_of(*h)).collect();
        let any_english = langids.iter().any(|l| is_english_langid(*l));
        assert_eq!(is_english_installed(), any_english, "installed layouts: {langids:04X?}");
        if let Some(hkl) = installed_english_layout() {
            assert!(is_english_langid(langid_of(hkl)));
        }
    }
}
