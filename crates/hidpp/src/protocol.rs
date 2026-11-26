//! HID++ 2.0 packet framing, request/response correlation, and feature table resolution.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::error::{Error, HidppErrorCode, Result};
use crate::transport::{MAX_REPORT_LEN, Transport};

pub const REPORT_ID_SHORT: u8 = 0x10;
pub const REPORT_ID_LONG: u8 = 0x11;
pub const SHORT_REPORT_LEN: usize = 7;
pub const LONG_REPORT_LEN: usize = 20;

const HIDPP10_ERROR_SUB_ID: u8 = 0x8F;
const HIDPP20_ERROR_FEATURE_IDX: u8 = 0xFF;

/// Standard HID++ 2.0 feature IDs.
pub mod feature_id {
    pub const IROOT: u16 = 0x0000;
    pub const IFEATURE_SET: u16 = 0x0001;
    pub const DEVICE_TYPE_AND_NAME: u16 = 0x0005;
    pub const BATTERY_STATUS: u16 = 0x1000;
    pub const BATTERY_VOLTAGE: u16 = 0x1001;
    pub const UNIFIED_BATTERY: u16 = 0x1004;
    pub const ADJUSTABLE_DPI: u16 = 0x2201;
    pub const EXTENDED_ADJUSTABLE_DPI: u16 = 0x2202;
    pub const REPORT_RATE: u16 = 0x8060;
    pub const EXTENDED_REPORT_RATE: u16 = 0x8061;
    pub const ONBOARD_PROFILES: u16 = 0x8100;
}

/// Metadata for a single HID++ 2.0 feature reported by `IFeatureSet (0x0001)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FeatureEntry {
    pub index: u8,
    pub id: u16,
    pub feature_type: u8,
    pub version: u8,
}

/// Active HID++ 2.0 session bound to a transport and a specific `device_index`
/// (`0xFF` for wired USB devices, `0x01..=0x06` for wireless receiver slots).
pub struct HidppSession<T: Transport> {
    transport: T,
    device_index: u8,
    sw_id: u8,
    timeout: Duration,
    feature_cache: HashMap<u16, Option<u8>>,
}

impl<T: Transport> HidppSession<T> {
    pub fn new(transport: T, device_index: u8) -> Self {
        let mut feature_cache = HashMap::new();
        // IRoot (0x0000) is always at feature index 0.
        feature_cache.insert(feature_id::IROOT, Some(0));
        Self {
            transport,
            device_index,
            sw_id: 1,
            timeout: Duration::from_millis(1000),
            feature_cache,
        }
    }

    pub fn device_index(&self) -> u8 {
        self.device_index
    }

    pub fn set_timeout(&mut self, timeout: Duration) {
        self.timeout = timeout;
    }

    pub fn into_transport(self) -> T {
        self.transport
    }

    fn next_sw_id(&mut self) -> u8 {
        let id = self.sw_id;
        // Software ID must be 1..=15; 0 is reserved for unsolicited device notifications.
        self.sw_id = if self.sw_id >= 0x0F { 1 } else { self.sw_id + 1 };
        id
    }

    /// Sends a HID++ 2.0 request to `(feature_index, function_id)` and returns the
    /// 16-byte response payload (padded with zeros for short replies).
    pub fn request_raw(
        &mut self,
        feature_index: u8,
        feature_id_for_err: u16,
        function_id: u8,
        params: &[u8],
    ) -> Result<[u8; 16]> {
        if params.len() > 16 {
            return Err(Error::InvalidArgument(format!(
                "HID++ 2.0 payload too large: {} bytes (max 16)",
                params.len()
            )));
        }

        self.transport.drain()?;

        let sw_id = self.next_sw_id();
        let mut packet = [0u8; LONG_REPORT_LEN];
        packet[0] = REPORT_ID_LONG;
        packet[1] = self.device_index;
        packet[2] = feature_index;
        packet[3] = ((function_id & 0x0F) << 4) | (sw_id & 0x0F);
        packet[4..4 + params.len()].copy_from_slice(params);

        self.transport.write_report(&packet)?;

        let deadline = Instant::now() + self.timeout;
        let mut buf = [0u8; MAX_REPORT_LEN];

        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(Error::Timeout);
            }

