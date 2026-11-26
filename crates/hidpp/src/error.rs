use thiserror::Error;

/// HID++ 2.0 protocol error codes returned in error responses (feature index `0xFF`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum HidppErrorCode {
    NoError = 0x00,
    Unknown = 0x01,
    InvalidArgument = 0x02,
    OutOfRange = 0x03,
    HwError = 0x04,
    LogitechInternal = 0x05,
    InvalidFeatureIndex = 0x06,
    InvalidFunctionId = 0x07,
    Busy = 0x08,
    Unsupported = 0x09,
    UnknownCode(u8),
}

impl From<u8> for HidppErrorCode {
    fn from(code: u8) -> Self {
        match code {
            0x00 => Self::NoError,
            0x01 => Self::Unknown,
            0x02 => Self::InvalidArgument,
            0x03 => Self::OutOfRange,
            0x04 => Self::HwError,
            0x05 => Self::LogitechInternal,
            0x06 => Self::InvalidFeatureIndex,
            0x07 => Self::InvalidFunctionId,
            0x08 => Self::Busy,
            0x09 => Self::Unsupported,
            other => Self::UnknownCode(other),
        }
    }
}

impl std::fmt::Display for HidppErrorCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoError => write!(f, "no error"),
            Self::Unknown => write!(f, "unknown error"),
            Self::InvalidArgument => write!(f, "invalid argument"),
            Self::OutOfRange => write!(f, "out of range"),
            Self::HwError => write!(f, "hardware error"),
            Self::LogitechInternal => write!(f, "Logitech internal error"),
            Self::InvalidFeatureIndex => write!(f, "invalid feature index"),
            Self::InvalidFunctionId => write!(f, "invalid function ID"),
            Self::Busy => write!(f, "device busy"),
            Self::Unsupported => write!(f, "unsupported"),
            Self::UnknownCode(c) => write!(f, "unknown error code 0x{c:02X}"),
        }
    }
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("no Logitech HID++ 2.0 device found (check USB connection and hidraw permissions)")]
    NoDeviceFound,

    #[error("permission denied opening {path}; install udev rules (see 99-logitech-hidpp.rules)")]
    PermissionDenied { path: String },

    #[error("device is offline or asleep (wake the mouse and retry)")]
    DeviceOffline,

    #[error("timed out waiting for HID++ response")]
    Timeout,

    #[error("HID++ 2.0 protocol error on feature 0x{feature_id:04X}: {code}")]
    Protocol {
        feature_id: u16,
        code: HidppErrorCode,
    },

    #[error("feature 0x{0:04X} is not supported by this device")]
    FeatureUnsupported(u16),

    #[error("invalid argument: {0}")]
    InvalidArgument(String),

    #[error("onboard profile CRC mismatch: expected 0x{expected:04X}, got 0x{actual:04X}")]
    CrcMismatch { expected: u16, actual: u16 },

    #[error("unsupported onboard profile format 0x{0:02X}")]
    UnsupportedProfileFormat(u8),
}

pub type Result<T> = std::result::Result<T, Error>;
