//! Persistent settings, stored as JSON in `%LOCALAPPDATA%\AlwaysEnglish\config.json`.
//!
//! The original Qt version stored its settings with `QSettings` in
//! `%LOCALAPPDATA%\AlwaysEnglish\AlwaysEnglish.ini`; on first launch that file is
//! migrated (see [`crate::legacy`]).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    /// Nothing is monitored.
    #[default]
    Stopped,
    /// Only the apps in the list (with `enabled`) are kept on English.
    PerApp,
    /// Every foreground window is kept on English.
    Always,
}

impl Mode {
    pub fn to_index(self) -> i32 {
        match self {
            Mode::Stopped => 0,
            Mode::PerApp => 1,
            Mode::Always => 2,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    #[default]
    System,
    Light,
    Dark,
}

impl Theme {
    pub fn to_index(self) -> i32 {
        match self {
            Theme::System => 0,
            Theme::Light => 1,
            Theme::Dark => 2,
        }
    }
    pub fn from_index(i: i32) -> Self {
        match i {
            1 => Theme::Light,
            2 => Theme::Dark,
            _ => Theme::System,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Language {
    #[default]
    System,
    English,
    Chinese,
}

impl Language {
    pub fn to_index(self) -> i32 {
        match self {
            Language::System => 0,
            Language::English => 1,
            Language::Chinese => 2,
        }
    }
    pub fn from_index(i: i32) -> Self {
        match i {
            1 => Language::English,
            2 => Language::Chinese,
            _ => Language::System,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CloseAction {
    #[default]
    Ask,
    Minimize,
    Quit,
}

impl CloseAction {
    pub fn to_index(self) -> i32 {
        match self {
            CloseAction::Ask => 0,
            CloseAction::Minimize => 1,
            CloseAction::Quit => 2,
        }
    }
    pub fn from_index(i: i32) -> Self {
        match i {
            1 => CloseAction::Minimize,
            2 => CloseAction::Quit,
            _ => CloseAction::Ask,
        }
    }
}

fn yes() -> bool {
    true
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct AppEntry {
    /// Full path of the executable.
    pub path: String,
    /// Display name (file name without extension).
    pub name: String,
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default)]
    pub caps_lock: bool,
}

impl AppEntry {
    pub fn new(path: String) -> Self {
        let name = Path::new(&path)
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.clone());
        Self { path, name, enabled: true, caps_lock: false }
    }

    /// Lower-cased executable file name (`code.exe`), used for matching.
    pub fn exe_key(&self) -> String {
        exe_key(&self.path)
    }
}

/// Lower-cased file name of a path, used to match the foreground process.
pub fn exe_key(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map(|s| s.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Config {
    pub apps: Vec<AppEntry>,
    pub theme: Theme,
    pub language: Language,
    /// Force Caps Lock on in "AlwaysEnglish" (all apps) mode.
    pub always_caps_lock: bool,
    pub auto_start: bool,
    /// Restore `last_mode` on launch.
    pub resume_last_mode: bool,
    pub last_mode: Mode,
    pub close_action: CloseAction,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            apps: Vec::new(),
            theme: Theme::System,
            language: Language::System,
            always_caps_lock: false,
            auto_start: false,
            resume_last_mode: true,
            last_mode: Mode::Stopped,
            close_action: CloseAction::Ask,
        }
    }
}

pub fn config_dir() -> PathBuf {
    // Overridable for testing / portable use.
    if let Some(dir) = std::env::var_os("ALWAYSENGLISH_CONFIG_DIR") {
        return PathBuf::from(dir);
    }
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    base.join("AlwaysEnglish")
}

pub fn config_path() -> PathBuf {
    config_dir().join("config.json")
}

/// Where the configuration came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Existing,
    MigratedFromQt,
    Fresh,
}

impl Config {
    pub fn load() -> (Config, Source) {
        let path = config_path();
        match fs::read_to_string(&path) {
            Ok(text) => match serde_json::from_str::<Config>(&text) {
                Ok(cfg) => (cfg, Source::Existing),
                Err(_) => {
                    // Keep the broken file for inspection instead of silently overwriting it.
                    let _ = fs::copy(&path, path.with_extension("json.broken"));
                    (Config::default(), Source::Fresh)
                }
            },
            Err(_) => match crate::legacy::migrate(&config_dir().join("AlwaysEnglish.ini")) {
                Some(cfg) => (cfg, Source::MigratedFromQt),
                None => (Config::default(), Source::Fresh),
            },
        }
    }

    /// Writes the configuration atomically (temp file + rename).
    pub fn save(&self) -> io::Result<()> {
        let dir = config_dir();
        fs::create_dir_all(&dir)?;
        let json = serde_json::to_string_pretty(self).map_err(io::Error::other)?;
        let tmp = dir.join("config.json.tmp");
        fs::write(&tmp, json)?;
        fs::rename(&tmp, config_path())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exe_key_is_lowercase_file_name() {
        assert_eq!(exe_key(r"C:\Program Files\Microsoft VS Code\Code.exe"), "code.exe");
        assert_eq!(exe_key("C:/Tools/a.EXE"), "a.exe");
    }

    #[test]
    fn config_roundtrip_and_defaults() {
        let cfg: Config = serde_json::from_str("{}").unwrap();
        assert!(cfg.resume_last_mode);
        assert_eq!(cfg.last_mode, Mode::Stopped);
        let mut cfg = Config::default();
        cfg.apps.push(AppEntry::new(r"C:\x\Game.exe".into()));
        let back: Config = serde_json::from_str(&serde_json::to_string(&cfg).unwrap()).unwrap();
        assert_eq!(back.apps, cfg.apps);
        assert_eq!(back.apps[0].name, "Game");
    }
}
