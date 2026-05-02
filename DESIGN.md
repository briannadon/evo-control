# Design — evo-control

A Linux-native Rust GUI + CLI to control the Audient EVO 8 USB audio interface,
replacing the vendor's Windows/macOS-only Evo Control app.

## Goals

- **One-line install** of the user-facing binary: `cargo install evo-control`.
- **Pure-Rust app code.** No libusb. No system GUI lib deps beyond a windowing
  surface. The only C in the project is a vendored, ~180 LOC kernel module.
- **Coexists with `snd-usb-audio`.** Audio streaming is never disturbed by
  control operations.
- **Reversible.** `sudo evo-control uninstall-driver` cleanly removes everything
  we install at the system level.
- **Single binary, dual mode.** `evo-control` with no args launches the GUI;
  with subcommands it acts as a CLI.

## Non-goals (explicitly skipped)

- EVO 4 support. Code paths stripped. The `vanzaho/audient-evo-py` protocol
  abstractions are device-agnostic; we deliberately give that up to avoid
  shipping untested EVO 4 code.
- Smart Gain wizard.
- DFU firmware updates. The kmod will *never* issue DFU class requests; it
  binds the DFU interface only to obtain a `usb_device` handle.
- Multi-device simultaneous control.
- Sample-rate / clock-source UI (already exposed by ALSA / PipeWire).
- Front-panel button or encoder mirroring. The EVO 8 has no HID interface;
  buttons are MCU-internal and not USB-accessible. Confirmed by USB descriptor.
- Level meters in v1. Deferred to v1.1 via a PipeWire tap (see Future).

## Why a kernel module is required

The EVO 8's vendor controls (phantom, mute, loopback mixer, monitor mix) are
exposed as UAC2 class control transfers targeting **interface 0**, which
`snd-usb-audio` claims. Linux `usbfs` enforces interface ownership for
class-targeted control transfers from userspace, returning `EBUSY` even with a
secondary interface claimed and even running as root.

This was verified empirically with a `nusb` probe on this codebase's target
machine (CachyOS, kernel 7.0.1):

```
[3] Claiming interface 3 (DFU)...                                 OK
[4] Claiming interface 0 (audio control, snd-usb-audio territory) FAILED: EBUSY
[5] GET_CUR FU10 CH1 (output volume)                              FAILED
[6] GET_CUR EU58 input1 phantom                                   FAILED
[7] GET_CUR FU11 CH1 (input1 gain)                                FAILED
```

The kernel-internal `usb_control_msg()` does not run through `usbfs` and is
therefore not subject to the interface-ownership check. Vanzaho's `evo_raw`
kmod exploits exactly this: it binds to **interface 3 (DFU, unbound)** purely
to obtain a `usb_device` handle, then forwards a single ioctl to
`usb_control_msg()` on endpoint 0.

A small subset of controls — output volume (FU10) and input gain (FU11) —
*is* reachable purely via standard ALSA mixer controls
(`Mic Playback Volume`, `EVO8 Playback Volume`). These are exposed by
`snd-usb-audio` because they are standard UAC2 Feature Units. Everything else
requires the kmod path.

## Architecture

```
                    cargo install evo-control
                                |
                                v
                    +----------------------+
                    |     evo-control      |    Single binary
                    |  (CLI mode | GUI)    |    Workspace member: evo-app
                    +----------+-----------+
                               |
              +----------------+----------------+
              |                                 |
              v                                 v
     +----------------+                +-----------------+
     |  evo-config    |                |   evo-driver    |
     |  TOML presets  |                |  ioctl client   |
     |  state shadow  |                +--------+--------+
     +----------------+                         |
                                                | ioctl(EVO_CTRL_TRANSFER)
                                                v
                                       +-----------------+
                                       |  /dev/evo8      |    misc device
                                       |  (kmod)         |
                                       +--------+--------+
                                                |
                                                | usb_control_msg()
                                                | on endpoint 0
                                                v
                                       +-----------------+
                                       |   Audient EVO 8 |
                                       |  iface 0..2: snd-usb-audio (untouched)
                                       |  iface 3:   evo_raw (control only)
                                       +-----------------+

                    sudo evo-control install-driver
                                |
              +-----------------+--------------------+
              |          |                |          |
              v          v                v          v
      builds kmod   udev rule    wireplumber.conf   modprobe
      (DKMS)        99-evo.rules 50-evo-routing     evo_raw
```

