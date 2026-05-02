//! XDG-compliant paths for evo-control state and presets.

use std::path::PathBuf;

/// Root config directory: `~/.config/evo-control/`.
pub fn config_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("evo-control")
}

/// State file: `~/.config/evo-control/state.toml`.
pub fn state_file() -> PathBuf {
    config_dir().join("state.toml")
}

/// Presets directory: `~/.config/evo-control/presets/`.
pub fn presets_dir() -> PathBuf {
    config_dir().join("presets")
}

/// Path for a named preset: `~/.config/evo-control/presets/{name}.toml`.
pub fn preset_file(name: &str) -> PathBuf {
    presets_dir().join(format!("{name}.toml"))
}
