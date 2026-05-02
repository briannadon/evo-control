# evo-control

Linux-native GUI and CLI for the Audient EVO 8 USB audio interface.
Replaces the vendor's Windows/macOS-only Evo Control app.

Controls phantom power, input gain, output volume, mute, and the internal
loopback mixer — everything the vendor app exposes, over a vendored kernel
module that coexists with `snd-usb-audio` without disturbing audio streams.

## Requirements

- Linux with DKMS support and kernel headers installed
- Audient EVO 8 (USB ID `2708:0007`)
- Rust toolchain (`cargo 1.75+`) to build from source

On Arch / CachyOS: `sudo pacman -S --needed linux-cachyos-headers base-devel dkms`

## Install

```sh
cargo install evo-control
sudo evo-control install-driver
```

`install-driver` builds and registers the kernel module via DKMS, installs the
udev rule that grants the logged-in user access to `/dev/evo8`, installs a
WirePlumber config that prevents the device from being suspended, and sets up a
systemd user unit that re-applies your last-used preset whenever the EVO 8 is
plugged in.

## Usage

```sh
evo-control                              # launch GUI

# volume / gain / phantom / mute
evo-control set volume -- -20            # main + headphones (dB, range -96..0)
evo-control set volume -- -20 --pair 1  # headphones only (0=main, 1=phones)
evo-control set gain 1 40               # input 1 gain in dB (-8..50)
evo-control set phantom 1 on            # 48V phantom on input 1
evo-control set mute output on          # mute main output
evo-control set mute input1 on          # mute input 1 monitor

# read back
evo-control get volume
evo-control get gain 1
evo-control status                       # all controls at once

# loopback mixer (10×4 matrix; state tracked on disk — write-only on hardware)
evo-control mixer set 0 0 --db -6       # input 0 → output L, -6 dB
evo-control mixer get                   # print full 10×4 matrix
evo-control mixer reset                 # restore hardware defaults

# presets
evo-control preset save live
evo-control preset load live
evo-control preset list
evo-control preset delete live

# diagnostics
evo-control probe                        # walk every control, verify protocol
evo-control probe --probe-writable       # also round-trip SET (writes original value back)
```

## Presets and auto-apply

Device state is saved to `~/.config/evo-control/state.toml` on every change
and restored automatically when the EVO 8 is connected (via udev → systemd
user unit). Named presets live in `~/.config/evo-control/presets/`.

## Uninstall

```sh
sudo evo-control uninstall-driver
cargo uninstall evo-control
```

`uninstall-driver` removes the DKMS module, udev rule, systemd unit, and
WirePlumber config. It is idempotent.

## Troubleshooting

**`/dev/evo8` does not appear after install**

1. Confirm the module loaded: `lsmod | grep evo_raw`
2. If not: `sudo modprobe evo_raw` — if that fails, check `dkms status`:
   `dkms status evo-raw` should show `installed` for your running kernel.
3. If the DKMS build failed: `sudo dkms install evo-raw/0.1.0 -v` for the full
   build log. The most common cause is missing kernel headers.
4. Verify the udev rule fired: `udevadm test $(udevadm info -q path -n /dev/bus/usb/... )`
   (find the EVO 8 bus path with `lsusb -d 2708:0007`).

**DKMS build fails: "no such file or directory" for kernel headers**

Install headers for your running kernel. On Arch/CachyOS:
```sh
sudo pacman -S linux-cachyos-headers   # adjust to your kernel variant
```
Then retry: `sudo dkms install evo-raw/0.1.0`

**GUI opens but controls are greyed out / "device not available"**

The kmod is not loaded or `/dev/evo8` is not accessible to your user. Run
`sudo evo-control install-driver` if you haven't yet. If you just installed it,
unplug and replug the EVO 8 to retrigger the udev rule.

**`evo-control probe` shows STALL rows**

A `STALL` means the device returned a USB STALL on a GET_CUR for that control.
This is expected for the mixer matrix (MU60 is write-only). For other controls,
open an issue with the full probe output — this may indicate a protocol
discrepancy between your hardware and the vanzaho tables we derived from.

**Audio stops working after install**

`install-driver` does not touch `snd-usb-audio` — audio streaming is handled
entirely by the kernel's built-in driver and is unaffected. If something broke,
check `dmesg | grep -i usb` for errors. The WirePlumber config installed at
`~/.config/wireplumber/wireplumber.conf.d/50-evo-routing.conf` sets the device
to stereo-only and disables idle suspend; remove it if it conflicts with your
setup and restart WirePlumber.

## Design

See [DESIGN.md](DESIGN.md) for architecture, protocol details, and decisions.

## License

Rust crates: **MIT OR Apache-2.0**.
`kmod/evo_raw.c`: **GPL-2.0-or-later** (kernel module requirement).

Protocol reverse-engineering and the original kernel module by
[vanzaho/audient-evo-py](https://github.com/vanzaho/audient-evo-py).
