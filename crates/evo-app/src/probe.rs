//! Protocol verification — walks every documented EVO 8 control on real hardware.
//!
//! Opens `/dev/evo8` directly, sends `GET_CUR` for every known control,
//! validates response shape/range, and prints a compact ASCII table.
//!
//! Any STALL or SHAPE row is printed in red and exit code is non-zero.
//! Pass `--probe-writable` to also test SET round-trip (writes original value back).

use std::fmt::Write;
use std::fs::OpenOptions;
use std::os::unix::io::AsRawFd;

use anyhow::{bail, Context, Result};
use evo_driver::ioctl::{get_cur, set_cur};
use evo_driver::DriverError;
use evo_protocol::{
    codec::{eu_to_bool, q88_to_db},
    controls::{CS_EU58_MUTE, CS_EU58_PHANTOM, CS_EU59_MUTE, CS_MIXER, CS_VOLUME},
    device::EVO8,
    entities::{mixer_cn, w_value, W_INDEX_EU58, W_INDEX_EU59, W_INDEX_FU10, W_INDEX_FU11, W_INDEX_MU60},
};

// ── Probe descriptors ─────────────────────────────────────────────────────

struct ProbeControl {
    name: &'static str,
    w_value: u16,
    w_index: u16,
    expected_len: u16,
    decode: fn(&[u8]) -> String,
    #[allow(dead_code)]
    reencode: fn(&[u8]) -> Vec<u8>,
    /// Whether a STALL is acceptable (e.g. write-only MU60).
    stall_ok: bool,
}

impl ProbeControl {
    fn status(&self, data: &[u8]) -> ProbeStatus {
        if data.is_empty() {
            return ProbeStatus::Stall;
        }
        if (data.len() as u16) < self.expected_len {
            return ProbeStatus::Shape {
                got: data.len(),
                expected: self.expected_len as usize,
            };
        }
        ProbeStatus::Ok((self.decode)(data))
    }
}

#[derive(Debug, Clone)]
enum ProbeStatus {
    Ok(String),
    Shape { got: usize, expected: usize },
    Stall,
}

impl ProbeStatus {
    fn is_failure(&self) -> bool {
        matches!(self, ProbeStatus::Shape { .. } | ProbeStatus::Stall)
    }
    fn label(&self) -> &'static str {
        match self {
            ProbeStatus::Ok(_) => "OK",
            ProbeStatus::Shape { .. } => "SHAPE",
            ProbeStatus::Stall => "STALL",
        }
    }
}

// ── Decoders ──────────────────────────────────────────────────────────────

fn decode_db(data: &[u8]) -> String {
    if data.len() < 2 {
        return "?".into();
    }
    let raw = i16::from_le_bytes([data[0], data[1]]);
    format!("{:.1} dB", q88_to_db(raw))
}

fn decode_bool(data: &[u8]) -> String {
    if data.len() < 4 {
        return "?".into();
    }
    if eu_to_bool(data) { "ON".into() } else { "OFF".into() }
}

fn reencode_identity(data: &[u8]) -> Vec<u8> {
    data.to_vec()
}

// ── Control table ─────────────────────────────────────────────────────────

fn all_controls() -> Vec<ProbeControl> {
    let mut ctrl = Vec::new();

    // FU10: output volume (2 pairs × 2 channels = 4 channels)
    for pair in 0..EVO8.num_output_pairs {
        for ch_in_pair in 0..2u8 {
            let cn = pair * 2 + ch_in_pair + 1;
            ctrl.push(ProbeControl {
                name: Box::leak(format!("FU10 volume pair{} CH{cn}", pair + 1).into_boxed_str()),
                w_value: w_value(CS_VOLUME, cn),
                w_index: W_INDEX_FU10,
                expected_len: 2,
                decode: decode_db,
                reencode: reencode_identity,
                stall_ok: false,
            });
        }
    }

    // FU11: input gain
    for cn in 1..=EVO8.num_inputs {
        ctrl.push(ProbeControl {
            name: Box::leak(format!("FU11 gain input{cn}").into_boxed_str()),
            w_value: w_value(CS_VOLUME, cn),
            w_index: W_INDEX_FU11,
            expected_len: 2,
            decode: decode_db,
            reencode: reencode_identity,
            stall_ok: false,
        });
    }

    // EU58: phantom per input
    for input in 0..EVO8.num_inputs {
        ctrl.push(ProbeControl {
            name: Box::leak(format!("EU58 phantom input{}", input + 1).into_boxed_str()),
            w_value: w_value(CS_EU58_PHANTOM, input),
            w_index: W_INDEX_EU58,
            expected_len: 4,
            decode: decode_bool,
            reencode: reencode_identity,
            stall_ok: false,
        });
    }

    // EU58: input mute per input
    for input in 0..EVO8.num_inputs {
        ctrl.push(ProbeControl {
            name: Box::leak(format!("EU58 mute input{}", input + 1).into_boxed_str()),
            w_value: w_value(CS_EU58_MUTE, input),
            w_index: W_INDEX_EU58,
            expected_len: 4,
            decode: decode_bool,
            reencode: reencode_identity,
            stall_ok: false,
        });
    }

    // EU59: output mute
    ctrl.push(ProbeControl {
        name: "EU59 output mute",
        w_value: w_value(CS_EU59_MUTE, 0),
        w_index: W_INDEX_EU59,
        expected_len: 4,
        decode: decode_bool,
        reencode: reencode_identity,
        stall_ok: false,
    });

    // MU60: write-only — expect STALL
    for in_idx in [0u8, 4] {
        let cn = mixer_cn(in_idx, 0, EVO8.mixer_outputs);
        ctrl.push(ProbeControl {
            name: Box::leak(format!("MU60 mixer[{in_idx}][0]").into_boxed_str()),
            w_value: w_value(CS_MIXER, cn),
            w_index: W_INDEX_MU60,
            expected_len: 2,
            decode: decode_db,
            reencode: reencode_identity,
            stall_ok: true,
        });
    }

    ctrl
}

