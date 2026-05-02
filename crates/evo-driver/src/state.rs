//! Snapshot of all readable EVO 8 device state.

/// Snapshot of all device controls readable via GET_CUR.
/// MU60 mixer state is NOT here — it's write-only and held in evo-config.
#[derive(Debug, Clone, Default)]
pub struct DeviceStatus {
    /// Output volume in dB for each pair (index 0 = main, 1 = headphone).
    pub volume_db: [f32; 2],

    /// Input gain in dB for inputs 1–4 (0-indexed).
    pub gain_db: [f32; 4],

    /// Phantom 48V power state for inputs 1–4 (0-indexed).
    pub phantom: [bool; 4],

    /// Input mute state for inputs 1–4 (0-indexed).
    pub input_mute: [bool; 4],

    /// Output mute (applies to all output pairs).
    pub output_mute: bool,
}
