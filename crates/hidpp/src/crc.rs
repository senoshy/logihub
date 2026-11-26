//! CRC-16-CCITT implementation used by Logitech HID++ 2.0 `0x8100` onboard memory sectors.

const CRC16_POLY: u16 = 0x1021;
const CRC16_INIT: u16 = 0xFFFF;

/// Computes CRC-16-CCITT (FALSE: poly 0x1021, init 0xFFFF, no reflection, no final XOR).
pub fn crc16_ccitt(data: &[u8]) -> u16 {
    let mut crc = CRC16_INIT;
    for &byte in data {
        crc ^= u16::from(byte) << 8;
        for _ in 0..8 {
            if (crc & 0x8000) != 0 {
                crc = (crc << 1) ^ CRC16_POLY;
            } else {
                crc <<= 1;
            }
        }
    }
    crc
}

/// Verifies that the final 2 big-endian bytes of `sector` match the CRC-16-CCITT
/// of the preceding bytes.
pub fn verify_sector_crc(sector: &[u8]) -> Result<(), (u16, u16)> {
    if sector.len() < 2 {
        return Err((0, 0));
    }
    let payload_len = sector.len() - 2;
    let computed = crc16_ccitt(&sector[..payload_len]);
    let stored = u16::from_be_bytes([sector[payload_len], sector[payload_len + 1]]);
    if computed == stored {
        Ok(())
    } else {
        Err((computed, stored))
    }
}

/// Writes the CRC-16-CCITT of `sector[..len-2]` into the final 2 big-endian bytes.
pub fn stamp_sector_crc(sector: &mut [u8]) {
    if sector.len() < 2 {
        return;
    }
    let payload_len = sector.len() - 2;
    let crc = crc16_ccitt(&sector[..payload_len]);
    let bytes = crc.to_be_bytes();
    sector[payload_len] = bytes[0];
    sector[payload_len + 1] = bytes[1];
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc16_ccitt_standard_check_value() {
        // Standard CRC-16/CCITT-FALSE check value for ASCII "123456789" is 0x29B1.
        assert_eq!(crc16_ccitt(b"123456789"), 0x29B1);
    }

    #[test]
    fn stamp_and_verify_sector_roundtrip() {
        let mut sector = vec![0xFFu8; 256];
        sector[0] = 0x01; // 1000 Hz
        sector[1] = 0x01; // default DPI index
        stamp_sector_crc(&mut sector);
        assert!(verify_sector_crc(&sector).is_ok());

        // Flip a single bit and ensure verification fails.
        sector[10] ^= 0x01;
        assert!(verify_sector_crc(&sector).is_err());
    }
}
