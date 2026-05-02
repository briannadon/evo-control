//! Atomic read/write for TOML state and preset files.
//!
//! Writes go to a temp file first, then atomically renamed to the target.
//! Loads verify the schema version.

use std::fs;
use std::io;
use std::path::Path;

use crate::state::DeviceState;

/// Schema version this crate knows how to read.
const SCHEMA: u32 = 1;

/// Load device state from a TOML file. Returns `None` if the file is absent.
pub fn load_state(path: &Path) -> Result<Option<DeviceState>, LoadError> {
    let bytes = match fs::read(path) {
        Ok(b) => b,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(LoadError::Io(e)),
    };
    let s = std::str::from_utf8(&bytes).map_err(|e| LoadError::Utf8(e))?;
    let state: DeviceState = toml::from_str(s)?;
    if state.schema > SCHEMA {
        return Err(LoadError::SchemaVersion(state.schema));
    }
    Ok(Some(state))
}

/// Atomically write device state to a TOML file.
///
/// Writes to a `.tmp` sibling, then renames to `path`. This is atomic on
/// POSIX filesystems (rename is atomic within the same filesystem).
pub fn save_state(path: &Path, state: &DeviceState) -> io::Result<()> {
    let toml_str = toml::to_string(state).expect("DeviceState is always serializable");
    let tmp = path.with_extension("toml.tmp");
    fs::write(&tmp, &toml_str)?;
    fs::rename(&tmp, path)?;
    Ok(())
}

/// List all preset names in `presets_dir` (files matching `*.toml`).
pub fn list_presets(dir: &Path) -> io::Result<Vec<String>> {
    let mut names = Vec::new();
    if !dir.exists() {
        return Ok(names);
    }
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        if entry.file_type()?.is_file() {
            if let Some(name) = entry
                .file_name()
                .to_str()
                .and_then(|s| s.strip_suffix(".toml"))
            {
                names.push(name.to_owned());
            }
        }
    }
    names.sort();
    Ok(names)
}

/// Delete a named preset file.
pub fn delete_preset(file: &Path) -> io::Result<()> {
    if file.exists() {
        fs::remove_file(file)
    } else {
        Ok(())
    }
}

// ── Errors ────────────────────────────────────────────────────────────────

#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    #[error("invalid UTF-8 in state file: {0}")]
    Utf8(#[from] std::str::Utf8Error),

    #[error("TOML parse error: {0}")]
    Parse(#[from] toml::de::Error),

    #[error("unsupported schema version {0}; this version supports up to v{SCHEMA}")]
    SchemaVersion(u32),
}

impl From<LoadError> for std::io::Error {
    fn from(e: LoadError) -> Self {
        std::io::Error::new(std::io::ErrorKind::Other, e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::DeviceState;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU16, Ordering};

    fn tmp_path() -> PathBuf {
        static COUNTER: AtomicU16 = AtomicU16::new(0);
        let c = COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!("evo-config-test-{c}.toml"))
    }

    #[test]
    fn round_trip_state() {
        let path = tmp_path();
        let state = DeviceState::default();

        save_state(&path, &state).unwrap();
        let loaded = load_state(&path).unwrap().unwrap();
        assert_eq!(loaded.schema, 1);
        assert_eq!(loaded.output_volume_db, [0.0; 2]);
        assert_eq!(loaded.phantom, [false; 4]);

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn missing_file_returns_none() {
        let path = tmp_path();
        let loaded = load_state(&path).unwrap();
        assert!(loaded.is_none());
    }

    #[test]
    fn atomic_write_does_not_corrupt() {
        let path = tmp_path();
        let state = DeviceState::default();

        // First write
        save_state(&path, &state).unwrap();

        // Simulate a partial write by writing garbage to a temp name
        let tmp = path.with_extension("toml.tmp");
        fs::write(&tmp, b"not valid toml").unwrap();

        // Real state should be intact (the garbage is in .tmp, not the real file)
        let loaded = load_state(&path).unwrap().unwrap();
        assert_eq!(loaded.schema, 1);

        let _ = fs::remove_file(&path);
        let _ = fs::remove_file(&tmp);
    }

    #[test]
    fn schema_rejected() {
        let path = tmp_path();
        // Write a complete state with schema 99
        let mut bad_state = DeviceState::default();
        bad_state.schema = 99;
        let bad = toml::to_string(&bad_state).unwrap();
        fs::write(&path, bad).unwrap();
        let err = load_state(&path).unwrap_err();
        assert!(matches!(err, LoadError::SchemaVersion(99)));
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn list_and_delete_presets() {
        let dir = std::env::temp_dir().join("evo-preset-test");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        // No presets yet
        assert!(list_presets(&dir).unwrap().is_empty());

        // Save two presets
        let state = DeviceState::default();
        save_state(&dir.join("studio.toml"), &state).unwrap();
        save_state(&dir.join("live.toml"), &state).unwrap();

        let names = list_presets(&dir).unwrap();
        assert_eq!(names, vec!["live", "studio"]);

        // Delete one
        delete_preset(&dir.join("live.toml")).unwrap();
        let names = list_presets(&dir).unwrap();
        assert_eq!(names, vec!["studio"]);

        let _ = fs::remove_dir_all(&dir);
    }
}
