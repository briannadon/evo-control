# Implementation plan — evo-control v1

Handoff document for the implementation session. Read `DESIGN.md` first for
architecture, decisions, and rationale. This file is the ordered build plan.

Each step is independently committable. After each step, the repo should
build (`cargo check --workspace`) and the steps so far should be reproducible
from a clean clone.

## Prerequisites

- Working directory: `/home/bdn/repos/evo-control` (already a git repo on `master`)
- Audient EVO 8 plugged in (USB ID `2708:0007`)
- Rust toolchain installed at `~/.cargo/bin/` (cargo 1.95.0+)
- CachyOS with `linux-cachyos-headers`, `base-devel` (already present)
- `dkms` is **not** present yet — install step must handle that

## Reference material

- `vanzaho/audient-evo-py` master branch — the protocol RE source. Especially:
  - `dev/DESIGN.md` (protocol tables)
  - `kmod/evo_raw.c` (kernel module, ~180 LOC; vendor verbatim with attribution)
  - `kmod/Makefile`, `kmod/dkms.conf`, `kmod/install.sh` (install patterns to mirror)
  - `evo/devices.py` (DeviceSpec for both EVO 4/8; we keep only EVO 8)
  - `evo/controller.py` (set_volume/get_volume/etc — algorithmic reference)
  - `wireplumber/` (the routing fix)

Clone reference into a sibling dir if convenient: `git clone https://github.com/vanzaho/audient-evo-py /tmp/vanzaho-ref`. **Do not vendor anything beyond `kmod/evo_raw.c` + its Makefile + dkms.conf + the WirePlumber config.** Everything else we re-derive in Rust.

---

## Step 1 — Bootstrap workspace + license + README

**Goal:** Empty but buildable workspace with all four crate skeletons and license files.

**Files to create:**
- `Cargo.toml` (workspace root, lists members)
- `LICENSE-MIT`, `LICENSE-APACHE` (standard texts; Rust ecosystem boilerplate)
- `README.md` (short — point at DESIGN.md and PLAN.md, name the goal, link upstream)
- `crates/evo-protocol/{Cargo.toml, src/lib.rs}` — empty `lib.rs`
- `crates/evo-driver/{Cargo.toml, src/lib.rs}` — empty
- `crates/evo-config/{Cargo.toml, src/lib.rs}` — empty
- `crates/evo-app/{Cargo.toml, src/main.rs}` — `fn main() { println!("evo-control v0.1.0") }`
- `.gitignore` (Cargo standard: `target/`, `.cargo/config.local.toml`, etc.)
- `rust-toolchain.toml` — pin to `stable` channel (no specific version)

**Workspace `Cargo.toml`:**
- `[workspace] members = ["crates/*"]`
- `[workspace.package]` with version = "0.1.0", edition = "2024", license = "MIT OR Apache-2.0", authors, repository
- `[workspace.dependencies]` for shared deps (anyhow, thiserror, serde, toml, clap, eframe/egui, etc.) — see Step 6 for the egui pin

**Acceptance:**
- `~/.cargo/bin/cargo check --workspace` succeeds.
- `~/.cargo/bin/cargo build --workspace` succeeds.
- `target/debug/evo-control` runs and prints the version line.

**Commit:** `bootstrap workspace`

---

## Step 2 — Vendor the kernel module with attribution

**Goal:** `kmod/` directory with `evo_raw.c`, Makefile, dkms.conf, and a README crediting upstream. Stripped to EVO 8 only.

**Files to create:**
- `kmod/evo_raw.c` — copy from upstream, then:
  - Remove the EVO 4 entry from the model table and id_table (keep only `0x2708/0x0007`).
  - Update the module description string accordingly.
  - Keep the file's existing `SPDX-License-Identifier: GPL-2.0+` header.
  - Add an `AUTHORS:` comment block citing `vanzaho/audient-evo-py`.
- `kmod/Makefile` — same as upstream; no EVO-4-specific content to strip.
- `kmod/dkms.conf` — `PACKAGE_NAME="evo-raw"`, `PACKAGE_VERSION="0.1.0"`, etc.
- `kmod/README.md` — explains:
  - What the module does (one paragraph, lift from DESIGN.md).
  - How to build/load manually (for development without DKMS).
  - That this is NOT typically built by hand — `evo-control install-driver` handles it.
  - Attribution to upstream.

