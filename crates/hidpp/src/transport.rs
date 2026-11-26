//! Low-level HID transport abstraction and Linux `/dev/hidraw*` implementation.

use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::error::{Error, Result};

/// Maximum HID++ report length (Long HID++ report = 20 bytes; Very Long = 64 bytes).
pub const MAX_REPORT_LEN: usize = 64;

/// Low-level byte-oriented HID report transport.
pub trait Transport {
    /// Writes a raw HID report (including leading report ID byte).
    fn write_report(&mut self, report: &[u8]) -> Result<()>;

    /// Reads a raw HID report within `timeout`, returning the number of bytes read.
    fn read_report(&mut self, buf: &mut [u8], timeout: Duration) -> Result<usize>;

    /// Drains any buffered unsolicited reports currently pending on the endpoint.
    fn drain(&mut self) -> Result<()>;
}

/// Direct Linux `/dev/hidrawN` transport using `O_NONBLOCK` + `poll(2)`.
pub struct HidrawTransport {
    file: File,
    path: PathBuf,
}

impl HidrawTransport {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path_buf = path.as_ref().to_path_buf();
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(&path_buf)
            .map_err(|err| {
                if err.kind() == std::io::ErrorKind::PermissionDenied {
                    Error::PermissionDenied {
                        path: path_buf.display().to_string(),
                    }
                } else {
                    Error::Io(err)
                }
            })?;
        Ok(Self {
            file,
            path: path_buf,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Transport for HidrawTransport {
    fn write_report(&mut self, report: &[u8]) -> Result<()> {
        let written = self.file.write(report)?;
        if written != report.len() {
            return Err(Error::Io(std::io::Error::new(
                std::io::ErrorKind::WriteZero,
                format!("short write to {}: {written}/{}", self.path.display(), report.len()),
            )));
        }
        Ok(())
    }

    fn read_report(&mut self, buf: &mut [u8], timeout: Duration) -> Result<usize> {
        let deadline = Instant::now() + timeout;
        let fd = self.file.as_raw_fd();

        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            let timeout_ms = remaining.as_millis().min(i32::MAX as u128) as i32;

            let mut pollfd = libc::pollfd {
                fd,
                events: libc::POLLIN,
                revents: 0,
            };

            // SAFETY: `pollfd` is a valid pointer to a single `libc::pollfd` struct.
            let rc = unsafe { libc::poll(&mut pollfd, 1, timeout_ms) };
            if rc < 0 {
                let err = std::io::Error::last_os_error();
                if err.kind() == std::io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(Error::Io(err));
            }
            if rc == 0 {
                return Err(Error::Timeout);
            }
            if (pollfd.revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL)) != 0 {
                return Err(Error::DeviceOffline);
            }

            match self.file.read(buf) {
                Ok(n) if n > 0 => return Ok(n),
                Ok(_) => return Err(Error::DeviceOffline),
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                    if Instant::now() >= deadline {
                        return Err(Error::Timeout);
                    }
                    continue;
                }
                Err(err) => return Err(Error::Io(err)),
            }
        }
    }

    fn drain(&mut self) -> Result<()> {
        let mut scratch = [0u8; MAX_REPORT_LEN];
        loop {
            match self.file.read(&mut scratch) {
                Ok(0) => return Ok(()),
                Ok(_) => continue,
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => return Ok(()),
                Err(err) => return Err(Error::Io(err)),
            }
        }
    }
}
