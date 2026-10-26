//! The user's settings, in `settings.json` in the app's config folder. The dev build and
//! the installed app share it (it follows the bundle identifier).

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Default, Debug)]
#[serde(default)]
pub struct Settings {
    /// User provider id -> the content hash (providers::content_hash) the user allowed.
    pub approved: BTreeMap<String, String>,
}

/// A missing file is the defaults. So is one that doesn't parse, so a damaged file
/// approves nothing rather than stopping the app.
pub fn load(path: &Path) -> Settings {
    match std::fs::read_to_string(path) {
        Ok(raw) => serde_json::from_str(&raw).unwrap_or_else(|e| {
            eprintln!("{}: {e}; using defaults", path.display());
            Settings::default()
        }),
        Err(_) => Settings::default(),
    }
}

/// Write via a temporary file and rename, so a crash never leaves half a file.
pub fn save(path: &Path, settings: &Settings) -> Result<(), String> {
    let err = |e: std::io::Error| format!("saving {}: {e}", path.display());
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(err)?;
    }
    let json = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json + "\n").map_err(err)?;
    std::fs::rename(&tmp, path).map_err(err)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_or_damaged_file_approves_nothing() {
        let dir = std::env::temp_dir().join(format!("ch-settings-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");
        assert!(load(&path).approved.is_empty());
        std::fs::write(&path, "{ not json").unwrap();
        assert!(load(&path).approved.is_empty());

        let mut s = Settings::default();
        s.approved.insert("demo".into(), "abc".into());
        save(&path, &s).unwrap();
        assert_eq!(load(&path).approved.get("demo").map(String::as_str), Some("abc"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
