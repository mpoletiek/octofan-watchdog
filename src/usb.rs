//! Just enough USB for vendor control requests on Linux, through the
//! kernel's usbdevfs (`/dev/bus/usb/BBB/DDD`): no libusb needed.

use std::fs::{File, OpenOptions};
use std::io;
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};

/// A USB device found in sysfs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    /// The sysfs directory (`/sys/bus/usb/devices/1-7`).
    pub sysfs: PathBuf,
    /// The device node (`/dev/bus/usb/001/002`).
    pub node: PathBuf,
}

fn attr(dir: &Path, name: &str) -> Option<String> {
    std::fs::read_to_string(dir.join(name))
        .ok()
        .map(|s| s.trim().to_string())
}

/// Devices with this vendor and product id and these manufacturer and
/// product strings, from `root` (`/sys/bus/usb/devices`).
pub fn find(root: &Path, vid: u16, pid: u16, manufacturer: &str, product: &str) -> Vec<Found> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut out: Vec<Found> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|d| {
            attr(d, "idVendor").as_deref() == Some(&format!("{vid:04x}"))
                && attr(d, "idProduct").as_deref() == Some(&format!("{pid:04x}"))
                && attr(d, "manufacturer").as_deref() == Some(manufacturer)
                && attr(d, "product").as_deref() == Some(product)
        })
        .filter_map(|d| {
            let bus: u32 = attr(&d, "busnum")?.parse().ok()?;
            let dev: u32 = attr(&d, "devnum")?.parse().ok()?;
            Some(Found {
                node: PathBuf::from(format!("/dev/bus/usb/{bus:03}/{dev:03}")),
                sysfs: d,
            })
        })
        .collect();
    out.sort_by(|a, b| a.sysfs.cmp(&b.sysfs));
    out
}

/// `struct usbdevfs_ctrltransfer` from `<linux/usbdevice_fs.h>`.
#[repr(C)]
struct CtrlTransfer {
    request_type: u8,
    request: u8,
    value: u16,
    index: u16,
    length: u16,
    timeout_ms: u32,
    data: *mut libc::c_void,
}

/// `USBDEVFS_CONTROL` = `_IOWR('U', 0, struct usbdevfs_ctrltransfer)`.
const USBDEVFS_CONTROL: u64 = (3 << 30)
    | ((std::mem::size_of::<CtrlTransfer>() as u64) << 16)
    | ((b'U' as u64) << 8);

/// An open USB device.
pub struct Device {
    file: File,
}

impl Device {
    /// Opens the device node (needs write access: root, or a udev rule).
    pub fn open(node: &Path) -> io::Result<Device> {
        Ok(Device {
            file: OpenOptions::new().read(true).write(true).open(node)?,
        })
    }

    /// A vendor control request to the device, device-to-host
    /// (`bmRequestType` 0xC0), reading into `buf`. Returns the bytes read.
    pub fn vendor_in(
        &self,
        request: u8,
        value: u16,
        index: u16,
        buf: &mut [u8],
        timeout_ms: u32,
    ) -> io::Result<usize> {
        let mut t = CtrlTransfer {
            request_type: 0xC0,
            request,
            value,
            index,
            length: buf.len().min(u16::MAX as usize) as u16,
            timeout_ms,
            data: buf.as_mut_ptr().cast(),
        };
        // SAFETY: `t` matches the kernel's struct and points at `buf`, which
        // outlives the call and is at least `length` bytes.
        let r = unsafe {
            libc::ioctl(
                self.file.as_raw_fd(),
                USBDEVFS_CONTROL as _,
                &mut t as *mut CtrlTransfer,
            )
        };
        if r < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(r as usize)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ioctl_number_matches_the_kernel() {
        // From <linux/usbdevice_fs.h> on x86-64 and aarch64.
        assert_eq!(std::mem::size_of::<CtrlTransfer>(), 24);
        assert_eq!(USBDEVFS_CONTROL, 0xC018_5500);
    }

    #[test]
    fn finds_by_ids_and_strings() {
        let root = std::env::temp_dir().join(format!("octofan-usb-test-{}", std::process::id()));
        let mk = |name: &str, vid: &str, pid: &str, m: &str, p: &str, dev: &str| {
            let d = root.join(name);
            std::fs::create_dir_all(&d).unwrap();
            for (k, v) in [
                ("idVendor", vid),
                ("idProduct", pid),
                ("manufacturer", m),
                ("product", p),
                ("busnum", "1"),
                ("devnum", dev),
            ] {
                std::fs::write(d.join(k), format!("{v}\n")).unwrap();
            }
        };
        mk("1-7", "16c0", "05dc", "info@octominer.com", "Fan-Controller", "2");
        // Same shared ids, another gadget (a USBasp programmer).
        mk("1-3", "16c0", "05dc", "www.fischl.de", "USBasp", "5");
        let f = find(&root, 0x16c0, 0x05dc, "info@octominer.com", "Fan-Controller");
        std::fs::remove_dir_all(&root).unwrap();
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].node, PathBuf::from("/dev/bus/usb/001/002"));
    }
}
