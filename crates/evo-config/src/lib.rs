//! EVO 8 config: TOML presets and write-only state shadow.
//!
//! # Overview
//!
//! - `DeviceState`: serializable snapshot of every v1 control.
//! - `paths`: XDG-compliant paths for state and preset files.
//! - `store`: atomic read/write with schema version checks.
//! - `Config`: in-memory handle that lazily loads state and presets.

pub mod paths;
pub mod state;
pub mod store;

use std::collections::HashMap;

pub use paths::{config_dir, preset_file, presets_dir, state_file};
pub use state::DeviceState;
pub use store::{delete_preset, list_presets, load_state, save_state, LoadError};

/// In-memory config handle.
///
/// Loaded once at startup and written back on every change.
/// `DeviceState` is the active state; `presets` is the full set of named presets
/// scanned from the presets directory.
pub struct Config {
    pub state: DeviceState,
    pub presets: HashMap<String, DeviceState>,
    state_path: std::path::PathBuf,
}

impl Config {
    /// Load or create config from the default XDG paths.
    ///
    /// If `state.toml` doesn't exist, a default `DeviceState` is used
    /// (hardware defaults). The presets list is always rescanned from disk.
    pub fn load() -> Result<Self, LoadError> {
        let state_path = state_file();
        let state = load_state(&state_path)?.unwrap_or_default();
        let presets = Self::scan_presets(&presets_dir())?;

        Ok(Self { state, presets, state_path })
    }

    /// Persist the current state to disk atomically.
    pub fn save_state(&self) -> std::io::Result<()> {
        // Ensure config dir exists.
        if let Some(parent) = self.state_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        save_state(&self.state_path, &self.state)
    }

    /// Persist a named preset to disk.
    pub fn save_preset(&mut self, name: &str) -> std::io::Result<()> {
        let file = preset_file(name);
        if let Some(parent) = file.parent() {
            std::fs::create_dir_all(parent)?;
        }
        save_state(&file, &self.state)?;
        self.presets.insert(name.to_owned(), self.state.clone());
        Ok(())
    }

    /// Load a named preset into the active state and persist it.
    pub fn load_preset(&mut self, name: &str) -> Result<(), LoadPresetError> {
        let file = preset_file(name);
        let preset = load_state(&file)?
            .ok_or_else(|| LoadPresetError::NotFound(name.to_owned()))?;
        self.state = preset;
        self.save_state()?;
        Ok(())
    }

    /// Delete a named preset from disk and the in-memory map.
    pub fn delete_preset(&mut self, name: &str) -> std::io::Result<()> {
        let file = preset_file(name);
        delete_preset(&file)?;
        self.presets.remove(name);
        Ok(())
    }

    /// Rescan presets directory, returning a sorted name list.
    pub fn list_presets(&self) -> Vec<String> {
        let mut names: Vec<String> = self.presets.keys().cloned().collect();
        names.sort();
        names
    }

    // ── helpers ──────────────────────────────────────────────────────────

    fn scan_presets(dir: &std::path::Path) -> Result<HashMap<String, DeviceState>, LoadError> {
        let names = list_presets(dir)?;
        let mut presets = HashMap::with_capacity(names.len());
        for name in &names {
            let file = dir.join(format!("{name}.toml"));
            if let Some(state) = load_state(&file)? {
                presets.insert(name.clone(), state);
            }
        }
        Ok(presets)
    }
}

// ── Errors ────────────────────────────────────────────────────────────────

#[derive(Debug, thiserror::Error)]
pub enum LoadPresetError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("parse error: {0}")]
    Parse(#[from] toml::de::Error),

    #[error("preset \"{0}\" not found")]
    NotFound(String),
}

impl From<LoadError> for LoadPresetError {
    fn from(e: LoadError) -> Self {
        match e {
            LoadError::Io(e) => Self::Io(e),
            LoadError::Utf8(e) => Self::Io(std::io::Error::new(std::io::ErrorKind::InvalidData, e)),
            LoadError::Parse(e) => Self::Parse(e),
            LoadError::SchemaVersion(v) => Self::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("unsupported schema version {v}"),
            )),
        }
    }
}
