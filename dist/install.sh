#!/bin/bash
# Install octofan-watchdog as a systemd service (run as root, from the
# directory holding the binary, the unit and README.md).
set -euo pipefail
cd "$(dirname "$0")"
install -m 0755 octofan-watchdog /usr/local/bin/octofan-watchdog
install -m 0644 octofan-watchdog.service /etc/systemd/system/octofan-watchdog.service
install -D -m 0644 README.md /usr/local/share/doc/octofan-watchdog/README.md
systemctl daemon-reload
systemctl enable --now octofan-watchdog.service
sleep 2
systemctl --no-pager --lines=8 status octofan-watchdog.service
