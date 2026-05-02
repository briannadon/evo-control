//! dB ↔ UAC2 Q8.8 (1/256 dB steps) conversion.
//!
//! UAC2 represents levels as a 16-bit signed integer where the unit is 1/256 dB.
//! E.g. -20.0 dB → raw = -20 * 256 = -5120 = 0xEC00.
//! Device values are little-endian on the wire.

/// Convert dB to UAC2 16-bit signed (Q8.8, 1/256 dB steps).
///
/// Values are NOT clamped here — callers should clamp to their entity's range
/// before calling (see `DeviceSpec`).
pub fn db_to_q88(db: f32) -> i16 {
    (db * 256.0).round() as i16
}

/// Convert UAC2 16-bit signed (Q8.8) back to dB.
pub fn q88_to_db(raw: i16) -> f32 {
    raw as f32 / 256.0
}

/// Encode a 4-byte boolean payload (phantom / mute controls).
///
/// EU58 and EU59 require full 4 bytes; short writes are silently ignored.
pub fn bool_to_eu(value: bool) -> [u8; 4] {
    (if value { 1u32 } else { 0u32 }).to_le_bytes()
}

/// Decode a 4-byte boolean payload.
pub fn eu_to_bool(bytes: &[u8]) -> bool {
    bytes.get(..4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]])) == Some(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        for db in [-128.0_f32, -96.0, -20.0, -6.0, -3.0, 0.0, 6.0] {
            let recovered = q88_to_db(db_to_q88(db));
            assert!(
                (recovered - db).abs() < 0.005,
                "round-trip failed for {db}: got {recovered}"
            );
        }
    }

    #[test]
    fn known_values() {
        assert_eq!(db_to_q88(0.0), 0x0000);
        assert_eq!(db_to_q88(-128.0), i16::MIN); // 0x8000 as i16
        assert_eq!(db_to_q88(6.0), 0x0600);
        assert_eq!(db_to_q88(-8.0), -2048);
    }

    #[test]
    fn bool_payload() {
        assert_eq!(bool_to_eu(true), [1, 0, 0, 0]);
        assert_eq!(bool_to_eu(false), [0, 0, 0, 0]);
        assert!(eu_to_bool(&[1, 0, 0, 0]));
        assert!(!eu_to_bool(&[0, 0, 0, 0]));
    }
}
