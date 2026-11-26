//! HID++ 2.0 `0x8100` Onboard Profiles support.
//!
//! Logitech G mice (including G PRO X SUPERLIGHT) revert live DPI and report-rate
//! changes whenever they enter Onboard Mode or wake from sleep unless the active
//! onboard profile sector is updated with a valid CRC-16-CCITT and committed.

use crate::crc::{stamp_sector_crc, verify_sector_crc};
use crate::error::{Error, Result};
use crate::features::{hz_to_interval_ms, interval_ms_to_hz};
use crate::protocol::{HidppSession, feature_id};
use crate::transport::Transport;

/// Maximum number of DPI stages stored in a Logitech onboard profile.
pub const MAX_DPI_STAGES: usize = 5;

/// Operating mode of `0x8100` Onboard Profiles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnboardMode {
    Onboard = 1,
    Host = 2,
}

impl std::fmt::Display for OnboardMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Onboard => write!(f, "Onboard Memory"),
            Self::Host => write!(f, "Host (Software)"),
        }
    }
}

/// Hardware geometry returned by `0x8100::getDescription (func 0)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OnboardDescription {
    pub memory_model_id: u8,
    pub profile_format_id: u8,
    pub macro_format_id: u8,
    pub profile_count: u8,
    pub profile_count_oob: u8,
    pub button_count: u8,
    pub sector_count: u8,
    pub sector_size: u16,
}

/// Parsed view of an onboard profile sector (`profile_format_id` `0x03` / `0x05`),
/// preserving the full raw sector bytes so unrelated fields (button bindings,
/// RGB/LED settings, power modes, profile name) remain completely untouched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OnboardProfileSector {
    pub sector_id: u16,
    pub format_id: u8,
    pub raw: Vec<u8>,
}

impl OnboardProfileSector {
    /// Parses and CRC-verifies a raw profile sector.
    pub fn from_raw(sector_id: u16, format_id: u8, raw: Vec<u8>) -> Result<Self> {
        if raw.len() < 16 {
            return Err(Error::InvalidArgument(format!(
                "profile sector too short: {} bytes",
                raw.len()
            )));
        }
        verify_sector_crc(&raw)
            .map_err(|(expected, actual)| Error::CrcMismatch { expected, actual })?;
        Ok(Self {
            sector_id,
            format_id,
            raw,
        })
    }

    /// Reads the profile's polling rate in Hz.
    pub fn report_rate_hz(&self) -> Result<u16> {
        match self.format_id {
            // Formats 0x01 (G402), 0x02 (G303), 0x03 (G900 / G Pro), 0x04 (G305 / Superlight 1),
            // 0x05 (G502X / Superlight 2) all store report rate interval at byte 0.
            0x01..=0x05 => Ok(interval_ms_to_hz(self.raw[0])),
            other => Err(Error::UnsupportedProfileFormat(other)),
        }
    }

    /// Updates the profile's polling rate in Hz.
    pub fn set_report_rate_hz(&mut self, hz: u16) -> Result<()> {
        match self.format_id {
            0x01..=0x05 => {
                self.raw[0] = hz_to_interval_ms(hz)?;
                Ok(())
            }
            other => Err(Error::UnsupportedProfileFormat(other)),
        }
    }

    /// Reads the default (active) DPI stage index (`0..5`).
    pub fn default_dpi_index(&self) -> Result<usize> {
        match self.format_id {
            0x01..=0x05 => Ok(usize::from(self.raw[1]).min(MAX_DPI_STAGES - 1)),
            other => Err(Error::UnsupportedProfileFormat(other)),
        }
    }

    /// Sets the default (active) DPI stage index (`0..5`).
    pub fn set_default_dpi_index(&mut self, index: usize) -> Result<()> {
        if index >= MAX_DPI_STAGES {
            return Err(Error::InvalidArgument(format!(
                "DPI stage index {index} out of range (0..{MAX_DPI_STAGES})"
            )));
        }
        match self.format_id {
            0x01..=0x05 => {
                self.raw[1] = index as u8;
                Ok(())
            }
            other => Err(Error::UnsupportedProfileFormat(other)),
        }
    }

