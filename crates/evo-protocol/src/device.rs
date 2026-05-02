//! EVO 8 device specification constants.

/// Static description of the EVO 8's channel layout and dB ranges.
#[derive(Debug, Clone, Copy)]
pub struct DeviceSpec {
    pub vid: u16,
    pub pid: u16,
    pub display_name: &'static str,

    /// Number of physical input channels (XLR/TRS combo jacks).
    pub num_inputs: u8,

    /// Number of stereo output pairs (main + headphone on EVO 8).
    pub num_output_pairs: u8,

    /// Mixer matrix input count (mic/line + DAW + LoopOut streams).
    pub mixer_inputs: u8,

    /// Mixer matrix output count (stereo pairs into the loopback bus).
    pub mixer_outputs: u8,

    /// Output volume range, dB. Hardware floor is -127; software limit is -96.
    pub vol_db_min: f32,
    pub vol_db_max: f32,

    /// Input gain range, dB.
    pub gain_db_min: f32,
    pub gain_db_max: f32,

    /// Device quantizes gain writes to this step size.
    pub gain_db_step: f32,

    /// Mixer cross-point range, dB. 0x8000 (i16::MIN) = silence.
    pub mixer_db_min: f32,
    pub mixer_db_max: f32,
}

impl DeviceSpec {
    /// Clamp dB to the output volume range.
    pub fn clamp_vol(&self, db: f32) -> f32 {
        db.clamp(self.vol_db_min, self.vol_db_max)
    }

    /// Clamp and snap dB to the nearest gain step.
    pub fn clamp_gain(&self, db: f32) -> f32 {
        let clamped = db.clamp(self.gain_db_min, self.gain_db_max);
        (clamped / self.gain_db_step).round() * self.gain_db_step
    }

    /// Clamp dB to the mixer cross-point range.
    pub fn clamp_mixer(&self, db: f32) -> f32 {
        db.clamp(self.mixer_db_min, self.mixer_db_max)
    }

    /// Total mixer cross-points (in × out).
    pub fn mixer_crosspoints(&self) -> u8 {
        self.mixer_inputs * self.mixer_outputs
    }
}

/// The Audient EVO 8.
///
/// 4 combo XLR/TRS inputs, 2 stereo output pairs (main + headphone),
/// 10×4 loopback mixer matrix.
pub const EVO8: DeviceSpec = DeviceSpec {
    vid: 0x2708,
    pid: 0x0007,
    display_name: "Audient EVO 8",

    num_inputs: 4,
    num_output_pairs: 2,

    // Mixer: 4 mic/line + 4 DAW + 2 LoopOut inputs; 4 loopback outputs (L1,R1,L2,R2)
    mixer_inputs: 10,
    mixer_outputs: 4,

    vol_db_min: -96.0, // software floor; hardware descriptor reports -127
    vol_db_max: 0.0,

    gain_db_min: -8.0,
    gain_db_max: 50.0,
    gain_db_step: 1.0,

    mixer_db_min: -128.0, // i16::MIN / 256 — silence
    mixer_db_max: 6.0,    // Windows app software limit
};
