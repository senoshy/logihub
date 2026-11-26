//! High-level Logitech G mouse facade combining discovery, live HID++ 2.0 controls,
//! and onboard memory persistence.

use std::path::PathBuf;
use std::time::Duration;

use crate::discovery::{HidrawCandidate, find_hidpp_candidates};
use crate::error::{Error, Result};
use crate::features::{
    BatteryInfo, DpiReading, DpiSpec, read_battery, read_device_name, read_dpi, read_dpi_spec,
    read_report_rate, read_supported_report_rates, write_live_dpi, write_live_report_rate,
};
use crate::onboard::{
    MAX_DPI_STAGES, OnboardDescription, OnboardMode, OnboardProfileSector, read_current_profile,
    read_description, read_mode, read_sector, write_current_profile, write_mode, write_sector,
};
use crate::protocol::{FeatureEntry, HidppSession, feature_id};
use crate::transport::{HidrawTransport, Transport};

/// Device indices to probe on a Logitech HID++ endpoint:
/// `0xFF` for directly wired USB mice, `0x01..=0x06` for Lightspeed/Unifying/Bolt receiver slots.
const PROBE_DEVICE_INDICES: [u8; 7] = [0xFF, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06];

/// Complete snapshot of a connected Logitech G mouse's state.
#[derive(Debug, Clone)]
pub struct MouseStatus {
    pub hidraw_path: PathBuf,
    pub vendor_id: u16,
    pub product_id: u16,
    pub device_index: u8,
    pub protocol_version: (u8, u8),
    pub name: String,
    pub battery: Option<BatteryInfo>,
    pub dpi_spec: Option<DpiSpec>,
    pub dpi: Option<DpiReading>,
    pub supported_rates_hz: Vec<u16>,
    pub report_rate_hz: Option<u16>,
    pub onboard_mode: Option<OnboardMode>,
    pub active_profile_sector: Option<u16>,
    pub onboard_dpi_stages: Option<[u16; MAX_DPI_STAGES]>,
    pub onboard_dpi_index: Option<usize>,
    pub onboard_report_rate_hz: Option<u16>,
}

#[derive(Default)]
struct OnboardSummary {
    mode: Option<OnboardMode>,
    active_profile_sector: Option<u16>,
    dpi_stages: Option<[u16; MAX_DPI_STAGES]>,
    dpi_index: Option<usize>,
    report_rate_hz: Option<u16>,
}

/// High-level controller for a connected Logitech HID++ 2.0 mouse.
pub struct LogitechMouse<T: Transport = HidrawTransport> {
    session: HidppSession<T>,
    hidraw_path: PathBuf,
    vendor_id: u16,
    product_id: u16,
    protocol_version: (u8, u8),
}

impl LogitechMouse<HidrawTransport> {
    /// Automatically discovers and connects to the first reachable Logitech HID++ 2.0 mouse.
    pub fn connect_first() -> Result<Self> {
        let candidates = find_hidpp_candidates()?;
        if candidates.is_empty() {
            return Err(Error::NoDeviceFound);
        }

        let mut last_err = Error::NoDeviceFound;
        for candidate in candidates {
            match Self::connect_candidate(&candidate) {
                Ok(mouse) => return Ok(mouse),
                Err(err) => last_err = err,
            }
        }
        Err(last_err)
    }

    fn connect_candidate(candidate: &HidrawCandidate) -> Result<Self> {
        let mut transport = HidrawTransport::open(&candidate.path)?;

        for &dev_idx in &PROBE_DEVICE_INDICES {
            let mut session = HidppSession::new(transport, dev_idx);
            session.set_timeout(Duration::from_millis(250));
            match session.ping() {
                Ok((major, minor)) if major >= 2 => {
                    session.set_timeout(Duration::from_millis(1000));
                    return Ok(Self {
                        session,
                        hidraw_path: candidate.path.clone(),
                        vendor_id: candidate.vendor_id,
                        product_id: candidate.product_id,
                        protocol_version: (major, minor),
                    });
                }
                _ => {
                    transport = session.into_transport();
                }
            }
        }

        Err(Error::DeviceOffline)
    }
}