    /// Reads the 5 DPI stage entries (`0` means disabled stage).
    pub fn dpi_stages(&self) -> Result<[u16; MAX_DPI_STAGES]> {
        match self.format_id {
            // Formats 0x01..=0x04 (including G PRO X SUPERLIGHT): 5 × u16 little-endian at offset 0x03.
            0x01..=0x04 => {
                let mut stages = [0u16; MAX_DPI_STAGES];
                for (i, slot) in stages.iter_mut().enumerate() {
                    let off = 3 + i * 2;
                    *slot = u16::from_le_bytes([self.raw[off], self.raw[off + 1]]);
                }
                Ok(stages)
            }
            // Format 0x05 (Superlight 2 / G502X): 5 × (u16 x, u16 y, u8 flags) starting at offset 0x03.
            0x05 => {
                let mut stages = [0u16; MAX_DPI_STAGES];
                for (i, slot) in stages.iter_mut().enumerate() {
                    let off = 3 + i * 5;
                    if off + 1 < self.raw.len() {
                        *slot = u16::from_le_bytes([self.raw[off], self.raw[off + 1]]);
                    }
                }
                Ok(stages)
            }
            other => Err(Error::UnsupportedProfileFormat(other)),
        }
    }

    /// Updates the 5 DPI stage entries (`0` disables a stage).
    pub fn set_dpi_stages(&mut self, stages: [u16; MAX_DPI_STAGES]) -> Result<()> {
        if stages.iter().all(|&d| d == 0) {
            return Err(Error::InvalidArgument(
                "at least one DPI stage must be non-zero".to_string(),
            ));
        }
        match self.format_id {
            0x01..=0x04 => {
                for (i, &dpi) in stages.iter().enumerate() {
                    let off = 3 + i * 2;
                    let bytes = dpi.to_le_bytes();
                    self.raw[off] = bytes[0];
                    self.raw[off + 1] = bytes[1];
                }
                Ok(())
            }
            0x05 => {
                for (i, &dpi) in stages.iter().enumerate() {
                    let off = 3 + i * 5;
                    if off + 3 < self.raw.len() {
                        let bytes = dpi.to_le_bytes();
                        // Write both X and Y DPI to keep symmetric sensitivity.
                        self.raw[off] = bytes[0];
                        self.raw[off + 1] = bytes[1];
                        self.raw[off + 2] = bytes[0];
                        self.raw[off + 3] = bytes[1];
                    }
                }
                Ok(())
            }
            other => Err(Error::UnsupportedProfileFormat(other)),
        }
    }

    /// Returns the raw sector bytes with a freshly computed CRC-16-CCITT at the end.
    pub fn finalize_bytes(&mut self) -> &[u8] {
        stamp_sector_crc(&mut self.raw);
        &self.raw
    }
}

/// Reads the `0x8100` onboard profile description.
pub fn read_description<T: Transport>(session: &mut HidppSession<T>) -> Result<OnboardDescription> {
    let resp = session.request(feature_id::ONBOARD_PROFILES, 0, &[])?;
    Ok(OnboardDescription {
        memory_model_id: resp[0],
        profile_format_id: resp[1],
        macro_format_id: resp[2],
        profile_count: resp[3],
        profile_count_oob: resp[4],
        button_count: resp[5],
        sector_count: resp[6],
        sector_size: u16::from_be_bytes([resp[7], resp[8]]),
    })
}

/// Reads the current onboard mode (`Onboard` vs `Host`).
pub fn read_mode<T: Transport>(session: &mut HidppSession<T>) -> Result<OnboardMode> {
    let resp = session.request(feature_id::ONBOARD_PROFILES, 2, &[])?;
    match resp[0] {
        1 => Ok(OnboardMode::Onboard),
        2 => Ok(OnboardMode::Host),
        other => Err(Error::InvalidArgument(format!(
            "unexpected onboard mode value {other}"
        ))),
    }
}

/// Sets the onboard mode (`Onboard` vs `Host`).
pub fn write_mode<T: Transport>(session: &mut HidppSession<T>, mode: OnboardMode) -> Result<()> {
    session.request(feature_id::ONBOARD_PROFILES, 1, &[mode as u8])?;
    Ok(())
}

