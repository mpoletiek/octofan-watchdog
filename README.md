# octofan-watchdog

Run an Octominer rig on plain Linux (Debian, Ubuntu, Gentoo, ...) instead
of OctOS or HiveOS, without it resetting itself every 25 minutes.

The rig's fan-controller board (the "Octofan", USB `16c0:05dc`,
"info@octominer.com" / "Fan-Controller") has a hardware watchdog that resets
the motherboard unless the OS feeds it. OctOS and HiveOS feed it; on any
other Linux nothing does, and the board resets the rig 25 minutes (1500 s)
after power-on, loaded or idle.

This program feeds it, from a small systemd service. The watchdog keeps its
purpose: if the machine hangs (or a `--check` fails), feeding stops and the
board resets the rig.

- One static binary, no dependencies: it talks to the board through the
  kernel's usbdevfs (no libusb).
- It only ever sends the feed, status and set-timeouts requests, never the
  board's reset or power commands.

## Does my rig have this board?

    lsusb -d 16c0:05dc -v 2>/dev/null | grep -E 'iManufacturer|iProduct'

should show `info@octominer.com` and `Fan-Controller`. (`16c0:05dc` is a
shared id that other gadgets such as USBasp programmers use too, so the
program checks the strings as well.)

## Install

You need root and systemd.

From a release (x86-64, static binary):

    curl -LO https://github.com/mpoletiek/octofan-watchdog/releases/latest/download/octofan-watchdog-x86_64-linux.tar.gz
    tar xzf octofan-watchdog-x86_64-linux.tar.gz
    sudo ./octofan-watchdog-x86_64-linux/install.sh

From source (Rust 1.85 or newer):

    git clone https://github.com/mpoletiek/octofan-watchdog
    cd octofan-watchdog
    cargo build --release
    sudo dist/install.sh

`install.sh` puts the binary in `/usr/local/bin`, the unit in
`/etc/systemd/system`, and this README in `/usr/local/share/doc/octofan-watchdog`,
then enables and (re)starts the service. Running it again upgrades in place.

Check it:

    systemctl status octofan-watchdog
    journalctl -u octofan-watchdog
    sudo octofan-watchdog status

## Usage

    octofan-watchdog status          # the board and its watchdog settings
    octofan-watchdog feed            # feed once
    octofan-watchdog run             # the service: feed every 10 s
    octofan-watchdog run --check 127.0.0.1:22   # only while sshd answers
    octofan-watchdog set-timeouts 120 1500      # as HiveOS sets them

`run` options:

| option | |
|---|---|
| `--interval SECS` | seconds between feeds, 1 to 60 (default 10) |
| `--check HOST:PORT` | feed only while this TCP port accepts connections; repeatable |
| `--set-timeouts S,L` | set the short and long timeouts (seconds) when the board is opened |

`set-timeouts` takes a short timeout of 30 s at least, so the default
10 s feeding always gets several tries in. The long timeout is kept in
whole minutes (60 s at least, 15300 s at most).

The service runs `octofan-watchdog run --interval 10`. To change that, for
example to also stop feeding (and so reset the rig) when sshd stops
answering, as HiveOS does:

    sudo systemctl edit octofan-watchdog

```ini
[Service]
ExecStart=
ExecStart=/usr/local/bin/octofan-watchdog run --interval 5 --check 127.0.0.1:22
```

## Stopping and uninstalling

Once the board has been fed, it expects to keep being fed: stopping the
service (or a crash that systemd cannot restart) means the board resets the
rig when its timeout runs out. Only stop the service when you are about to
reboot, or have something else feeding the board.

To uninstall:

    sudo systemctl disable --now octofan-watchdog
    sudo rm /usr/local/bin/octofan-watchdog /etc/systemd/system/octofan-watchdog.service
    sudo rm -r /usr/local/share/doc/octofan-watchdog
    sudo systemctl daemon-reload

## Troubleshooting

- **"no Octominer fan controller ... on USB"**: the board is not on USB, or
  its strings differ; see [Does my rig have this board?](#does-my-rig-have-this-board).
  The service keeps looking and starts feeding as soon as the board appears.
- **"Permission denied" on `/dev/bus/usb/...`**: run the one-off commands
  with `sudo`.
- **The rig still resets**: check `journalctl -u octofan-watchdog` for
  "not feeding" or "feeding failed" lines, and `octofan-watchdog status`
  for the timeouts and the board's reset count.

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

## License

[MIT](LICENSE). Not affiliated with or endorsed by Octominer; the protocol
was worked out for interoperability, and no vendor code is included.