## Repo layout

```
evo-control/
├── Cargo.toml                          # workspace
├── DESIGN.md                           # this file
├── PLAN.md                             # implementation plan (handoff to next session)
├── README.md                           # user-facing
├── LICENSE-MIT
├── LICENSE-APACHE
├── crates/
│   ├── evo-protocol/                   # pure types: entity IDs, control selectors, dB↔raw
│   │   ├── Cargo.toml
│   │   └── src/lib.rs
│   ├── evo-driver/                     # ioctl wrapper, hotplug, /dev/evo8
│   │   ├── Cargo.toml
│   │   └── src/lib.rs
│   ├── evo-config/                     # preset save/load, state shadow
│   │   ├── Cargo.toml
│   │   └── src/lib.rs
│   └── evo-app/                        # the binary: clap CLI + egui GUI
│       ├── Cargo.toml
│       └── src/
│           ├── main.rs
│           ├── cli.rs
│           ├── gui/
│           ├── install.rs              # install-driver / uninstall-driver
│           └── probe.rs                # protocol verification subcommand
├── kmod/
│   ├── evo_raw.c                       # vendored from vanzaho/audient-evo-py (GPL2+)
│   ├── Makefile
│   ├── dkms.conf
│   └── README.md                       # attribution + build instructions
└── packaging/
    ├── 99-evo.rules                    # udev: device perms + apply-preset trigger
    ├── evo-control.service             # systemd user unit (apply preset on connect)
    └── wireplumber/
        └── 50-evo-routing.conf         # WirePlumber: stereo-only, no idle suspend
```

## USB protocol summary