/// Reads the currently active onboard profile sector ID (e.g. `0x0001`).
pub fn read_current_profile<T: Transport>(session: &mut HidppSession<T>) -> Result<u16> {
    let resp = session.request(feature_id::ONBOARD_PROFILES, 4, &[])?;
    Ok(u16::from_be_bytes([resp[0], resp[1]]))
}

/// Selects the active onboard profile sector ID (also triggers the mouse to reload it).
pub fn write_current_profile<T: Transport>(
    session: &mut HidppSession<T>,
    profile_sector: u16,
) -> Result<()> {
    let bytes = profile_sector.to_be_bytes();
    session.request(feature_id::ONBOARD_PROFILES, 3, &[bytes[0], bytes[1]])?;
    Ok(())
}

/// Reads an entire `sector_size`-byte sector from onboard memory in 16-byte chunks.
/// Handles non-multiple-of-16 sector sizes (such as 255 bytes on G PRO X SUPERLIGHT)
/// by clamping the final read offset to `sector_size - 16` so it never reads past the sector boundary.
pub fn read_sector<T: Transport>(
    session: &mut HidppSession<T>,
    sector_id: u16,
    sector_size: u16,
) -> Result<Vec<u8>> {
    let size = usize::from(sector_size);
    let mut data = Vec::with_capacity(size);
    let sec_bytes = sector_id.to_be_bytes();

    let mut offset = 0usize;
    while offset < size {
        let remaining = size - offset;
        let (read_offset, skip) = if remaining < 16 && size >= 16 {
            let clamped = size - 16;
            (clamped, offset - clamped)
        } else {
            (offset, 0)
        };
        let off_bytes = (read_offset as u16).to_be_bytes();
        let chunk = session.request(
            feature_id::ONBOARD_PROFILES,
            5,
            &[sec_bytes[0], sec_bytes[1], off_bytes[0], off_bytes[1]],
        )?;
        let take = remaining.min(16 - skip);
        data.extend_from_slice(&chunk[skip..skip + take]);
        offset += take;
    }

    Ok(data)
}