            let n = self.transport.read_report(&mut buf, remaining)?;
            if n < SHORT_REPORT_LEN {
                continue;
            }
            let report_id = buf[0];
            if report_id != REPORT_ID_SHORT && report_id != REPORT_ID_LONG {
                continue;
            }
            if buf[1] != self.device_index {
                continue;
            }

            let resp_feat_idx = buf[2];

            // HID++ 1.0 error (e.g. device offline / unknown receiver slot)
            if resp_feat_idx == HIDPP10_ERROR_SUB_ID {
                let err_code = buf[5];
                // 0x09 = resource error / device offline on Lightspeed receiver
                if err_code == 0x09 || err_code == 0x08 {
                    return Err(Error::DeviceOffline);
                }
                return Err(Error::Protocol {
                    feature_id: feature_id_for_err,
                    code: HidppErrorCode::from(err_code),
                });
            }

            // HID++ 2.0 error response: [rep_id, dev_idx, 0xFF, req_feat_idx, (func<<4)|sw_id, err_code]
            if resp_feat_idx == HIDPP20_ERROR_FEATURE_IDX {
                if buf[3] == feature_index && (buf[4] & 0x0F) == sw_id {
                    return Err(Error::Protocol {
                        feature_id: feature_id_for_err,
                        code: HidppErrorCode::from(buf[5]),
                    });
                }
                continue;
            }

            let resp_func = (buf[3] >> 4) & 0x0F;
            let resp_sw_id = buf[3] & 0x0F;
            if resp_feat_idx == feature_index && resp_func == (function_id & 0x0F) && resp_sw_id == sw_id {
                let mut payload = [0u8; 16];
                let payload_len = (n - 4).min(16);
                payload[..payload_len].copy_from_slice(&buf[4..4 + payload_len]);
                return Ok(payload);
            }
        }
    }

    /// Pings the device via `IRoot::getProtocolVersion (0x0000, func 1)` and returns `(major, minor)`.
    pub fn ping(&mut self) -> Result<(u8, u8)> {
        const PING_MARKER: u8 = 0xAA;
        let resp = self.request_raw(0, feature_id::IROOT, 1, &[0x00, 0x00, PING_MARKER])?;
        if resp[2] != PING_MARKER {
            return Err(Error::DeviceOffline);
        }
        Ok((resp[0], resp[1]))
    }

    /// Resolves a HID++ 2.0 feature ID to its runtime feature index via `IRoot::getFeature`.
    /// Results are cached for the lifetime of the session.
    pub fn resolve_feature(&mut self, feat_id: u16) -> Result<Option<u8>> {
        if let Some(&cached) = self.feature_cache.get(&feat_id) {
            return Ok(cached);
        }
        let id_bytes = feat_id.to_be_bytes();
        let resp = self.request_raw(0, feature_id::IROOT, 0, &[id_bytes[0], id_bytes[1], 0x00])?;
        let index = if resp[0] == 0 { None } else { Some(resp[0]) };
        self.feature_cache.insert(feat_id, index);
        Ok(index)
    }

    /// Calls a function on a feature identified by its 16-bit HID++ 2.0 `feature_id`.
    pub fn request(&mut self, feat_id: u16, function_id: u8, params: &[u8]) -> Result<[u8; 16]> {
        let index = self
            .resolve_feature(feat_id)?
            .ok_or(Error::FeatureUnsupported(feat_id))?;
        self.request_raw(index, feat_id, function_id, params)
    }

    /// Enumerates all features exposed by the device using `IFeatureSet (0x0001)`.
    pub fn list_features(&mut self) -> Result<Vec<FeatureEntry>> {
        let count_resp = self.request(feature_id::IFEATURE_SET, 0, &[])?;
        let count = count_resp[0];
        let mut entries = Vec::with_capacity(usize::from(count));

        for idx in 0..=count {
            let resp = self.request(feature_id::IFEATURE_SET, 1, &[idx])?;
            let id = u16::from_be_bytes([resp[0], resp[1]]);
            entries.push(FeatureEntry {
                index: idx,
                id,
                feature_type: resp[2],
                version: resp[3],
            });
            if idx > 0 {
                self.feature_cache.insert(id, Some(idx));
            }
        }
        Ok(entries)
    }
}

