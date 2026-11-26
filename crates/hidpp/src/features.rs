//! HID++ 2.0 feature implementations: device info, battery, DPI, and polling rate.

use crate::error::{Error, Result};
use crate::protocol::{HidppSession, feature_id};
use crate::transport::Transport;

/// Battery charging state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChargingStatus {
    Discharging,
    Recharging,
    AlmostFull,
    Full,
    SlowRecharge,
    InvalidBattery,
    ThermalError,
    Unknown(u8),
}

impl std::fmt::Display for ChargingStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Discharging => write!(f, "Discharging"),
            Self::Recharging => write!(f, "Charging"),
            Self::AlmostFull => write!(f, "Almost Full"),
            Self::Full => write!(f, "Full"),
            Self::SlowRecharge => write!(f, "Slow Charging"),
            Self::InvalidBattery => write!(f, "Invalid Battery"),
            Self::ThermalError => write!(f, "Thermal Error"),
            Self::Unknown(v) => write!(f, "Unknown (0x{v:02X})"),
        }
    }
}

/// Battery reading from `0x1004` UnifiedBattery, `0x1001` BatteryVoltage, or `0x1000` BatteryStatus.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BatteryInfo {
    pub percentage: u8,
    pub charging: ChargingStatus,
    pub voltage_mv: Option<u16>,
}

/// Supported DPI range or discrete list reported by `0x2201` / `0x2202`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DpiSpec {
    Range { min: u16, max: u16, step: u16 },
    Discrete(Vec<u16>),
}

impl DpiSpec {
    /// Clamps and snaps a requested DPI value to the nearest valid value supported by the sensor.
    pub fn clamp_and_snap(&self, dpi: u16) -> u16 {
        match self {
            Self::Range { min, max, step } => {
                let clamped = dpi.clamp(*min, *max);
                let step = (*step).max(1);
                let offset = clamped - *min;
                let snapped_offset = ((offset + step / 2) / step) * step;
                (*min + snapped_offset).min(*max)
            }
            Self::Discrete(values) => {
                if values.is_empty() {
                    return dpi;
                }
                *values
                    .iter()
                    .min_by_key(|&&v| v.abs_diff(dpi))
                    .unwrap_or(&dpi)
            }
        }
    }

    pub fn min(&self) -> u16 {
        match self {
            Self::Range { min, .. } => *min,
            Self::Discrete(v) => v.iter().copied().min().unwrap_or(100),
        }
    }

    pub fn max(&self) -> u16 {
        match self {
            Self::Range { max, .. } => *max,
            Self::Discrete(v) => v.iter().copied().max().unwrap_or(25600),
        }
    }

    pub fn step(&self) -> u16 {
        match self {
            Self::Range { step, .. } => (*step).max(1),
            Self::Discrete(_) => 50,
        }
    }
}

/// Current DPI reading from `0x2201` / `0x2202`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DpiReading {
    pub current_x: u16,
    pub current_y: u16,
    pub default_dpi: u16,
}

/// Reads the device's human-readable name using `DeviceTypeAndName (0x0005)`.
pub fn read_device_name<T: Transport>(session: &mut HidppSession<T>) -> Result<String> {
    let count_resp = session.request(feature_id::DEVICE_TYPE_AND_NAME, 0, &[])?;
    let total_len = usize::from(count_resp[0]);
    let mut bytes = Vec::with_capacity(total_len);

    let mut offset = 0usize;
    while offset < total_len {
        let resp = session.request(feature_id::DEVICE_TYPE_AND_NAME, 1, &[offset as u8])?;
        let remaining = total_len - offset;
        let take = remaining.min(16);
        bytes.extend_from_slice(&resp[..take]);
        offset += take;
    }

    let trimmed = String::from_utf8_lossy(&bytes)
        .trim_end_matches('\0')
        .trim()
        .to_string();
    Ok(trimmed)
}

