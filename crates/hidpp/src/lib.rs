//! Pure-Rust Logitech HID++ 2.0 library for Linux `/dev/hidraw*`.

pub mod crc;
pub mod device;
pub mod discovery;
pub mod error;
pub mod features;
pub mod onboard;
pub mod protocol;
pub mod transport;

pub use device::{LogitechMouse, MouseStatus};
pub use error::{Error, HidppErrorCode, Result};
pub use features::{BatteryInfo, ChargingStatus, DpiReading, DpiSpec};
pub use onboard::{MAX_DPI_STAGES, OnboardDescription, OnboardMode, OnboardProfileSector};
pub use protocol::{FeatureEntry, feature_name};