**Verify it actually builds against the running kernel** (do not load yet — the `install-driver` subcommand will be the canonical way to load):
```
cd kmod && make
```
Should produce `evo_raw.ko`. Don't `insmod` it manually unless explicitly debugging — let the install subcommand do it later.

**Acceptance:**
- `make -C kmod` produces `evo_raw.ko`.
- Source files have correct SPDX headers and attribution.
- `kmod/Makefile clean` works.

**Commit:** `vendor evo_raw kernel module (vanzaho, GPL-2.0+)`

---

## Step 3 — `evo-protocol` crate: pure types and dB↔raw conversions

**Goal:** Self-contained library of EVO 8 protocol constants, control selectors, and dB↔raw codecs. No I/O. No std requirement (use `core` where possible — but `std` is acceptable since the binary always runs in std).

**Module layout:**
- `entities.rs` — `Entity` enum: `Fu10Output`, `Fu11InputGain`, `Eu58Input`, `Eu59Output`, `Mu60Mixer`. Constants for `wIndex` values.
- `controls.rs` — `ControlSelector` enum, `RequestType` (`Get`, `Set`).
- `codec.rs` — `db_to_q88(db: f32) -> i16`, `q88_to_db(raw: i16) -> f32`. Test edge cases: 0 dB, -128 dB, +6 dB, clamp ranges.
- `device.rs` — `DeviceSpec` struct with EVO 8 constants:
  - `vid: 0x2708`, `pid: 0x0007`
  - `display_name: "Audient EVO 8"`
  - `num_inputs: 4` (XLR/TRS combos 1–4)
  - `num_output_pairs: 2` (Main + headphones)
  - `mixer_inputs: 10`, `mixer_outputs: 4`
  - dB ranges per control type
- `lib.rs` — re-exports.

**Tests:**
- Round-trip: `q88_to_db(db_to_q88(x)) ≈ x` for x in [-128, +6].
- Boundary: `db_to_q88(-128.0) == 0x8000_i16` and `db_to_q88(0.0) == 0x0000`.
- Clamping behavior on out-of-range inputs.

**Acceptance:**
- `cargo test -p evo-protocol` passes.
- Crate has zero non-test deps beyond `core`/`std`.

**Commit:** `evo-protocol: types and codecs`

---

## Step 4 — `evo-driver` crate: ioctl client to /dev/evo8

**Goal:** Safe Rust wrapper around the kmod's ioctl. Provides `Driver` with `get_cur` / `set_cur` methods and high-level operations.

**Module layout:**
- `ioctl.rs` — raw `evo_ctrl_xfer` struct (`#[repr(C)]`, matches the C struct in `kmod/evo_raw.c`), `EVO_CTRL_TRANSFER` ioctl number computed at compile time, raw `ioctl()` syscall via `nix` or `libc`.
  - Ioctl number: `_IOWR('E', 0, sizeof(struct))` = `(3 << 30) | (264 << 16) | (0x45 << 8) | 0`.
- `driver.rs` — `Driver { fd: File }`:
  - `Driver::open() -> io::Result<Self>` — opens `/dev/evo8`.
  - `get_cur(wValue: u16, wIndex: u16, len: u16) -> io::Result<Vec<u8>>`.
  - `set_cur(wValue: u16, wIndex: u16, data: &[u8]) -> io::Result<()>`.
  - High-level: `volume_get(pair) -> f32`, `volume_set(pair, db)`, `gain_get/set`, `phantom_get/set`, `mute_get/set`, `mixer_set(in_idx, out_idx, db)`. Use the protocol crate's codecs.
  - Use `evo-protocol::DeviceSpec` to validate channel indices.
- `worker.rs` — background thread + crossbeam-channel:
  - `DriverHandle` (cheap-clone, holds sender) for the GUI/CLI.
  - Messages: `RefreshStatus`, `Set { control, value }`, `Shutdown`.
  - Responses sent via a watch channel or callback.
- `hotplug.rs` — `HotplugMonitor` using `udev` crate (or polling `/dev/evo8` existence as fallback): emits `Connected` / `Disconnected` events.
- `lib.rs` — re-exports.

**Deps:** `nix` (or `libc`), `crossbeam-channel`, `udev` (optional for hotplug), `evo-protocol`, `thiserror`.

**Tests:**
- Mock the file descriptor (use a tempfile + a mock ioctl) for unit-testable codec round-trips.
- Real-hardware integration tests gated behind `--features hardware-tests` and an env var.

