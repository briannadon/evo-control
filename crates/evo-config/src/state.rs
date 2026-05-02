//! Serializable snapshot of all EVO 8 control state.
//!
//! Written to `~/.config/evo-control/state.toml` on every change, and used
//! as the persistence/cache for the write-only MU60 mixer matrix.

use serde::{Deserialize, Serialize};

/// Complete device state, serializable to TOML.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceState {
    /// Schema version for forward-compat. Current: 1.
    pub schema: u32,

    /// Output volume in dB for each pair (index 0 = main, 1 = headphone).
    pub output_volume_db: [f32; 2],

    /// Input gain in dB for inputs 1–4 (0-indexed).
    pub input_gain_db: [f32; 4],

    /// Phantom 48V power state for inputs 1–4 (0-indexed).
    pub phantom: [bool; 4],

    /// Input mute for inputs 1–4 (0-indexed).
    pub input_mute: [bool; 4],

    /// Output mute (applies to both pairs).
    pub output_mute: bool,

    /// Write-only MU60 loopback mixer matrix: 10 inputs × 4 outputs.
    /// Stored as [input][output] in dB.
    pub mixer: [[f32; 4]; 10],
}

impl Default for DeviceState {
    fn default() -> Self {
        // Hardware default: each mic/line input routed to its loopback pair.
        // 10 inputs × 4 outputs (L1, R1, L2, R2). First 4 inputs get unity
        // to their pair's outputs; everything else is -128 dB (silence).
        let mut mixer = [[-128.0_f32; 4]; 10];
        for i in 0..4 {
            let out_base = (i % 2) * 2;
            mixer[i][out_base] = 0.0;
            mixer[i][out_base + 1] = 0.0;
        }
        Self {
            schema: 1,
            output_volume_db: [0.0; 2],
            input_gain_db: [0.0; 4],
            phantom: [false; 4],
            input_mute: [false; 4],
            output_mute: false,
            mixer,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_toml() {
        let state = DeviceState::default();
        let toml_str = toml::to_string(&state).unwrap();
        let decoded: DeviceState = toml::from_str(&toml_str).unwrap();
        assert_eq!(decoded.schema, 1);
        assert_eq!(decoded.output_volume_db, [0.0; 2]);
        // Default mixer: channels 0..3 have unity diagonal
        assert_eq!(decoded.mixer[0][0], 0.0);  // input 0 → L1
        assert_eq!(decoded.mixer[0][1], 0.0);  // input 0 → R1
        assert_eq!(decoded.mixer[1][2], 0.0);  // input 1 → L2
        assert_eq!(decoded.mixer[1][3], 0.0);  // input 1 → R2
        assert_eq!(decoded.mixer[2][0], 0.0);  // input 2 → L1
        assert_eq!(decoded.mixer[3][2], 0.0);  // input 3 → L2
        // Non-mic inputs are silence
        assert_eq!(decoded.mixer[4][0], -128.0);
        assert_eq!(decoded.mixer[9][3], -128.0);
    }

    #[test]
    fn schema_version_check() {
        // A full state with unknown schema version is an error.
        let mut state = DeviceState::default();
        state.schema = 99;
        let toml_str = toml::to_string(&state).unwrap();
        // The store's load_state wrapper checks schema version.
        let path = std::env::temp_dir().join("evo-schema-test.toml");
        std::fs::write(&path, &toml_str).unwrap();
        let err = crate::store::load_state(&path).unwrap_err();
        assert!(matches!(err, crate::store::LoadError::SchemaVersion(99)));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn missing_field_fails() {
        let partial = r#"schema = 1"#;
        assert!(toml::from_str::<DeviceState>(partial).is_err());
    }
}