/// Writes an entire sector to onboard memory (`memoryAddrWrite` -> `memoryWrite` -> `memoryWriteEnd`).
pub fn write_sector<T: Transport>(
    session: &mut HidppSession<T>,
    sector_id: u16,
    data: &[u8],
) -> Result<()> {
    let sec_bytes = sector_id.to_be_bytes();
    let count_bytes = (data.len() as u16).to_be_bytes();

    // func 6: memoryAddrWrite(sector_hi, sector_lo, offset_hi=0, offset_lo=0, count_hi, count_lo)
    session.request(
        feature_id::ONBOARD_PROFILES,
        6,
        &[
            sec_bytes[0],
            sec_bytes[1],
            0x00,
            0x00,
            count_bytes[0],
            count_bytes[1],
        ],
    )?;

    // func 7: memoryWrite(16 bytes per packet)
    for chunk in data.chunks(16) {
        let mut block = [0u8; 16];
        block[..chunk.len()].copy_from_slice(chunk);
        session.request(feature_id::ONBOARD_PROFILES, 7, &block)?;
    }

    // func 8: memoryWriteEnd() — commits sector and verifies CRC on the mouse MCU.
    session.request(feature_id::ONBOARD_PROFILES, 8, &[])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_valid_superlight_sector() -> Vec<u8> {
        let mut raw = vec![0xFFu8; 256];
        raw[0] = 1; // 1000 Hz
        raw[1] = 1; // active stage index 1 (800 DPI)
        raw[2] = 0; // switched DPI index
        let dpis: [u16; 5] = [400, 800, 1600, 3200, 0];
        for (i, dpi) in dpis.iter().enumerate() {
            let b = dpi.to_le_bytes();
            raw[3 + i * 2] = b[0];
            raw[3 + i * 2 + 1] = b[1];
        }
        // Arbitrary untouched button/RGB bytes in the middle of the sector
        raw[64] = 0xAB;
        raw[128] = 0xCD;
        stamp_sector_crc(&mut raw);
        raw
    }

    #[test]
    fn parse_modify_and_re_crc_format_03_sector_preserves_other_bytes() {
        let raw = make_valid_superlight_sector();
        let mut profile = OnboardProfileSector::from_raw(1, 0x03, raw).unwrap();

        assert_eq!(profile.report_rate_hz().unwrap(), 1000);
        assert_eq!(profile.default_dpi_index().unwrap(), 1);
        assert_eq!(profile.dpi_stages().unwrap(), [400, 800, 1600, 3200, 0]);

        profile.set_report_rate_hz(500).unwrap();
        profile.set_dpi_stages([800, 1200, 1600, 0, 0]).unwrap();
        profile.set_default_dpi_index(0).unwrap();

        let finalized = profile.finalize_bytes();
        assert!(verify_sector_crc(finalized).is_ok());

        // Re-parse to confirm round-trip and untouched bytes
        let reparsed = OnboardProfileSector::from_raw(1, 0x03, finalized.to_vec()).unwrap();
        assert_eq!(reparsed.report_rate_hz().unwrap(), 500);
        assert_eq!(reparsed.default_dpi_index().unwrap(), 0);
        assert_eq!(reparsed.dpi_stages().unwrap(), [800, 1200, 1600, 0, 0]);
        assert_eq!(reparsed.raw[64], 0xAB);
        assert_eq!(reparsed.raw[128], 0xCD);
    }

    #[test]
    fn corrupted_crc_is_rejected() {
        let mut raw = make_valid_superlight_sector();
        raw[50] ^= 0xFF;
        let err = OnboardProfileSector::from_raw(1, 0x03, raw).unwrap_err();
        assert!(matches!(err, Error::CrcMismatch { .. }));
    }

    #[test]
    fn format_04_superlight_255_byte_sector_roundtrip() {
        let mut raw = vec![0xFFu8; 255];
        raw[0] = 1; // 1000 Hz
        raw[1] = 2; // active stage index 2 (1600 DPI)
        raw[2] = 0;
        let dpis: [u16; 5] = [400, 800, 1600, 3200, 0];
        for (i, dpi) in dpis.iter().enumerate() {
            let b = dpi.to_le_bytes();
            raw[3 + i * 2] = b[0];
            raw[3 + i * 2 + 1] = b[1];
        }
        stamp_sector_crc(&mut raw);

        let mut profile = OnboardProfileSector::from_raw(1, 0x04, raw).unwrap();
        assert_eq!(profile.report_rate_hz().unwrap(), 1000);
        assert_eq!(profile.default_dpi_index().unwrap(), 2);
        assert_eq!(profile.dpi_stages().unwrap(), [400, 800, 1600, 3200, 0]);

        profile.set_report_rate_hz(500).unwrap();
        let finalized = profile.finalize_bytes();
        assert_eq!(finalized.len(), 255);
        assert!(verify_sector_crc(finalized).is_ok());
    }

    #[test]
    fn read_sector_clamps_offset_for_255_byte_sector() {
        use crate::protocol::test_util::{MockTransport, long_resp};

        // 1. resolve_feature(0x8100) -> feature index 8
        let mut responses = vec![long_resp(0x01, 0x00, 0x00, 0x01, &[0x08, 0x00, 0x00])];
        // 2. 16 reads: 15 full 16-byte chunks (0..240) + 1 clamped read at offset 239 (bytes 239..255)
        let mut sw = 2u8;
        for chunk_idx in 0..16u8 {
            let base = if chunk_idx < 15 {
                chunk_idx * 16
            } else {
                239
            };
            let mut payload = [0u8; 16];
            for (j, byte) in payload.iter_mut().enumerate() {
                *byte = base.wrapping_add(j as u8);
            }
            responses.push(long_resp(0x01, 0x08, 0x05, sw, &payload));
            sw = if sw >= 0x0F { 1 } else { sw + 1 };
        }

        let transport = MockTransport::new(responses);
        let mut session = HidppSession::new(transport, 0x01);
        let sector = read_sector(&mut session, 1, 255).unwrap();
        assert_eq!(sector.len(), 255);
        for (i, &b) in sector.iter().enumerate() {
            assert_eq!(b, i as u8, "mismatch at byte {i}");
        }
        // Verify last request asked for offset 239 (0x00EF), not 240 (0x00F0).
        let transport = session.into_transport();
        let last_req = transport.sent.last().unwrap();
        assert_eq!(u16::from_be_bytes([last_req[6], last_req[7]]), 239);
    }
}