**Acceptance:**
- `cargo build -p evo-driver` succeeds.
- Unit tests pass without hardware.
- Code paths exist for "kmod absent" — `Driver::open()` returns a typed error and the GUI can degrade to ALSA-fallback (Step 8).

**Commit:** `evo-driver: ioctl client and worker thread`

---

## Step 5 — `evo-config` crate: presets and state shadow

**Goal:** TOML serialization of preset state, shadow store for write-only MU60.

**Module layout:**
- `state.rs` — `DeviceState` struct serializable to TOML. Fields cover every v1 control: `output_volume_db: [f32; 2]`, `input_gain_db: [f32; 4]`, `phantom: [bool; 4]`, `input_mute: [bool; 4]`, `output_mute: [bool; 2]`, `mixer: [[f32; 4]; 10]` (in × out cross-points). `schema: u32 = 1`.
- `paths.rs` — XDG paths: `~/.config/evo-control/state.toml`, `~/.config/evo-control/presets/<name>.toml`.
- `store.rs` — atomic write (write-temp-then-rename), load with version check.
- `lib.rs` — `Config { state: DeviceState, presets: HashMap<String, DeviceState> }`.

**Deps:** `serde`, `toml`, `dirs`.

**Tests:**
- Round-trip serialize/deserialize.
- Version mismatch handling (forward compatibility).
- Atomic write doesn't corrupt on simulated-crash tests.

**Acceptance:**
- `cargo test -p evo-config` passes.

**Commit:** `evo-config: TOML state and presets`

---

## Step 6 — `evo-app` skeleton with clap CLI dispatcher

**Goal:** The binary parses CLI args, dispatches to GUI or to a CLI command. GUI is still empty/placeholder; CLI commands wire to `evo-driver`.

**Module layout:**
- `main.rs` — clap parser, dispatch.
- `cli.rs` — handlers for `set`, `get`, `mixer`, `preset`, `status`. Each opens the driver, performs op, prints result. Suppress GUI startup when any subcommand is given.
- `gui/mod.rs` — empty `App` struct, `eframe::App` impl that draws a "hello" window. Wire up later in Step 9.
- `install.rs` — `install_driver()` and `uninstall_driver()` stubs returning "not implemented" — finished in Step 7.
- `probe.rs` — `probe()` stub — finished in Step 8.

**CLI surface (clap):**
```
evo-control                                 # GUI
evo-control set volume <DB>
evo-control set gain <input> <DB>
evo-control set phantom <input> <on|off>
evo-control set mute <target> <on|off>
evo-control mixer set <in> <out> --db <DB>
evo-control get <control> [target]
evo-control status
evo-control preset save <name>
evo-control preset load <name>
evo-control preset list
evo-control probe                           # (Step 8)
evo-control install-driver                  # (Step 7) — sudo enforced
evo-control uninstall-driver                # (Step 7) — sudo enforced
evo-control --apply-default-preset          # used by udev hook
```

**Deps:** `clap` (derive), `eframe` (with the egui re-export), `anyhow`, `evo-driver`, `evo-config`, `evo-protocol`.

Pin `eframe`/`egui` to the latest stable minor version available at build time.

**Acceptance:**
- `cargo run -p evo-app -- --help` shows the full subcommand tree.
- `cargo run -p evo-app` opens an egui window with "evo-control".
- All CLI subcommands compile but most can return "not implemented" except trivial ones.

**Commit:** `evo-app: CLI skeleton + empty GUI`

---

## Step 7 — `install-driver` and `uninstall-driver` subcommands

**Goal:** A user can run `sudo evo-control install-driver` on a clean CachyOS box and end up with a working `/dev/evo8`.

**Behavior of `install-driver`:**
1. Refuse if not running as root.
2. Check `pacman -Q linux-cachyos-headers base-devel` — fail with helpful message if missing.
3. Check `dkms` — if absent, prompt and run `pacman -S --needed --noconfirm dkms` (respect `--non-interactive` flag).
4. Bundle the kmod sources into the binary via `include_bytes!("../../kmod/evo_raw.c")` etc. Extract to `/usr/src/evo-raw-<version>/`.
5. Run `dkms add /usr/src/evo-raw-<version>` and `dkms install evo-raw/<version>`.
6. Write `/etc/udev/rules.d/99-evo.rules` (also bundled via `include_bytes!`).
7. Write `/etc/systemd/user/evo-control-apply.service` (bundled).
8. Write `~/.config/wireplumber/wireplumber.conf.d/50-evo-routing.conf` (bundled). Use the *invoking user's* HOME — fetch from `SUDO_USER` if set.
9. `udevadm control --reload && udevadm trigger`.
10. `modprobe evo_raw`.
11. Verify `/dev/evo8` exists. Print success.

