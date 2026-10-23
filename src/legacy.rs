//! Migration of the settings written by the original Qt version
//! (`QSettings` INI file, `[General]` section).
//!
//! Relevant keys: `darkMode` (0 system / 1 light / 2 dark), `capLock`, `autoStart`,
//! `exeInfos` (JSON object `{ path: { name, icon, isTurnOn, isCapLock } }`) and
//! `existingFilePath` (JSON array of paths).

use std::collections::HashMap;
use std::path::Path;

use crate::config::{AppEntry, Config, Theme};

pub fn migrate(ini: &Path) -> Option<Config> {
    let bytes = std::fs::read(ini).ok()?;
    let text = String::from_utf8_lossy(&bytes);
    let values = parse_general(&text);
    if values.is_empty() {
        return None;
    }

    let mut cfg = Config {
        theme: match values.get("darkMode").map(|s| s.trim()) {
            Some("1") => Theme::Light,
            Some("2") => Theme::Dark,
            _ => Theme::System,
        },
        always_caps_lock: values.get("capLock").is_some_and(|v| is_true(v)),
        auto_start: values.get("autoStart").is_some_and(|v| is_true(v)),
        ..Config::default()
    };

    let mut seen = std::collections::HashSet::new();
    if let Some(infos) = values.get("exeInfos")
        && let Ok(serde_json::Value::Object(map)) = serde_json::from_str::<serde_json::Value>(infos)
    {
        for (path, info) in map {
            let path = normalize(&path);
            if path.is_empty() || !seen.insert(path.to_lowercase()) {
                continue;
            }
            let mut entry = AppEntry::new(path);
            if let Some(name) = info.get("name").and_then(|v| v.as_str())
                && !name.is_empty()
            {
                entry.name = name.to_owned();
            }
            entry.enabled = info.get("isTurnOn").and_then(|v| v.as_bool()).unwrap_or(true);
            // The Qt version defaulted Caps Lock to on for new apps.
            entry.caps_lock = info.get("isCapLock").and_then(|v| v.as_bool()).unwrap_or(true);
            cfg.apps.push(entry);
        }
    }
    if let Some(list) = values.get("existingFilePath")
        && let Ok(serde_json::Value::Array(items)) = serde_json::from_str::<serde_json::Value>(list)
    {
        for item in items.iter().filter_map(|v| v.as_str()) {
            let path = normalize(item);
            if !path.is_empty() && seen.insert(path.to_lowercase()) {
                let mut entry = AppEntry::new(path);
                entry.caps_lock = true;
                cfg.apps.push(entry);
            }
        }
    }
    Some(cfg)
}

fn is_true(v: &str) -> bool {
    matches!(v.trim().to_ascii_lowercase().as_str(), "true" | "1")
}

fn normalize(path: &str) -> String {
    let p = path.trim();
    let p = p.strip_prefix("file:///").unwrap_or(p);
    p.replace('/', "\\")
}

/// Returns the key/value pairs of the `[General]` section (or of keys before any section).
fn parse_general(text: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let mut in_general = true;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            in_general = line[1..line.len() - 1].eq_ignore_ascii_case("General");
            continue;
        }
        if !in_general {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            out.insert(k.trim().to_owned(), unescape_value(v.trim()));
        }
    }
    out
}

/// Undoes the escaping `QSettings` applies to INI string values:
/// surrounding / embedded double quotes, C-style escapes and `\x` hex escapes.
fn unescape_value(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' => {}
            '\\' => match chars.next() {
                Some('a') => out.push('\u{7}'),
                Some('b') => out.push('\u{8}'),
                Some('f') => out.push('\u{c}'),
                Some('n') => out.push('\n'),
                Some('r') => out.push('\r'),
                Some('t') => out.push('\t'),
                Some('v') => out.push('\u{b}'),
                Some('x') => {
                    let mut value: u32 = 0;
                    while let Some(&h) = chars.peek()
                        && let Some(d) = h.to_digit(16)
                    {
                        value = value.wrapping_mul(16).wrapping_add(d);
                        chars.next();
                    }
                    if let Some(ch) = char::from_u32(value) {
                        out.push(ch);
                    }
                }
                Some(d @ '0'..='7') => {
                    let mut value = d.to_digit(8).unwrap();
                    while let Some(&h) = chars.peek()
                        && let Some(d) = h.to_digit(8)
                    {
                        value = value * 8 + d;
                        chars.next();
                    }
                    if let Some(ch) = char::from_u32(value) {
                        out.push(ch);
                    }
                }
                Some(other) => out.push(other),
                None => {}
            },
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unescapes_qsettings_strings() {
        assert_eq!(unescape_value(r#""{\"a\":1,\"b\":true}""#), r#"{"a":1,"b":true}"#);
        assert_eq!(unescape_value(r"\x5fae\x4fe1"), "微信");
        assert_eq!(unescape_value("plain"), "plain");
    }

    #[test]
    fn migrates_qt_ini() {
        let dir = std::env::temp_dir().join(format!("ae-legacy-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let ini = dir.join("AlwaysEnglish.ini");
        std::fs::write(
            &ini,
            concat!(
                "[General]\n",
                "darkMode=2\n",
                "capLock=true\n",
                "autoStart=false\n",
                r#"exeInfos="{\"C:/Program Files/Game/game.exe\":{\"name\":\"game\",\"icon\":\"data:image/png;base64,AAAA\",\"isTurnOn\":false,\"isCapLock\":true}}""#,
                "\n",
                r#"existingFilePath="[\"C:/Program Files/Game/game.exe\",\"D:/Tools/\x5de5\x5177.exe\"]""#,
                "\n"
            ),
        )
        .unwrap();
        let cfg = migrate(&ini).unwrap();
        assert_eq!(cfg.theme, Theme::Dark);
        assert!(cfg.always_caps_lock);
        assert_eq!(cfg.apps.len(), 2);
        assert_eq!(cfg.apps[0].path, r"C:\Program Files\Game\game.exe");
        assert!(!cfg.apps[0].enabled);
        assert!(cfg.apps[0].caps_lock);
        assert_eq!(cfg.apps[1].path, r"D:\Tools\工具.exe");
        assert_eq!(cfg.apps[1].name, "工具");
        let _ = std::fs::remove_dir_all(dir);
    }
}