/// Reads battery status, automatically preferring `0x1004` UnifiedBattery,
/// falling back to `0x1001` BatteryVoltage, then `0x1000` BatteryStatus.
pub fn read_battery<T: Transport>(session: &mut HidppSession<T>) -> Result<BatteryInfo> {
    if session.resolve_feature(feature_id::UNIFIED_BATTERY)?.is_some() {
        let resp = session.request(feature_id::UNIFIED_BATTERY, 1, &[])?;
        let percentage = resp[0].min(100);
        let charging = match resp[2] {
            0 => ChargingStatus::Discharging,
            1 => ChargingStatus::Recharging,
            2 => ChargingStatus::AlmostFull,
            3 => ChargingStatus::Full,
            4 => ChargingStatus::SlowRecharge,
            5 => ChargingStatus::InvalidBattery,
            6 => ChargingStatus::ThermalError,
            other => ChargingStatus::Unknown(other),
        };
        return Ok(BatteryInfo {
            percentage,
            charging,
            voltage_mv: None,
        });
    }

    if session.resolve_feature(feature_id::BATTERY_VOLTAGE)?.is_some() {
        let resp = session.request(feature_id::BATTERY_VOLTAGE, 0, &[])?;
        let voltage_mv = u16::from_be_bytes([resp[0], resp[1]]);
        let flags = resp[2];
        let charging = if (flags & 0x80) != 0 {
            if (flags & 0x07) == 0x07 {
                ChargingStatus::Full
            } else {
                ChargingStatus::Recharging
            }
        } else {
            ChargingStatus::Discharging
        };
        let percentage = voltage_mv_to_percent(voltage_mv);
        return Ok(BatteryInfo {
            percentage,
            charging,
            voltage_mv: Some(voltage_mv),
        });
    }

    if session.resolve_feature(feature_id::BATTERY_STATUS)?.is_some() {
        let resp = session.request(feature_id::BATTERY_STATUS, 0, &[])?;
        let percentage = resp[0].min(100);
        let charging = match resp[2] {
            0 => ChargingStatus::Discharging,
            1 => ChargingStatus::Recharging,
            2 => ChargingStatus::AlmostFull,
            3 => ChargingStatus::Full,
            4 => ChargingStatus::SlowRecharge,
            5 => ChargingStatus::InvalidBattery,
            6 => ChargingStatus::ThermalError,
            other => ChargingStatus::Unknown(other),
        };
        return Ok(BatteryInfo {
            percentage,
            charging,
            voltage_mv: None,
        });
    }

    Err(Error::FeatureUnsupported(feature_id::UNIFIED_BATTERY))
}

/// Approximates Li-ion / LiPo state-of-charge percentage from millivolts (3500mV..4180mV).
pub(crate) fn voltage_mv_to_percent(mv: u16) -> u8 {
    // Piecewise linear discharge table typical for Logitech G wireless mice.
    const TABLE: [(u16, u8); 11] = [
        (4180, 100),
        (4080, 90),
        (3990, 80),
        (3920, 70),
        (3860, 60),
        (3820, 50),
        (3780, 40),
        (3750, 30),
        (3710, 20),
        (3640, 10),
        (3500, 0),
    ];
    if mv >= TABLE[0].0 {
        return 100;
    }
    if mv <= TABLE[TABLE.len() - 1].0 {
        return 0;
    }
    for window in TABLE.windows(2) {
        let (hi_v, hi_p) = window[0];
        let (lo_v, lo_p) = window[1];
        if mv >= lo_v && mv <= hi_v {
            let frac = u32::from(mv - lo_v) * u32::from(hi_p - lo_p) / u32::from(hi_v - lo_v);
            return lo_p + frac as u8;
        }
    }
    0
}

/// Parses the 14-byte DPI list payload returned by `0x2201::getSensorDpiList` or `0x2202`.
pub(crate) fn parse_dpi_list(payload: &[u8]) -> DpiSpec {
    // Byte 0 is sensor index; bytes 1..15 hold up to 7 big-endian u16 values.
    // A value > 0xE000 encodes a step size: step = val - 0xE000, placed between min and max.
    let mut values = Vec::new();
    let mut i = 1;
    while i + 1 < payload.len() {
        let v = u16::from_be_bytes([payload[i], payload[i + 1]]);
        if v == 0 {
            break;
        }
        values.push(v);
        i += 2;
    }

    if values.len() >= 3 && values[1] > 0xE000 {
        let min = values[0];
        let step = values[1] - 0xE000;
        let max = values[2];
        return DpiSpec::Range { min, max, step };
    }

    DpiSpec::Discrete(values)
}

/// Reads the supported DPI range/list from `0x2201` (Superlight 1) or `0x2202` (Superlight 2).
pub fn read_dpi_spec<T: Transport>(session: &mut HidppSession<T>) -> Result<DpiSpec> {
    if session.resolve_feature(feature_id::ADJUSTABLE_DPI)?.is_some() {
        let resp = session.request(feature_id::ADJUSTABLE_DPI, 1, &[0])?;
        return Ok(parse_dpi_list(&resp));
    }
    if session
        .resolve_feature(feature_id::EXTENDED_ADJUSTABLE_DPI)?
        .is_some()
    {
        let resp = session.request(feature_id::EXTENDED_ADJUSTABLE_DPI, 2, &[0])?;
        return Ok(parse_dpi_list(&resp));
    }
    Err(Error::FeatureUnsupported(feature_id::ADJUSTABLE_DPI))
}