**Behavior of `uninstall-driver`:** reverse each step. Idempotent. Removes `/usr/src/evo-raw-*`, `/etc/udev/rules.d/99-evo.rules`, the systemd unit, the WirePlumber config; runs `dkms remove evo-raw/<version> --all`; `rmmod evo_raw` if loaded.

**Deps:** stdlib `Command`, `nix::unistd::Uid` for euid check, `which` crate (optional).

**Acceptance:**
- `sudo evo-control install-driver` on a fresh box ends with `/dev/evo8` accessible to the `audio` group, kmod loaded, `audient: device EVO8 found` in `dmesg`.
- `sudo evo-control uninstall-driver` returns the system to the prior state.
- Re-running `install-driver` is a no-op when already installed (idempotent).

**Commit:** `install-driver / uninstall-driver subcommands`

---

## Step 8 — `probe` subcommand: protocol verification on real hardware

**Goal:** Walk every documented EVO 8 control, send `GET_CUR`, validate shape and range. Report a compact table of confirmed / unexpected / failing controls. **Do not write to the device** unless `--probe-writable` is passed (then SET back the same value just read, to verify round-trip).

This catches discrepancies between vanzaho's documented EVO 8 protocol (which they marked as untested) and what your specific EVO 8 actually does, before the GUI starts trusting the protocol.

**Output:** ASCII table to stdout. Color via `nu_ansi_term`/`anstream`.

**Acceptance:**
- `evo-control probe` runs to completion in <2 seconds with the device connected.
- Each row shows: entity, wValue, wIndex, expected size, actual size, value, status (OK / SHAPE / STALL).
- Any STALL or SHAPE row is printed in red and the exit code is non-zero.
- No write-side-effects without `--probe-writable`.

**Commit:** `probe: walk and verify EVO 8 protocol on real hardware`

---

## Step 9 — Basic CLI: `set` / `get` / `status`

**Goal:** End-to-end CLI for the simple controls. Validate against the device after Step 8 confirms the protocol.

- `evo-control set volume -20` writes FU10 to both pairs.
- `evo-control set volume -20 --pair 1` writes only the second pair.
- `evo-control get volume` reads pair 0 and prints in dB.
- `evo-control set phantom input1 on` writes EU58 CS=0 CN=0.
- `evo-control set mute output on` writes EU59.
- `evo-control status` reads all readable controls, prints a compact summary.

**Acceptance:**
- All listed commands work on the live EVO 8 with the kmod loaded.
- Out-of-range arguments produce useful error messages, no panic.
- Setting then getting yields the value within ±0.5 dB (device quantizes).

**Commit:** `cli: set/get/status for non-mixer controls`

---

## Step 10 — Mixer CLI + state shadow

**Goal:** `evo-control mixer set <in> <out> --db <DB>` writes MU60 cross-points. Because MU60 is write-only, every set updates the on-disk shadow in `~/.config/evo-control/state.toml` so `get` and the GUI can render the matrix.

- `evo-control mixer set input1 loop-l --db -6` — by name.
- `evo-control mixer set 0 0 --db -6` — by index.
- `evo-control mixer get` — reads the shadow, prints the full 10×4 matrix.
- `evo-control mixer reset` — resets to defaults (diagonal cross-points unity, off-diagonal silence) and writes both device + shadow.

**Acceptance:**
- Shadow file is updated atomically on every set.
- `mixer get` after `mixer set` returns the new value.
- Default reset matches the device's hardware default per vanzaho's notes.

**Commit:** `cli: mixer matrix with shadow store`

---

## Step 11 — Presets

**Goal:** Save / load / list named presets. Atomic writes, atomic loads.

- `evo-control preset save <name>` — captures current `state.toml`, writes to `presets/<name>.toml`.
- `evo-control preset load <name>` — reads the preset file, applies every control to the device, updates `state.toml`.
- `evo-control preset list` — lists files in `presets/`.
- `evo-control preset delete <name>` — removes a preset file.

**Acceptance:**
- Round-trip: save preset, change some controls, load preset, controls return to saved values.
- Loading a nonexistent preset prints an error, exit nonzero.

**Commit:** `cli: preset save/load/list/delete`

