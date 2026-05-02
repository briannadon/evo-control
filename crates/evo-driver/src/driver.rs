//! High-level EVO 8 driver: open /dev/evo8, execute typed control operations.

use std::fs::{File, OpenOptions};
use std::os::unix::io::AsRawFd;

use evo_protocol::{
    codec::{bool_to_eu, db_to_q88, eu_to_bool, q88_to_db},
    controls::{CS_EU58_MUTE, CS_EU58_PHANTOM, CS_EU59_MUTE, CS_MIXER, CS_VOLUME},
    device::{DeviceSpec, EVO8},
    entities::{mixer_cn, w_value, W_INDEX_EU58, W_INDEX_EU59, W_INDEX_FU10, W_INDEX_FU11, W_INDEX_MU60},
};

use crate::{
    error::DriverError,
    ioctl::{get_cur, set_cur},
    state::DeviceStatus,
};

const DEV_PATH: &str = "/dev/evo8";

pub struct Driver {
    file: File,
    pub spec: &'static DeviceSpec,
}

impl Driver {
    /// Open /dev/evo8. Returns KmodNotLoaded if the device file is absent.
    pub fn open() -> Result<Self, DriverError> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(DEV_PATH)
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound
                    || e.kind() == std::io::ErrorKind::PermissionDenied
                {
                    DriverError::KmodNotLoaded
                } else {
                    DriverError::Io(e)
                }
            })?;
        Ok(Self { file, spec: &EVO8 })
    }

    fn fd(&self) -> std::os::unix::io::RawFd {
        self.file.as_raw_fd()
    }

    // ── FU10: output volume ───────────────────────────────────────────────

    pub fn volume_get(&self, pair: u8) -> Result<f32, DriverError> {
        let cn = pair * 2 + 1;
        let data = get_cur(self.fd(), w_value(CS_VOLUME, cn), W_INDEX_FU10, 2)?;
        let raw = i16::from_le_bytes([data[0], data[1]]);
        Ok(q88_to_db(raw))
    }

    /// Set both channels of the output pair. Returns the clamped dB sent.
    pub fn volume_set(&self, pair: u8, db: f32) -> Result<f32, DriverError> {
        let db = self.spec.clamp_vol(db);
        let raw = db_to_q88(db).to_le_bytes();
        let base_cn = pair * 2 + 1;
        for cn in base_cn..=base_cn + 1 {
            set_cur(self.fd(), w_value(CS_VOLUME, cn), W_INDEX_FU10, &raw)?;
        }
        Ok(db)
    }

    // ── FU11: input gain ─────────────────────────────────────────────────

    pub fn gain_get(&self, input: u8) -> Result<f32, DriverError> {
        self.check_input(input)?;
        let data = get_cur(self.fd(), w_value(CS_VOLUME, input + 1), W_INDEX_FU11, 2)?;
        let raw = i16::from_le_bytes([data[0], data[1]]);
        Ok(q88_to_db(raw))
    }

    /// Returns the clamped + snapped dB sent.
    pub fn gain_set(&self, input: u8, db: f32) -> Result<f32, DriverError> {
        self.check_input(input)?;
        let db = self.spec.clamp_gain(db);
        let raw = db_to_q88(db).to_le_bytes();
        set_cur(self.fd(), w_value(CS_VOLUME, input + 1), W_INDEX_FU11, &raw)?;
        Ok(db)
    }

    // ── EU58: phantom power ───────────────────────────────────────────────

    pub fn phantom_get(&self, input: u8) -> Result<bool, DriverError> {
        self.check_input(input)?;
        let data = get_cur(self.fd(), w_value(CS_EU58_PHANTOM, input), W_INDEX_EU58, 4)?;
        Ok(eu_to_bool(&data))
    }

    pub fn phantom_set(&self, input: u8, on: bool) -> Result<(), DriverError> {
        self.check_input(input)?;
        set_cur(self.fd(), w_value(CS_EU58_PHANTOM, input), W_INDEX_EU58, &bool_to_eu(on))
    }

    // ── EU58: input mute ─────────────────────────────────────────────────

    pub fn input_mute_get(&self, input: u8) -> Result<bool, DriverError> {
        self.check_input(input)?;
        let data = get_cur(self.fd(), w_value(CS_EU58_MUTE, input), W_INDEX_EU58, 4)?;
        Ok(eu_to_bool(&data))
    }

    pub fn input_mute_set(&self, input: u8, muted: bool) -> Result<(), DriverError> {
        self.check_input(input)?;
        set_cur(self.fd(), w_value(CS_EU58_MUTE, input), W_INDEX_EU58, &bool_to_eu(muted))
    }

    // ── EU59: output mute ────────────────────────────────────────────────

    pub fn output_mute_get(&self) -> Result<bool, DriverError> {
        let data = get_cur(self.fd(), w_value(CS_EU59_MUTE, 0), W_INDEX_EU59, 4)?;
        Ok(eu_to_bool(&data))
    }

    pub fn output_mute_set(&self, muted: bool) -> Result<(), DriverError> {
        set_cur(self.fd(), w_value(CS_EU59_MUTE, 0), W_INDEX_EU59, &bool_to_eu(muted))
    }

    // ── MU60: loopback mixer (write-only) ────────────────────────────────

    /// Set a single cross-point. in_idx 0..9, out_idx 0..3.
    pub fn mixer_set(&self, in_idx: u8, out_idx: u8, db: f32) -> Result<(), DriverError> {
        if in_idx >= self.spec.mixer_inputs || out_idx >= self.spec.mixer_outputs {
            return Err(DriverError::ChannelIndex {
                idx: in_idx.max(out_idx),
                max: self.spec.mixer_inputs.max(self.spec.mixer_outputs) - 1,
            });
        }
        let db = self.spec.clamp_mixer(db);
        let cn = mixer_cn(in_idx, out_idx, self.spec.mixer_outputs);
        let raw = db_to_q88(db).to_le_bytes();
        set_cur(self.fd(), w_value(CS_MIXER, cn), W_INDEX_MU60, &raw)
    }

    // ── Full status snapshot ─────────────────────────────────────────────

    pub fn status(&self) -> Result<DeviceStatus, DriverError> {
        let mut s = DeviceStatus::default();
        for pair in 0..self.spec.num_output_pairs as u8 {
            s.volume_db[pair as usize] = self.volume_get(pair)?;
        }
        for input in 0..self.spec.num_inputs as u8 {
            s.gain_db[input as usize] = self.gain_get(input)?;
            s.phantom[input as usize] = self.phantom_get(input)?;
            s.input_mute[input as usize] = self.input_mute_get(input)?;
        }
        s.output_mute = self.output_mute_get()?;
        Ok(s)
    }

    // ─────────────────────────────────────────────────────────────────────

    fn check_input(&self, input: u8) -> Result<(), DriverError> {
        if input >= self.spec.num_inputs {
            Err(DriverError::ChannelIndex { idx: input, max: self.spec.num_inputs - 1 })
        } else {
            Ok(())
        }
    }
}
