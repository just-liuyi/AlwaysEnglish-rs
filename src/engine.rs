//! The monitoring loop: keeps the foreground window on the English (US) layout.
//!
//! Same idea as `ControlInputLayout` in the original version: every 100 ms look at
//! the foreground window, and if it should be English but isn't, post
//! `WM_INPUTLANGCHANGEREQUEST` to it; optionally force Caps Lock on.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use windows::Win32::UI::Input::KeyboardAndMouse::HKL;

use crate::config::{AppEntry, Mode, exe_key};
use crate::platform;

pub const TICK: Duration = Duration::from_millis(100);
/// Refresh the cached layout handle and process names every ~5 s.
const REFRESH_TICKS: u32 = 50;
/// Don't press Caps Lock more often than this, in case the key state lags behind.
const CAPS_DEBOUNCE: Duration = Duration::from_millis(400);

#[derive(Clone, Copy)]
struct Rule {
    caps_lock: bool,
}

pub struct Engine {
    mode: Mode,
    always_caps_lock: bool,
    /// Lower-cased exe file name -> rule, for enabled apps only.
    rules: HashMap<String, Rule>,
    english: Option<HKL>,
    /// Process id -> lower-cased exe file name.
    names: HashMap<u32, Option<String>>,
    ticks: u32,
    own_pid: u32,
    last_caps_press: Option<Instant>,
}

impl Engine {
    pub fn new() -> Self {
        Self {
            mode: Mode::Stopped,
            always_caps_lock: false,
            rules: HashMap::new(),
            english: None,
            names: HashMap::new(),
            ticks: 0,
            own_pid: std::process::id(),
            last_caps_press: None,
        }
    }

    pub fn set_mode(&mut self, mode: Mode) {
        self.mode = mode;
    }

    pub fn set_always_caps_lock(&mut self, on: bool) {
        self.always_caps_lock = on;
    }

    pub fn set_apps(&mut self, apps: &[AppEntry]) {
        self.rules.clear();
        for app in apps.iter().filter(|a| a.enabled) {
            let rule = self.rules.entry(app.exe_key()).or_insert(Rule { caps_lock: false });
            rule.caps_lock |= app.caps_lock;
        }
    }

    pub fn tick(&mut self) {
        if self.mode == Mode::Stopped {
            return;
        }
        self.ticks = self.ticks.wrapping_add(1);
        if self.english.is_none() || self.ticks.is_multiple_of(REFRESH_TICKS) {
            self.english = platform::english_layout();
            self.names.clear();
        }

        let Some(fg) = platform::foreground() else { return };
        let caps_lock = match self.mode {
            Mode::Stopped => return,
            Mode::Always => self.always_caps_lock,
            Mode::PerApp => {
                if fg.process_id == self.own_pid {
                    return;
                }
                let name = self
                    .names
                    .entry(fg.process_id)
                    .or_insert_with(|| platform::process_path(fg.process_id).map(|p| exe_key(&p)));
                match name.as_ref().and_then(|n| self.rules.get(n)) {
                    Some(rule) => rule.caps_lock,
                    None => return,
                }
            }
        };

        if !platform::thread_layout_is_english(fg.thread_id)
            && let Some(hkl) = self.english
        {
            platform::request_layout(fg.hwnd, hkl);
            #[cfg(debug_assertions)]
            eprintln!("[engine] requested English for pid {}", fg.process_id);
        }

        if caps_lock && !platform::caps_lock_on() {
            let now = Instant::now();
            if self.last_caps_press.is_none_or(|t| now.duration_since(t) >= CAPS_DEBOUNCE) {
                self.last_caps_press = Some(now);
                platform::toggle_caps_lock();
                #[cfg(debug_assertions)]
                eprintln!("[engine] Caps Lock pressed");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rules_match_by_file_name_and_merge_caps() {
        let mut e = Engine::new();
        let mut a = AppEntry::new(r"C:\A\Game.exe".into());
        a.caps_lock = false;
        let mut b = AppEntry::new(r"D:\B\game.exe".into());
        b.caps_lock = true;
        let mut c = AppEntry::new(r"C:\C\Off.exe".into());
        c.enabled = false;
        e.set_apps(&[a, b, c]);
        assert!(e.rules.get("game.exe").unwrap().caps_lock);
        assert!(!e.rules.contains_key("off.exe"));
        // The original matched with a substring test; "me.exe" must not match "game.exe".
        assert!(!e.rules.contains_key("me.exe"));
    }
}