impl<T: Transport> LogitechMouse<T> {
    /// Creates a `LogitechMouse` from an existing session (useful for unit tests).
    pub fn from_session(
        session: HidppSession<T>,
        hidraw_path: PathBuf,
        vendor_id: u16,
        product_id: u16,
        protocol_version: (u8, u8),
    ) -> Self {
        Self {
            session,
            hidraw_path,
            vendor_id,
            product_id,
            protocol_version,
        }
    }

    pub fn session_mut(&mut self) -> &mut HidppSession<T> {
        &mut self.session
    }

    /// Lists all HID++ 2.0 features supported by the connected device.
    pub fn inspect_features(&mut self) -> Result<Vec<FeatureEntry>> {
        self.session.list_features()
    }

    /// Reads a full status snapshot from the mouse.
    pub fn status(&mut self) -> Result<MouseStatus> {
        let name = read_device_name(&mut self.session)
            .unwrap_or_else(|_| format!("Logitech Device {:04X}:{:04X}", self.vendor_id, self.product_id));
        let battery = read_battery(&mut self.session).ok();
        let dpi_spec = read_dpi_spec(&mut self.session).ok();
        let dpi = read_dpi(&mut self.session).ok();
        let supported_rates_hz = read_supported_report_rates(&mut self.session).unwrap_or_default();
        let report_rate_hz = read_report_rate(&mut self.session).ok();

        let onboard = self.read_onboard_summary();

        Ok(MouseStatus {
            hidraw_path: self.hidraw_path.clone(),
            vendor_id: self.vendor_id,
            product_id: self.product_id,
            device_index: self.session.device_index(),
            protocol_version: self.protocol_version,
            name,
            battery,
            dpi_spec,
            dpi,
            supported_rates_hz,
            report_rate_hz,
            onboard_mode: onboard.mode,
            active_profile_sector: onboard.active_profile_sector,
            onboard_dpi_stages: onboard.dpi_stages,
            onboard_dpi_index: onboard.dpi_index,
            onboard_report_rate_hz: onboard.report_rate_hz,
        })
    }

    fn read_onboard_summary(&mut self) -> OnboardSummary {
        let has_onboard = self
            .session
            .resolve_feature(feature_id::ONBOARD_PROFILES)
            .ok()
            .flatten()
            .is_some();
        if !has_onboard {
            return OnboardSummary::default();
        }

        let mode = read_mode(&mut self.session).ok();
        let desc = read_description(&mut self.session).ok();
        let active_sector = read_current_profile(&mut self.session).ok();

        if let (Some(desc), Some(sector_id)) = (desc, active_sector)
            && let Ok(profile) = self.load_profile_sector(sector_id, desc)
        {
            return OnboardSummary {
                mode,
                active_profile_sector: Some(sector_id),
                dpi_stages: profile.dpi_stages().ok(),
                dpi_index: profile.default_dpi_index().ok(),
                report_rate_hz: profile.report_rate_hz().ok(),
            };
        }

        OnboardSummary {
            mode,
            active_profile_sector: active_sector,
            ..Default::default()
        }
    }

    fn load_profile_sector(
        &mut self,
        sector_id: u16,
        desc: OnboardDescription,
    ) -> Result<OnboardProfileSector> {
        let raw = read_sector(&mut self.session, sector_id, desc.sector_size)?;
        OnboardProfileSector::from_raw(sector_id, desc.profile_format_id, raw)
    }

    /// Reads the active onboard profile sector, applies `mutate`, re-stamps CRC,
    /// writes it back to flash, and reloads the profile. Returns `Ok(true)` if persisted.
    fn update_active_onboard_profile<F>(&mut self, mutate: F) -> Result<bool>
    where
        F: FnOnce(&mut OnboardProfileSector) -> Result<()>,
    {
        if self
            .session
            .resolve_feature(feature_id::ONBOARD_PROFILES)?
            .is_none()
        {
            return Ok(false);
        }

        let Ok(desc) = read_description(&mut self.session) else {
            return Ok(false);
        };
        let Ok(sector_id) = read_current_profile(&mut self.session) else {
            return Ok(false);
        };
        let Ok(mut profile) = self.load_profile_sector(sector_id, desc) else {
            return Ok(false);
        };

        mutate(&mut profile)?;
        let bytes = profile.finalize_bytes().to_vec();
        write_sector(&mut self.session, sector_id, &bytes)?;
        // Re-select the profile so the mouse immediately reloads flash into RAM.
        let _ = write_current_profile(&mut self.session, sector_id);
        Ok(true)
    }