/// Returns a human-readable name for known HID++ 2.0 feature IDs.
pub fn feature_name(id: u16) -> &'static str {
    match id {
        0x0000 => "IRoot",
        0x0001 => "IFeatureSet",
        0x0003 => "DeviceFwVersion",
        0x0005 => "DeviceTypeAndName",
        0x0007 => "DeviceFriendlyName",
        0x0020 => "ConfigChange",
        0x1000 => "BatteryStatus",
        0x1001 => "BatteryVoltage",
        0x1004 => "UnifiedBattery",
        0x1806 => "ConfigurableDeviceProperties",
        0x1814 => "ChangeHost",
        0x1B04 => "SpecialKeysMseButtons",
        0x1D4B => "WirelessDeviceStatus",
        0x2121 => "HiResWheel",
        0x2201 => "AdjustableDpi",
        0x2202 => "ExtendedAdjustableDpi",
        0x2250 => "AnalysisMode",
        0x8060 => "ReportRate",
        0x8061 => "ExtendedAdjustableReportRate",
        0x8070 => "ColorLedEffects",
        0x8071 => "RgbEffects",
        0x8100 => "OnboardProfiles",
        0x8110 => "MouseButtonSpy",
        0x8300 => "Sidetone",
        _ => "Unknown",
    }
}

#[cfg(test)]
pub(crate) mod test_util {
    use std::collections::VecDeque;
    use std::time::Duration;

    use crate::error::{Error, Result};
    use crate::transport::Transport;

    /// Deterministic in-memory transport for unit-testing HID++ 2.0 protocol flows.
    pub struct MockTransport {
        pub sent: Vec<Vec<u8>>,
        pub responses: VecDeque<Vec<u8>>,
    }

    impl MockTransport {
        pub fn new(responses: Vec<Vec<u8>>) -> Self {
            Self {
                sent: Vec::new(),
                responses: responses.into(),
            }
        }
    }

    impl Transport for MockTransport {
        fn write_report(&mut self, report: &[u8]) -> Result<()> {
            self.sent.push(report.to_vec());
            Ok(())
        }

        fn read_report(&mut self, buf: &mut [u8], _timeout: Duration) -> Result<usize> {
            match self.responses.pop_front() {
                Some(pkt) => {
                    let len = pkt.len().min(buf.len());
                    buf[..len].copy_from_slice(&pkt[..len]);
                    Ok(len)
                }
                None => Err(Error::Timeout),
            }
        }

        fn drain(&mut self) -> Result<()> {
            Ok(())
        }
    }

    /// Helper to build a 20-byte HID++ 2.0 long response packet.
    pub fn long_resp(dev_idx: u8, feat_idx: u8, func_id: u8, sw_id: u8, payload: &[u8]) -> Vec<u8> {
        let mut pkt = vec![0u8; 20];
        pkt[0] = 0x11;
        pkt[1] = dev_idx;
        pkt[2] = feat_idx;
        pkt[3] = ((func_id & 0x0F) << 4) | (sw_id & 0x0F);
        let len = payload.len().min(16);
        pkt[4..4 + len].copy_from_slice(&payload[..len]);
        pkt
    }
}

#[cfg(test)]
mod tests {
    use super::test_util::{MockTransport, long_resp};
    use super::*;

    #[test]
    fn ping_skips_unsolicited_notifications_and_matches_sw_id() {
        let unsolicited = long_resp(0x01, 0x05, 0x00, 0x00, &[0x99]);
        let ping_reply = long_resp(0x01, 0x00, 0x01, 0x01, &[0x04, 0x02, 0xAA]);
        let transport = MockTransport::new(vec![unsolicited, ping_reply]);
        let mut session = HidppSession::new(transport, 0x01);

        let (major, minor) = session.ping().unwrap();
        assert_eq!((major, minor), (4, 2));
    }

    #[test]
    fn hidpp20_error_response_is_decoded() {
        let err_pkt = long_resp(0x01, 0xFF, 0x00, 0x00, &[]);
        // Build proper error packet: [0x11, 0x01, 0xFF, req_feat_idx=0x00, (func<<4)|sw_id=0x11, err=0x02]
        let mut pkt = err_pkt;
        pkt[3] = 0x00;
        pkt[4] = 0x11;
        pkt[5] = 0x02; // InvalidArgument

        let transport = MockTransport::new(vec![pkt]);
        let mut session = HidppSession::new(transport, 0x01);
        let err = session.ping().unwrap_err();
        match err {
            Error::Protocol { code, .. } => assert_eq!(code, HidppErrorCode::InvalidArgument),
            other => panic!("expected Protocol error, got {other:?}"),
        }
    }
}
