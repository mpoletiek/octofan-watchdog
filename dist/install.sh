#!/bin/bash
# Install (or upgrade) octofan-watchdog as a systemd service. Run as root,
# from an unpacked release (binary, unit and README side by side) or as
# dist/install.sh in a source checkout after `cargo build --release`.
set -euo pipefail
cd "$(dirname "$0")"
if [[ $EUID -ne 0 ]]; then
    echo "install.sh: run as root" >&2
    exit 1
fi
bin=octofan-watchdog
[[ -f $bin ]] || bin=../target/release/octofan-watchdog
if [[ ! -f $bin ]]; then
    echo "install.sh: no octofan-watchdog binary here or in ../target/release (cargo build --release)" >&2
    exit 1
fi
readme=README.md
[[ -f $readme ]] || readme=../README.md
install -m 0755 "$bin" /usr/local/bin/octofan-watchdog
install -m 0644 octofan-watchdog.service /etc/systemd/system/octofan-watchdog.service
install -D -m 0644 "$readme" /usr/local/share/doc/octofan-watchdog/README.md
systemctl daemon-reload
systemctl enable octofan-watchdog.service
# Restart rather than start, so an upgrade runs the new binary.
systemctl restart octofan-watchdog.service
sleep 2
systemctl --no-pager --lines=8 status octofan-watchdog.service
