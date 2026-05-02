# evo-control

Linux-native GUI and CLI for the Audient EVO 8 USB audio interface.
Replaces the vendor's Windows/macOS-only Evo Control app.

## Install

```sh
cargo install evo-control
sudo evo-control install-driver   # one-time: DKMS module, udev rule, WirePlumber config
```

## Usage

```sh
evo-control                          # launch GUI
evo-control set volume -20
evo-control set gain input1 40
evo-control set phantom input1 on
evo-control set mute output on
evo-control mixer set input1 loop-l --db -6
evo-control status
evo-control preset save live
evo-control preset load live
evo-control probe                    # verify EVO 8 protocol on connected device
```

## Uninstall

```sh
sudo evo-control uninstall-driver
cargo uninstall evo-control
```

## Design

See [DESIGN.md](DESIGN.md) for architecture, protocol details, and decisions.
See [PLAN.md](PLAN.md) for the implementation plan.

## Prior art

Protocol reverse-engineering by [vanzaho/audient-evo-py](https://github.com/vanzaho/audient-evo-py) (public domain).
The bundled kernel module (`kmod/evo_raw.c`) is derived from that project.