The EVO 8 protocol used by this project is **lifted from
[`vanzaho/audient-evo-py`'s `dev/DESIGN.md`](https://github.com/vanzaho/audient-evo-py/blob/master/dev/DESIGN.md)**.
That document is the source of truth for entity IDs, control selectors, byte
layouts, and quirk notes. Our `evo-protocol` crate encodes the same tables in
Rust. The key entities used in v1:

| Control | Entity | wValue | wIndex | Payload | Access |
|---|---|---|---|---|---|
| Output volume | FU10 | `(CS=2 << 8) \| CN` | `0x0A00` | i16 LE, 1/256 dB | R/W |
| Input gain | FU11 | `(CS=2 << 8) \| CN` | `0x0B00` | i16 LE, 1/256 dB | R/W |
| Phantom 48V | EU58 | `(CS=0 << 8) \| CN` | `0x3A00` | u32 LE bool | R/W |
| Input mute | EU58 | `(CS=2 << 8) \| CN` | `0x3A00` | u32 LE bool | R/W |
| Output mute | EU59 | `(CS=1 << 8) \| CN` | `0x3B00` | u32 LE bool | R/W |
| Mixer matrix | MU60 | `(CS=1 << 8) \| CN` | `0x3C00` | i16 LE, 1/256 dB | **W only** |

bRequestType = `0x21` (SET_CUR), `0xA1` (GET_CUR). bRequest = `0x01` (CUR).

**EVO 8 mixer matrix is 10×4 = 40 cross-points.** CN = `out_idx + in_idx * num_outputs`.
Inputs: 4 mic/line + 4 DAW + 2 LoopOut. Outputs: L1, R1, L2, R2 (two stereo
output pairs).

**Caveat.** Vanzaho's project marks the EVO 8 protocol as "needs hardware
testing" — they reverse-engineered it from packet captures and EVO 4 patterns
without a physical EVO 8. We are the EVO 8 hardware testers. The
`evo-control probe` subcommand exists specifically to walk every documented
control on connect and report shape/range mismatches before the GUI binds.

### Locally derivable, no kmod required

`snd-usb-audio` already exposes FU10 and FU11 as ALSA mixer controls
(`EVO8 Playback Volume`, `Mic Playback Volume`). The driver could fall back to
ALSA for those two controls, allowing the GUI to operate in degraded mode
(volume + gain only) when the kmod is not installed. **Optional in v1**;
include the path in `evo-driver` so the GUI can show "limited mode" rather
than refusing to start when `/dev/evo8` is missing.

## Threading model

- egui runs on the main thread.
- `evo-driver` does blocking ioctls; we wrap it in a worker thread with a
  request/response channel pattern.
- GUI tick (default 200 ms when window is focused, 0 otherwise) issues a
  `RefreshStatus` request on the channel; worker reads all controls, returns a
  `Status` snapshot, GUI updates state.
- User input (fader move, knob turn, toggle) issues a `Set` request, debounced
  50 ms per control. Worker applies, broadcasts the new value back.
- MU60 is **write-only**; mixer state is held entirely in `evo-config`'s
  shadow. On startup, the shadow is replayed to the device.

## Persistence

`~/.config/evo-control/`:
- `state.toml` — last-known device state, including the MU60 shadow. Auto-saved
  on change. Auto-applied on connect (via udev → systemd user unit).
- `presets/<name>.toml` — named user presets (save/load via CLI or GUI).

Schema is the same for both: a flat TOML representation of every control we
manage. Versioned via a `schema = 1` key for forward compatibility.

## Install / uninstall

`sudo evo-control install-driver` performs:

1. Verify kernel headers and `make` are present (CachyOS: `linux-headers`, `base-devel`).
2. Install `dkms` if missing (`pacman -S --needed dkms` with confirmation prompt).
3. Extract bundled `evo_raw.c` + Makefile + dkms.conf to `/usr/src/evo-raw-<version>/`.
4. `dkms add . && dkms install evo-raw/<version>`.
5. Install `/etc/udev/rules.d/99-evo.rules` (device permissions + preset trigger).
6. Install `/etc/systemd/user/evo-control-apply.service` (applies default preset on connect).
7. Install WirePlumber config to `~/.config/wireplumber/wireplumber.conf.d/50-evo-routing.conf`.
8. `udevadm control --reload && udevadm trigger`.
9. `modprobe evo_raw`.
10. Verify `/dev/evo8` exists and is accessible. Exit 0.

`sudo evo-control uninstall-driver` reverses each step.

The install command is idempotent: re-running upgrades the bundled module to
the binary's version.

## License

- Rust crates: **MIT OR Apache-2.0** (Rust ecosystem standard).
- `kmod/evo_raw.c`: **GPL-2.0-or-later** (Linux kernel module requirement;
  vanzaho's original is public-domain, so re-licensing as GPL-2.0+ is
  permitted and required for kernel module compatibility).
- All vendored code retains attribution: `kmod/evo_raw.c` carries an
  `AUTHORS:` block crediting `vanzaho/audient-evo-py` contributors.

## Future (v1.1+)

- **Level meters** via `pipewire` crate tap on the EVO 8 capture/playback
  streams. Adds `libpipewire-0.3` system dep but already present on every
  PipeWire system.
- **Loopback mixer matrix view** in addition to the channel-strip primary view.
- **EVO 4 support** if a contributor with hardware steps up.
- **Smart Gain wizard** (PipeWire capture + RMS analysis loop + iterative gain set).
- **Upstream snd-usb-audio quirk patch** to expose phantom/mute/mixer as native
  ALSA controls, eventually obsoleting our kmod. Out of scope for this project
  but the long-term right answer for the Linux ecosystem.

## References

- [`vanzaho/audient-evo-py`](https://github.com/vanzaho/audient-evo-py) — Python
  predecessor, source of protocol RE.
- [`vanzaho/audient-evo-py/dev/DESIGN.md`](https://github.com/vanzaho/audient-evo-py/blob/master/dev/DESIGN.md) — full protocol documentation.
- [`vanzaho/audient-evo-py/dev/EVO8-TESTING.md`](https://github.com/vanzaho/audient-evo-py/blob/master/dev/EVO8-TESTING.md) — known EVO 8 protocol gaps.
- [`nusb` crate](https://docs.rs/nusb/) — used for the userspace probe; not in
  the shipping app.
