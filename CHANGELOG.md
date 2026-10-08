# Changelog

## 0.1.0

First release.

- `run`: feeds the Octominer fan board's watchdog every 10 s, optionally only
  while TCP checks pass, and reopens the board after USB errors.
- `status`, `feed` and `set-timeouts` for one-off use; the short timeout is
  30 s at least.
- A sandboxed systemd unit and `install.sh`; static x86-64 release binaries.
