# kmod — evo_raw

Out-of-tree Linux kernel module that gives userspace access to the Audient
EVO 8's USB control transfers without disturbing `snd-usb-audio`.

## How it works

`snd-usb-audio` claims interfaces 0–2 (audio control + streaming). Interface 3
(DFU firmware update, unused at runtime) is left unbound. This module binds to
interface 3 solely to obtain a `usb_device` handle, then exposes `/dev/evo8`
as a misc device. A single ioctl (`EVO_CTRL_TRANSFER`) forwards arbitrary USB
control transfers through `usb_control_msg()` on endpoint 0 — bypassing the
`usbfs` interface-ownership check while leaving audio streaming untouched.

## Normal usage

**Don't build this manually.** The `evo-control install-driver` subcommand
handles building, DKMS registration, udev rules, and loading:

```sh
sudo evo-control install-driver
```

## Manual build (development)

Requires kernel headers for the running kernel:

```sh
cd kmod
make
# produces evo_raw.ko — load with: sudo insmod evo_raw.ko
# unload with: sudo rmmod evo_raw
```

## Attribution

Derived from [vanzaho/audient-evo-py](https://github.com/vanzaho/audient-evo-py)
(public domain). The original supported both EVO 4 and EVO 8; this version
is stripped to EVO 8 only.

License: GPL-2.0-or-later (required for Linux kernel modules).