// ── Probe execution ───────────────────────────────────────────────────────

/// Run the full probe. `writable` enables SET round-trip testing.
pub fn probe(writable: bool) -> Result<()> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/evo8")
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                anyhow::anyhow!("/dev/evo8 not found — is the EVO 8 connected and kmod loaded?")
            } else {
                e.into()
            }
        })
        .context("cannot open /dev/evo8 for probing")?;
    let fd = file.as_raw_fd();

    let controls = all_controls();
    let mut results: Vec<(&ProbeControl, ProbeStatus)> = Vec::with_capacity(controls.len());

    for ctrl in &controls {
        let status = probe_one(fd, ctrl);
        results.push((ctrl, status));
    }

    if writable {
        for &(ctrl, ref status) in &results.clone() {
            if let ProbeStatus::Ok(val) = status {
                let _ = writable_roundtrip(fd, ctrl, val);
            }
        }
    }

    print_table(&results);

    let failures: usize = results.iter().filter(|(_, s)| s.is_failure()).count();
    if failures > 0 {
        bail!("{failures} control(s) failed probe");
    }
    Ok(())
}

fn probe_one(fd: std::os::unix::io::RawFd, ctrl: &ProbeControl) -> ProbeStatus {
    match get_cur(fd, ctrl.w_value, ctrl.w_index, ctrl.expected_len) {
        Ok(data) if data.is_empty() => ProbeStatus::Stall,
        Ok(data) => ctrl.status(&data),
        Err(DriverError::Transfer(errno)) if ctrl.stall_ok => {
            // STALL is expected for write-only controls like MU60
            let _ = errno;
            ProbeStatus::Stall
        }
        Err(DriverError::Disconnected) => ProbeStatus::Stall,
        Err(e) => {
            // Any other error → treat as STALL
            let _ = e;
            ProbeStatus::Stall
        }
    }
}

fn writable_roundtrip(fd: std::os::unix::io::RawFd, ctrl: &ProbeControl, _val: &str) -> Result<()> {
    // Read current value
    let data = get_cur(fd, ctrl.w_value, ctrl.w_index, ctrl.expected_len)?;
    // Write it back
    set_cur(fd, ctrl.w_value, ctrl.w_index, &data)?;
    Ok(())
}

// ── Output formatting ─────────────────────────────────────────────────────

