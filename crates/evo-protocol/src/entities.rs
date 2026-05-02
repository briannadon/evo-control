//! UAC2 entity IDs and their wIndex values: (EntityID << 8) | Interface(0).
//!
//! Source: vanzaho/audient-evo-py dev/DESIGN.md, cross-referenced with the
//! EVO 8 USB descriptor (lsusb -v -d 2708:0007).

/// Feature Unit 10 — output volume. R/W. i16 LE 1/256 dB per channel.
/// CN=1..4 for two output pairs (pair_idx*2+1, pair_idx*2+2).
pub const W_INDEX_FU10: u16 = 0x0A00;

/// Feature Unit 11 — input gain. R/W. i16 LE 1/256 dB per channel.
/// CN=1..4 for inputs 1–4. Device quantizes writes to 1 dB steps.
pub const W_INDEX_FU11: u16 = 0x0B00;

/// Extension Unit 58 — input config (phantom power + mute). R/W. u32 LE bool.
/// CS=0, CN=0/1/2/3 → phantom 48V for inputs 1–4.
/// CS=2, CN=0/1/2/3 → input mute for inputs 1–4.
/// Must send full 4 bytes; device ignores short writes.
pub const W_INDEX_EU58: u16 = 0x3A00;

/// Extension Unit 59 — output config (mute). R/W. u32 LE bool.
/// CS=1, CN=0 → output mute (both output pairs).
/// Must send full 4 bytes.
pub const W_INDEX_EU59: u16 = 0x3B00;

/// Mixer Unit 60 — loopback mixer matrix. **Write-only** (GET_CUR STALLs).
/// i16 LE 1/256 dB. CS=1. CN = out_idx + in_idx * NUM_MIXER_OUTPUTS.
/// EVO 8: 10 inputs × 4 outputs = 40 cross-points.
pub const W_INDEX_MU60: u16 = 0x3C00;

/// Build wValue from control selector and channel number.
#[inline]
pub const fn w_value(cs: u8, cn: u8) -> u16 {
    ((cs as u16) << 8) | (cn as u16)
}

/// MU60 cross-point CN for (in_idx, out_idx).
/// EVO 8: num_outputs = 4. Range: in_idx 0..9, out_idx 0..3.
#[inline]
pub const fn mixer_cn(in_idx: u8, out_idx: u8, num_outputs: u8) -> u8 {
    out_idx + in_idx * num_outputs
}