/// Reads the current active DPI from `0x2201` or `0x2202`.
pub fn read_dpi<T: Transport>(session: &mut HidppSession<T>) -> Result<DpiReading> {
    if session.resolve_feature(feature_id::ADJUSTABLE_DPI)?.is_some() {
        let resp = session.request(feature_id::ADJUSTABLE_DPI, 2, &[0])?;
        let current = u16::from_be_bytes([resp[1], resp[2]]);
        let default_dpi = u16::from_be_bytes([resp[3], resp[4]]);
        return Ok(DpiReading {
            current_x: current,
            current_y: current,
            default_dpi,
        });
    }
    if session
        .resolve_feature(feature_id::EXTENDED_ADJUSTABLE_DPI)?
        .is_some()
    {
        let resp = session.request(feature_id::EXTENDED_ADJUSTABLE_DPI, 5, &[0])?;
        let dpi_x = u16::from_be_bytes([resp[1], resp[2]]);
        let dpi_y = u16::from_be_bytes([resp[3], resp[4]]);
        let dpi_y = if dpi_y == 0 { dpi_x } else { dpi_y };
        return Ok(DpiReading {
            current_x: dpi_x,
            current_y: dpi_y,
            default_dpi: dpi_x,
        });
    }
    Err(Error::FeatureUnsupported(feature_id::ADJUSTABLE_DPI))
}

/// Sets the live sensor DPI via `0x2201` or `0x2202`.
pub fn write_live_dpi<T: Transport>(session: &mut HidppSession<T>, dpi: u16) -> Result<()> {
    let bytes = dpi.to_be_bytes();
    if session.resolve_feature(feature_id::ADJUSTABLE_DPI)?.is_some() {
        session.request(feature_id::ADJUSTABLE_DPI, 3, &[0, bytes[0], bytes[1]])?;
        return Ok(());
    }
    if session
        .resolve_feature(feature_id::EXTENDED_ADJUSTABLE_DPI)?
        .is_some()
    {
        // Extended Adjustable DPI: [sensor, x_hi, x_lo, y_hi, y_lo, lod]
        session.request(
            feature_id::EXTENDED_ADJUSTABLE_DPI,
            6,
            &[0, bytes[0], bytes[1], bytes[0], bytes[1], 0],
        )?;
        return Ok(());
    }
    Err(Error::FeatureUnsupported(feature_id::ADJUSTABLE_DPI))
}

/// Supported polling rate bitmask decoding for `0x8060` (ReportRate).
pub(crate) fn decode_report_rate_list_8060(bitmask: u8) -> Vec<u16> {
    // Bit i corresponds to interval (i + 1) ms => rate = 1000 / (i + 1) Hz.
    let mut rates = Vec::new();
    for bit in 0..8u8 {
        if (bitmask & (1 << bit)) != 0 {
            let interval_ms = u16::from(bit) + 1;
            let hz = 1000 / interval_ms;
            rates.push(hz);
        }
    }
    rates.sort_unstable();
    rates
}

/// Converts a polling rate in Hz (125, 250, 500, 1000) to the 1-byte interval in ms used by `0x8060` and `0x8100`.
pub fn hz_to_interval_ms(hz: u16) -> Result<u8> {
    match hz {
        1000 => Ok(1),
        500 => Ok(2),
        250 => Ok(4),
        125 => Ok(8),
        _ => Err(Error::InvalidArgument(format!(
            "unsupported polling rate {hz} Hz for standard report rate (expected 125, 250, 500, or 1000)"
        ))),
    }
}

/// Converts a 1-byte interval in ms (`1, 2, 4, 8`) back to Hz.
pub fn interval_ms_to_hz(interval_ms: u8) -> u16 {
    if interval_ms == 0 {
        return 1000;
    }
    1000 / u16::from(interval_ms)
}

const EXTENDED_RATES_HZ: [u16; 7] = [125, 250, 500, 1000, 2000, 4000, 8000];

/// Decodes the 16-bit bitmask used by `0x8061` ExtendedAdjustableReportRate.
pub(crate) fn decode_report_rate_list_8061(bitmask: u16) -> Vec<u16> {
    let mut rates = Vec::new();
    for (idx, &hz) in EXTENDED_RATES_HZ.iter().enumerate() {
        if (bitmask & (1 << idx)) != 0 {
            rates.push(hz);
        }
    }
    rates
}