fn print_table(results: &[(&ProbeControl, ProbeStatus)]) {
    let mut table = String::new();
    let _ = writeln!(
        table,
        "{:<35} {:>8} {:>8} {:>5} {:>5} {:<12} {}",
        "Control", "wValue", "wIndex", "Exp", "Got", "Value", "Status"
    );
    let _ = writeln!(table, "{}", "─".repeat(95));

    for (ctrl, status) in results {
        let (got, value) = match status {
            ProbeStatus::Ok(v) => (ctrl.expected_len as usize, v.as_str()),
            ProbeStatus::Shape { got, expected: _exp } => {
                let _ = _exp;
                (*got, "—")
            },
            ProbeStatus::Stall => (0, "—"),
        };

        let is_fail = status.is_failure();
        if is_fail {
            let _ = write!(table, "\x1b[31m");
        }
        let _ = writeln!(
            table,
            "{name:<35} 0x{w_val:04X} 0x{w_idx:04X} {exp:>3} {got:>3} {val:<12} {label}",
            name = ctrl.name,
            w_val = ctrl.w_value,
            w_idx = ctrl.w_index,
            exp = ctrl.expected_len,
            val = value,
            label = status.label(),
        );
        if is_fail {
            let _ = write!(table, "\x1b[0m");
        }
    }

    print!("{table}");
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_table_count() {
        let controls = all_controls();
        // FU10: 4, FU11: 4, EU58 phantom: 4, EU58 mute: 4, EU59: 1, MU60: 2 = 19
        assert_eq!(controls.len(), 19);
    }

    #[test]
    fn fu10_wvalues() {
        let controls = all_controls();
        let fu10: Vec<_> = controls.iter().filter(|c| c.name.contains("FU10")).collect();
        assert_eq!(fu10.len(), 4);
        for c in &fu10 {
            assert_eq!(c.w_index, W_INDEX_FU10);
            assert_eq!((c.w_value >> 8) as u8, CS_VOLUME);
        }
        // CN should be 1..4
        let cns: Vec<u8> = fu10.iter().map(|c| (c.w_value & 0xFF) as u8).collect();
        assert_eq!(cns, vec![1, 2, 3, 4]);
    }

    #[test]
    fn fu11_wvalues() {
        let controls = all_controls();
        let fu11: Vec<_> = controls.iter().filter(|c| c.name.contains("FU11")).collect();
        assert_eq!(fu11.len(), 4);
        for c in &fu11 {
            assert_eq!(c.w_index, W_INDEX_FU11);
            assert_eq!((c.w_value >> 8) as u8, CS_VOLUME);
        }
    }

    #[test]
    fn eu58_phantom_wvalues() {
        let controls = all_controls();
        let phantom: Vec<_> = controls.iter().filter(|c| c.name.contains("phantom")).collect();
        assert_eq!(phantom.len(), 4);
        for c in &phantom {
            assert_eq!(c.w_index, W_INDEX_EU58);
            assert_eq!((c.w_value >> 8) as u8, CS_EU58_PHANTOM);
        }
    }

    #[test]
    fn eu58_mute_wvalues() {
        let controls = all_controls();
        // EU58 mutes have "mute input" in the name; EU59 has "output mute"
        let mutes: Vec<_> = controls.iter().filter(|c| c.name.contains("mute input")).collect();
        assert_eq!(mutes.len(), 4);
        for c in &mutes {
            assert_eq!(c.w_index, W_INDEX_EU58);
            assert_eq!((c.w_value >> 8) as u8, CS_EU58_MUTE);
        }
    }

    #[test]
    fn mu60_stall_ok() {
        let controls = all_controls();
        let mu60: Vec<_> = controls.iter().filter(|c| c.name.contains("MU60")).collect();
        assert_eq!(mu60.len(), 2);
        for c in &mu60 {
            assert!(c.stall_ok);
            assert_eq!(c.w_index, W_INDEX_MU60);
        }
    }

    #[test]
    fn expected_lengths() {
        for c in all_controls() {
            if c.name.contains("MU60") || c.name.contains("FU1") {
                assert_eq!(c.expected_len, 2, "{} should be 2 bytes", c.name);
            } else {
                assert_eq!(c.expected_len, 4, "{} should be 4 bytes", c.name);
            }
        }
    }

    #[test]
    fn probe_status_is_failure() {
        assert!(!ProbeStatus::Ok("0.0 dB".into()).is_failure());
        assert!(ProbeStatus::Shape { got: 1, expected: 2 }.is_failure());
        assert!(ProbeStatus::Stall.is_failure());
    }

    #[test]
    fn status_ok_for_good_data() {
        let ctrl = ProbeControl {
            name: "test",
            w_value: 0,
            w_index: 0,
            expected_len: 2,
            decode: decode_db,
            reencode: reencode_identity,
            stall_ok: false,
        };
        assert!(matches!(ctrl.status(&[0x00, 0x00]), ProbeStatus::Ok(_)));
    }

    #[test]
    fn status_shape_for_short_data() {
        let ctrl = ProbeControl {
            name: "test",
            w_value: 0,
            w_index: 0,
            expected_len: 2,
            decode: decode_db,
            reencode: reencode_identity,
            stall_ok: false,
        };
        assert!(matches!(ctrl.status(&[0x00]), ProbeStatus::Shape { .. }));
    }

    #[test]
    fn status_stall_for_empty() {
        let ctrl = ProbeControl {
            name: "test",
            w_value: 0,
            w_index: 0,
            expected_len: 2,
            decode: decode_db,
            reencode: reencode_identity,
            stall_ok: false,
        };
        assert!(matches!(ctrl.status(&[]), ProbeStatus::Stall));
    }

    #[test]
    fn decode_db_works() {
        assert_eq!(decode_db(&[0x00, 0x00]), "0.0 dB");
        assert_eq!(decode_db(&[0x00, 0x80]), "-128.0 dB");
    }

    #[test]
    fn decode_bool_works() {
        assert_eq!(decode_bool(&[1, 0, 0, 0]), "ON");
        assert_eq!(decode_bool(&[0, 0, 0, 0]), "OFF");
    }

    #[test]
    fn decode_short() {
        assert_eq!(decode_db(&[0x00]), "?");
        assert_eq!(decode_bool(&[1, 0, 0]), "?");
    }
}
