//! Linux sysfs discovery for Logitech HID++ 2.0 `/dev/hidraw*` endpoints.

use std::fs;
use std::path::{Path, PathBuf};

use crate::error::Result;

/// Logitech USB Vendor ID.
pub const LOGITECH_VID: u16 = 0x046D;

/// Metadata for a discovered Logitech HID++ `hidraw` endpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HidrawCandidate {
    pub path: PathBuf,
    pub vendor_id: u16,
    pub product_id: u16,
    pub name: String,
    pub phys: String,
    pub supports_short: bool,
    pub supports_long: bool,
}

/// Scans `/sys/class/hidraw` for Logitech endpoints that advertise HID++ reports
/// (Vendor Usage Page `0xFF00` with report ID `0x10` or `0x11`).
pub fn find_hidpp_candidates() -> Result<Vec<HidrawCandidate>> {
    find_hidpp_candidates_in(Path::new("/sys/class/hidraw"))
}

pub(crate) fn find_hidpp_candidates_in(sysfs_root: &Path) -> Result<Vec<HidrawCandidate>> {
    let mut candidates = Vec::new();
    let entries = match fs::read_dir(sysfs_root) {
        Ok(entries) => entries,
        Err(_) => return Ok(candidates),
    };

    for entry in entries.flatten() {
        let dev_name = entry.file_name();
        let dev_name_str = dev_name.to_string_lossy();
        if !dev_name_str.starts_with("hidraw") {
            continue;
        }

        let device_dir = entry.path().join("device");
        let uevent = match fs::read_to_string(device_dir.join("uevent")) {
            Ok(content) => content,
            Err(_) => continue,
        };

        let Some((vid, pid, name, phys)) = parse_uevent(&uevent) else {
            continue;
        };
        if vid != LOGITECH_VID {
            continue;
        }

        let descriptor = fs::read(device_dir.join("report_descriptor")).unwrap_or_default();
        let (supports_short, supports_long) = parse_hidpp_report_support(&descriptor);
        if !supports_short && !supports_long {
            continue;
        }

        candidates.push(HidrawCandidate {
            path: PathBuf::from(format!("/dev/{dev_name_str}")),
            vendor_id: vid,
            product_id: pid,
            name,
            phys,
            supports_short,
            supports_long,
        });
    }

    candidates.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(candidates)
}

/// Parses `HID_ID`, `HID_NAME`, and `HID_PHYS` from a sysfs `uevent` file.
pub(crate) fn parse_uevent(content: &str) -> Option<(u16, u16, String, String)> {
    let mut vid = None;
    let mut pid = None;
    let mut name = String::new();
    let mut phys = String::new();

    for line in content.lines() {
        if let Some(val) = line.strip_prefix("HID_ID=") {
            // Format: BUS:VID:PID (hex, e.g. 0003:0000046D:0000C547)
            let parts: Vec<&str> = val.split(':').collect();
            if parts.len() == 3 {
                vid = u16::from_str_radix(parts[1].trim_start_matches('0'), 16)
                    .ok()
                    .or_else(|| u32::from_str_radix(parts[1], 16).ok().map(|v| v as u16));
                pid = u16::from_str_radix(parts[2].trim_start_matches('0'), 16)
                    .ok()
                    .or_else(|| u32::from_str_radix(parts[2], 16).ok().map(|v| v as u16));
            }
        } else if let Some(val) = line.strip_prefix("HID_NAME=") {
            name = val.to_string();
        } else if let Some(val) = line.strip_prefix("HID_PHYS=") {
            phys = val.to_string();
        }
    }

    Some((vid?, pid?, name, phys))
}

/// Inspects a USB HID report descriptor to detect whether it exposes Logitech's
/// vendor-defined HID++ usage page (`0xFF00`) with Short (`0x10`) and/or Long (`0x11`) reports.
pub(crate) fn parse_hidpp_report_support(descriptor: &[u8]) -> (bool, bool) {
    let mut i = 0;
    let mut in_vendor_page = false;
    let mut supports_short = false;
    let mut supports_long = false;

    while i < descriptor.len() {
        let prefix = descriptor[i];
        // Long item format (0xFE)
        if prefix == 0xFE {
            if i + 1 >= descriptor.len() {
                break;
            }
            let data_size = descriptor[i + 1] as usize;
            i += 3 + data_size;
            continue;
        }

        let size_code = prefix & 0x03;
        let data_size = match size_code {
            0 => 0,
            1 => 1,
            2 => 2,
            3 => 4,
            _ => unreachable!(),
        };
        let item_tag = prefix & 0xFC;

        if i + 1 + data_size > descriptor.len() {
            break;
        }
        let data = &descriptor[i + 1..i + 1 + data_size];

        match item_tag {
            // Usage Page (Global, tag 0x04)
            0x04 => {
                let page = match data_size {
                    1 => u16::from(data[0]),
                    2 => u16::from_le_bytes([data[0], data[1]]),
                    _ => 0,
                };
                in_vendor_page = page == 0xFF00;
            }
            // Report ID (Global, tag 0x84)
            0x84 if in_vendor_page && data_size == 1 => match data[0] {
                0x10 => supports_short = true,
                0x11 => supports_long = true,
                _ => {}
            },
            _ => {}
        }

        i += 1 + data_size;
    }

    (supports_short, supports_long)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_logitech_lightspeed_uevent() {
        let uevent = "\
DRIVER=hid-generic
HID_ID=0003:0000046D:0000C547
HID_NAME=Logitech USB Receiver
HID_PHYS=usb-0000:00:14.0-1.4/input2
";
        let (vid, pid, name, phys) = parse_uevent(uevent).unwrap();
        assert_eq!(vid, 0x046D);
        assert_eq!(pid, 0xC547);
        assert_eq!(name, "Logitech USB Receiver");
        assert_eq!(phys, "usb-0000:00:14.0-1.4/input2");
    }

    #[test]
    fn parse_hidpp_descriptor_detects_short_and_long() {
        // Usage Page (0xFF00), Usage (0x01), Collection (App), Report ID (0x10), ...,
        // Usage Page (0xFF00), Report ID (0x11), ...
        let desc = [
            0x06, 0x00, 0xFF, // Usage Page (0xFF00)
            0x09, 0x01, // Usage (0x01)
            0xA1, 0x01, // Collection (Application)
            0x85, 0x10, // Report ID (0x10)
            0xC0, // End Collection
            0x06, 0x00, 0xFF, // Usage Page (0xFF00)
            0x09, 0x02, // Usage (0x02)
            0xA1, 0x01, // Collection (Application)
            0x85, 0x11, // Report ID (0x11)
            0xC0, // End Collection
        ];
        assert_eq!(parse_hidpp_report_support(&desc), (true, true));
    }

    #[test]
    fn parse_non_hidpp_mouse_descriptor_returns_false() {
        let desc = [
            0x05, 0x01, // Usage Page (Generic Desktop)
            0x09, 0x02, // Usage (Mouse)
            0xA1, 0x01, // Collection
            0x85, 0x01, // Report ID (0x01)
            0xC0,
        ];
        assert_eq!(parse_hidpp_report_support(&desc), (false, false));
    }
}