    /// Reads the `0x8100` onboard description if supported.
    pub fn onboard_description(&mut self) -> Result<OnboardDescription> {
        read_description(&mut self.session)
    }

    /// Switches between Onboard Memory mode and Host (Software) mode.
    pub fn set_onboard_mode(&mut self, mode: OnboardMode) -> Result<()> {
        write_mode(&mut self.session, mode)
    }

    /// Sets the active DPI.
    ///
    /// When `persist_onboard` is true and the mouse supports `0x8100` Onboard Profiles,
    /// this updates the active DPI slot in the current onboard memory profile sector
    /// (with valid CRC-16-CCITT) and reloads it, AND also applies the live DPI via `0x2201`/`0x2202`.
    pub fn set_dpi(&mut self, dpi: u16, persist_onboard: bool) -> Result<u16> {
        let snapped = match read_dpi_spec(&mut self.session) {
            Ok(spec) => spec.clamp_and_snap(dpi),
            Err(_) => dpi,
        };

        let persisted = if persist_onboard {
            self.update_active_onboard_profile(|profile| {
                let mut stages = profile.dpi_stages()?;
                let idx = profile.default_dpi_index()?;
                stages[idx] = snapped;
                profile.set_dpi_stages(stages)
            })?
        } else {
            false
        };

        // Always set live DPI as well so Host Mode (or mice without 0x8100) updates immediately.
        if let Err(err) = write_live_dpi(&mut self.session, snapped)
            && !persisted
        {
            return Err(err);
        }

        Ok(snapped)
    }

    /// Updates all 5 onboard DPI stages and the active stage index, persisting to onboard flash
    /// and applying the active stage's DPI live.
    pub fn set_dpi_stages(
        &mut self,
        stages: [u16; MAX_DPI_STAGES],
        active_index: usize,
    ) -> Result<[u16; MAX_DPI_STAGES]> {
        if active_index >= MAX_DPI_STAGES || stages[active_index] == 0 {
            return Err(Error::InvalidArgument(format!(
                "active DPI stage index {active_index} must point to a non-zero stage"
            )));
        }

        let spec = read_dpi_spec(&mut self.session).ok();
        let mut snapped = [0u16; MAX_DPI_STAGES];
        for (i, &val) in stages.iter().enumerate() {
            snapped[i] = if val == 0 {
                0
            } else if let Some(ref s) = spec {
                s.clamp_and_snap(val)
            } else {
                val
            };
        }

        let desc = read_description(&mut self.session)?;
        let sector_id = read_current_profile(&mut self.session)?;
        let mut profile = self.load_profile_sector(sector_id, desc)?;
        profile.set_dpi_stages(snapped)?;
        profile.set_default_dpi_index(active_index)?;
        let bytes = profile.finalize_bytes().to_vec();
        write_sector(&mut self.session, sector_id, &bytes)?;
        let _ = write_current_profile(&mut self.session, sector_id);
        let _ = write_live_dpi(&mut self.session, snapped[active_index]);

        Ok(snapped)
    }

    /// Sets the polling rate in Hz (e.g. 125, 250, 500, 1000, or 2000/4000/8000 on Superlight 2).
    ///
    /// When `persist_onboard` is true, updates the active onboard profile sector's `report_rate`
    /// byte with a fresh CRC-16-CCITT and reloads it, AND applies the live report rate.
    pub fn set_report_rate(&mut self, hz: u16, persist_onboard: bool) -> Result<()> {
        let supported = read_supported_report_rates(&mut self.session).unwrap_or_default();
        if !supported.is_empty() && !supported.contains(&hz) {
            return Err(Error::InvalidArgument(format!(
                "polling rate {hz} Hz not supported by this mouse (supported: {supported:?})"
            )));
        }

        let persisted = if persist_onboard {
            self.update_active_onboard_profile(|profile| profile.set_report_rate_hz(hz))?
        } else {
            false
        };

        if let Err(err) = write_live_report_rate(&mut self.session, hz)
            && !persisted
        {
            return Err(err);
        }

        Ok(())
    }
}