---

## Step 12 — udev hotplug + apply-on-connect

**Goal:** Plugging in the EVO 8 (or booting with it plugged in) auto-applies the default preset.

- `packaging/99-evo.rules` includes a `RUN+=` or `TAG+=systemd, ENV{SYSTEMD_USER_WANTS}+=evo-control-apply.service` line that triggers the user-level systemd unit.
- `packaging/evo-control-apply.service` runs `evo-control --apply-default-preset` as the user.
- `evo-control --apply-default-preset` reads `~/.config/evo-control/state.toml` (or a designated default preset) and applies all controls. Bails gracefully if device isn't ready yet (retry with backoff, max 10 s).

**Acceptance:**
- Unplug + replug the EVO 8 → state from `state.toml` is reapplied within a few seconds.
- Boot with EVO 8 plugged in → state applied during user-session startup.
- No errors in `journalctl --user -u evo-control-apply`.

**Commit:** `udev hotplug + apply-default-preset systemd unit`

---

## Step 13 — GUI: channel strips and mixer view

**Goal:** Functional egui GUI that renders the device state, lets the user manipulate every v1 control, and stays in sync with the device worker thread.

**Layout** (single-window, resizable):
- **Top bar:** device status (connected / disconnected), preset selector, save/load/save-as buttons.
- **Input strips (4):** label, gain knob with dB readout, phantom toggle (red when on), mute button, send-to-mixer level (vertical slider into loopback).
- **Output strips (2 pairs):** label, volume knob (large), mute button.
- **Mixer panel** (collapsible): 10×4 matrix grid view with click-to-edit cross-points (advanced view). Channel strips remain the primary surface.

**Custom widgets needed:**
- `RotaryKnob` (dB-aware, draggable, scroll wheel).
- `LevelSlider` (vertical fader; for the mixer cross-points and master volumes).
- `ToggleLamp` (button with LED-like glow when on; for phantom/mute).

Each is a few dozen LOC of egui `Painter` calls. Don't pull in a widget crate.

**State sync:**
- `DriverHandle` is held by `App`. On `update`, if window is focused and 200 ms have passed since last poll, send `RefreshStatus`. On response, update `App::state`.
- User input → debounced 50 ms → `Set` request → on response, confirm state.
- Hotplug events → show "device disconnected" overlay, disable controls; on reconnect, reload state.

**Acceptance:**
- All controls reachable via mouse or keyboard.
- GUI tracks CLI changes (run `evo-control set volume -30` while GUI is open → fader moves within 200 ms).
- Disconnect/reconnect handled gracefully without crashing.
- Resizes cleanly down to ~600×400 minimum.

**Commit:** `gui: channel strips, mixer panel, state sync`

---

## Step 14 — README polish + first-run UX

**Goal:** Repo is presentable. New users can install, troubleshoot, and find help.

- Update `README.md` with: install instructions (`cargo install` + `install-driver`), screenshot, usage examples, troubleshooting (kmod build failures, missing headers, what to do if `/dev/evo8` doesn't appear).
- Add a `--first-run-check` mode that the GUI invokes if `/dev/evo8` is missing, telling the user to run `sudo evo-control install-driver`.

**Acceptance:**
- A friend with an EVO 8 can `cargo install evo-control && sudo evo-control install-driver` and have a working app from the README alone.

**Commit:** `README + first-run UX`

---

## Notes for the implementer

- **The protocol may have bugs we discover.** Run `evo-control probe` after Step 8 and **before** trusting any GET/SET in Step 9+. If anything in vanzaho's tables is wrong on this specific EVO 8, fix `evo-protocol` and document the discrepancy in `DESIGN.md` under a "EVO 8 hardware notes" section.
- **Don't skip the worker-thread pattern.** Doing ioctls from the egui frame is easy and fast in the small but produces awful UX hitches under load. Set it up correctly in Step 4 the first time.
- **MU60 write-only is non-negotiable.** Don't try to GET_CUR mixer cross-points "to be sure" — vanzaho documents that some entities STALL on bad probes and the device may need a USB unplug to recover. The shadow file is the source of truth.
- **udev rule sets `TAG+="uaccess"`.** This is what grants the logged-in user access to `/dev/evo8` without needing to add them to a group. Don't fight it; it's the modern Linux way.
- **Stay terse.** No multi-paragraph doc comments, no "added for X feature" comments, no defensive validation past system boundaries. Trust internal callers.
