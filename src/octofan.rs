//! The Octominer fan-controller board ("Octofan": an ATmega324PB on USB,
//! `16c0:05dc`, manufacturer `info@octominer.com`, product `Fan-Controller`).
//!
//! Requests, from the vendor's `fan_controller_cli` (HiveOS): every one is a
//! vendor control-IN transfer (`bmRequestType` 0xC0) with a 256-byte buffer.
//! - `0x00`: feed the watchdog (`fan_controller_cli -s`);
//! - `0x06`: watchdog status (mode, short and long timeouts, resets);
//! - `0x07`: set the timeouts: `wValue` = short timeout in seconds, `wIndex`
//!   = 0x6400 + long timeout in minutes (`-w SHORT -v LONG`, LONG in seconds,
//!   at least 60).
//!
//! The board resets the motherboard when it is not fed in time; HiveOS feeds
//! it every 5 s (`-w 120 -v 1500`, then `-s` while sshd answers). Reset
//! (0x91) and power (0x92) requests exist; this program never sends them.

use std::io;
use std::path::Path;

use crate::usb::{self, Device, Found};

/// USB ids (V-USB's shared libusb ids, so the strings must match too).
pub const VID: u16 = 0x16c0;
/// See [`VID`].
pub const PID: u16 = 0x05dc;
/// The board's manufacturer string.
pub const MANUFACTURER: &str = "info@octominer.com";
/// The board's product string.
pub const PRODUCT: &str = "Fan-Controller";

const REQ_FEED: u8 = 0x00;
const REQ_WATCHDOG_STATUS: u8 = 0x06;
const REQ_WATCHDOG_SET: u8 = 0x07;
const BUF: usize = 256;
const TIMEOUT_MS: u32 = 5000;

/// The boards on this machine.
pub fn find() -> Vec<Found> {
    usb::find(Path::new("/sys/bus/usb/devices"), VID, PID, MANUFACTURER, PRODUCT)
}

/// The watchdog's state as the board reports it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Status {
    /// Watchdog mode.
    pub mode: u8,
    /// Short timeout, seconds.
    pub short: u32,
    /// Long timeout, seconds.
    pub long: u32,
    /// Resets the watchdog has done (as the board counts them).
    pub resets: u8,
    /// The reply's first bytes, for the record.
    pub raw: [u8; 8],
}

impl Status {
    /// From the reply to request 0x06.
    pub fn parse(b: &[u8]) -> Option<Status> {
        let raw: [u8; 8] = b.get(..8)?.try_into().ok()?;
        // Timeouts: little-endian, in 3 s units (40 and 500 for 120 s and
        // 1500 s, as HiveOS sets them).
        let ticks = |i: usize| 3 * u32::from(u16::from_le_bytes([raw[i], raw[i + 1]]));
        Some(Status {
            mode: raw[0],
            short: ticks(3),
            long: ticks(5),
            resets: raw[7],
            raw,
        })
    }
}

impl std::fmt::Display for Status {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "mode {}, short timeout {} s, long timeout {} s, resets {} (raw {:02x?})",
            self.mode, self.short, self.long, self.resets, self.raw
        )
    }
}

/// `wIndex` for request 0x07: 0x6400 + the long timeout in minutes, the
/// way `fan_controller_cli -w S -v L` sends it (L under 60 s counts as 60).
pub fn long_index(long_secs: u32) -> Result<u16, String> {
    let s = if long_secs < 60 { 60 } else { long_secs };
    let min = s / 60;
    if min > 0xff {
        return Err(format!("long timeout {long_secs} s: at most {} s", 0xff * 60));
    }
    Ok(0x6400 + min as u16)
}

/// An open board.
pub struct Board {
    dev: Device,
    /// Where it was found.
    pub at: Found,
}

impl Board {
    /// Opens a board found by [`find`].
    pub fn open(at: Found) -> io::Result<Board> {
        Ok(Board {
            dev: Device::open(&at.node)?,
            at,
        })
    }

    /// Feeds the watchdog.
    pub fn feed(&self) -> io::Result<()> {
        let mut b = [0u8; BUF];
        self.dev.vendor_in(REQ_FEED, 0, 0, &mut b, TIMEOUT_MS)?;
        Ok(())
    }

    /// The watchdog's state.
    pub fn status(&self) -> io::Result<Status> {
        let mut b = [0u8; BUF];
        let n = self
            .dev
            .vendor_in(REQ_WATCHDOG_STATUS, 0, 0, &mut b, TIMEOUT_MS)?;
        Status::parse(&b[..n]).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("watchdog status: {n} bytes"),
            )
        })
    }

    /// Sets the short (seconds) and long (seconds, kept in minutes)
    /// timeouts.
    pub fn set_timeouts(&self, short_secs: u16, long_secs: u32) -> io::Result<()> {
        let index = long_index(long_secs).map_err(io::Error::other)?;
        let mut b = [0u8; BUF];
        self.dev
            .vendor_in(REQ_WATCHDOG_SET, short_secs, index, &mut b, TIMEOUT_MS)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_layout() {
        // Read from the rig's board (HiveOS settings: 120 s, 1500 s).
        let s = Status::parse(&[0x01, 0x02, 0x5a, 0x28, 0x00, 0xf4, 0x01, 0x5c]).unwrap();
        assert_eq!((s.mode, s.short, s.long, s.resets), (1, 120, 1500, 92));
        assert!(Status::parse(&[1, 2, 3]).is_none());
    }

    #[test]
    fn long_timeout_in_minutes() {
        // HiveOS: -w 120 -v 1500.
        assert_eq!(long_index(1500), Ok(0x6400 + 25));
        assert_eq!(long_index(10), Ok(0x6401));
        assert!(long_index(256 * 60).is_err());
    }
}
