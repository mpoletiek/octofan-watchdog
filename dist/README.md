# octofan-watchdog

Keeps an Octominer rig running on plain Linux: the rig's fan-controller board
(the "Octofan", USB `16c0:05dc`, "info@octominer.com" / "Fan-Controller") has
a hardware watchdog that resets the motherboard unless the OS feeds it.
OctOS and HiveOS feed it; on any other Linux nothing does, and the board
resets the rig 25 minutes (1500 s) after power-on, loaded or idle.

This program feeds it, from a small systemd service. The watchdog keeps its
purpose: if the machine hangs (or a `--check` fails), feeding stops and the
board resets the rig.

    octofan-watchdog status          # the board and its watchdog settings
    octofan-watchdog feed            # feed once
    octofan-watchdog run             # the service: feed every 10 s
    octofan-watchdog run --check 127.0.0.1:22   # only while sshd answers
    octofan-watchdog set-timeouts 120 1500      # as HiveOS sets them

Install (as root, with the binary, the unit and this README together):

    ./install.sh

It uses the kernel's usbdevfs directly (no libusb) and needs root for the
board's `/dev/bus/usb` node. It never sends the board's reset or power
commands.

## The protocol

From the vendor's `fan_controller_cli` (shipped with HiveOS). Every request
is a vendor control-IN transfer (`bmRequestType` 0xC0, 256-byte buffer):

| bRequest | wValue | wIndex | what |
|---|---|---|---|
| 0x00 | 0 | 0 | feed the watchdog (`-s`) |
| 0x06 | 0 | 0 | watchdog status |
| 0x07 | short timeout, s | 0x6400 + long timeout, min | set the timeouts (`-w S -v L`) |
| 0x91 / 0x92 | | | reset / power the rig (never sent) |

The status reply: byte 0 the mode; bytes 3-4 and 5-6 the short and long
timeouts (little-endian, in 3 s units: 40 and 500 for 120 s and 1500 s);
byte 7 the number of resets the watchdog has done.

HiveOS sets `-w 120 -v 1500` and then feeds every 5 s while sshd answers.