/// Reads supported polling rates (in Hz) from `0x8061` (preferred) or `0x8060`.
pub fn read_supported_report_rates<T: Transport>(session: &mut HidppSession<T>) -> Result<Vec<u16>> {
    if session
        .resolve_feature(feature_id::EXTENDED_REPORT_RATE)?
        .is_some()
    {
        let resp = session.request(feature_id::EXTENDED_REPORT_RATE, 0, &[])?;
        let mask = u16::from_be_bytes([resp[0], resp[1]]);
        let rates = decode_report_rate_list_8061(mask);
        if !rates.is_empty() {
            return Ok(rates);
        }
    }
    if session.resolve_feature(feature_id::REPORT_RATE)?.is_some() {
        let resp = session.request(feature_id::REPORT_RATE, 0, &[])?;
        return Ok(decode_report_rate_list_8060(resp[0]));
    }
    Err(Error::FeatureUnsupported(feature_id::REPORT_RATE))
}

/// Reads the current live polling rate (in Hz) from `0x8061` or `0x8060`.
pub fn read_report_rate<T: Transport>(session: &mut HidppSession<T>) -> Result<u16> {
    if session
        .resolve_feature(feature_id::EXTENDED_REPORT_RATE)?
        .is_some()
    {
        let resp = session.request(feature_id::EXTENDED_REPORT_RATE, 2, &[])?;
        let idx = usize::from(resp[0]);
        if let Some(&hz) = EXTENDED_RATES_HZ.get(idx) {
            return Ok(hz);
        }
    }
    if session.resolve_feature(feature_id::REPORT_RATE)?.is_some() {
        let resp = session.request(feature_id::REPORT_RATE, 1, &[])?;
        return Ok(interval_ms_to_hz(resp[0]));
    }
    Err(Error::FeatureUnsupported(feature_id::REPORT_RATE))
}

/// Sets the live polling rate (in Hz) via `0x8061` or `0x8060`.
pub fn write_live_report_rate<T: Transport>(session: &mut HidppSession<T>, hz: u16) -> Result<()> {
    if session
        .resolve_feature(feature_id::EXTENDED_REPORT_RATE)?
        .is_some()
    {
        let idx = EXTENDED_RATES_HZ
            .iter()
            .position(|&r| r == hz)
            .ok_or_else(|| Error::InvalidArgument(format!("unsupported polling rate {hz} Hz")))?;
        session.request(feature_id::EXTENDED_REPORT_RATE, 3, &[idx as u8])?;
        return Ok(());
    }
    if session.resolve_feature(feature_id::REPORT_RATE)?.is_some() {
        let interval_ms = hz_to_interval_ms(hz)?;
        session.request(feature_id::REPORT_RATE, 2, &[interval_ms])?;
        return Ok(());
    }
    Err(Error::FeatureUnsupported(feature_id::REPORT_RATE))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_superlight_dpi_range() {
        // Superlight 1: sensor 0, min=100 (0x0064), step=50 (0xE032), max=25600 (0x6400)
        let payload = [0x00, 0x00, 0x64, 0xE0, 0x32, 0x64, 0x00, 0x00, 0x00];
        let spec = parse_dpi_list(&payload);
        assert_eq!(
            spec,
            DpiSpec::Range {
                min: 100,
                max: 25600,
                step: 50
            }
        );
        assert_eq!(spec.clamp_and_snap(820), 800);
        assert_eq!(spec.clamp_and_snap(830), 850);
        assert_eq!(spec.clamp_and_snap(50), 100);
        assert_eq!(spec.clamp_and_snap(30000), 25600);
    }

    #[test]
    fn decode_standard_and_extended_report_rates() {
        // Bitmask 0x8B = bits 0 (1ms=1000Hz), 1 (2ms=500Hz), 3 (4ms=250Hz), 7 (8ms=125Hz)
        assert_eq!(decode_report_rate_list_8060(0x8B), vec![125, 250, 500, 1000]);

        // Extended bitmask 0x7F = 125..8000 Hz
        assert_eq!(
            decode_report_rate_list_8061(0x7F),
            vec![125, 250, 500, 1000, 2000, 4000, 8000]
        );
    }

    #[test]
    fn voltage_to_percent_bounds() {
        assert_eq!(voltage_mv_to_percent(4200), 100);
        assert_eq!(voltage_mv_to_percent(3820), 50);
        assert_eq!(voltage_mv_to_percent(3400), 0);
    }
}
