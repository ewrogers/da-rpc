use std::fmt;

#[cfg(windows)]
use std::io;

#[cfg(windows)]
use windows_sys::Win32::Foundation::{
    ERROR_ACCESS_DENIED, ERROR_FILE_NOT_FOUND, ERROR_PIPE_BUSY, ERROR_SEM_TIMEOUT,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) enum ErrorKind {
    InvalidArguments,
    #[cfg_attr(windows, allow(dead_code))]
    UnsupportedPlatform,
    PipeMissing,
    PipeBusy,
    AccessDenied,
    Timeout,
    Incompatible,
    Protocol,
    Io,
}

impl ErrorKind {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::InvalidArguments => "invalid_arguments",
            Self::UnsupportedPlatform => "unsupported_platform",
            Self::PipeMissing => "pipe_missing",
            Self::PipeBusy => "pipe_busy",
            Self::AccessDenied => "access_denied",
            Self::Timeout => "timeout",
            Self::Incompatible => "incompatible",
            Self::Protocol => "protocol",
            Self::Io => "io",
        }
    }

    pub(crate) const fn exit_code(self) -> u8 {
        match self {
            Self::InvalidArguments => 2,
            Self::UnsupportedPlatform => 3,
            Self::PipeMissing => 4,
            Self::PipeBusy => 5,
            Self::AccessDenied => 6,
            Self::Timeout => 7,
            Self::Incompatible => 8,
            Self::Protocol => 9,
            Self::Io => 10,
        }
    }
}

#[derive(Debug)]
pub(crate) struct ClientError {
    kind: ErrorKind,
    message: String,
    pid: Option<u32>,
}

impl ClientError {
    pub(crate) fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            pid: None,
        }
    }

    #[cfg(windows)]
    pub(crate) fn from_io(pid: u32, context: &str, error: io::Error) -> Self {
        let raw = error.raw_os_error().map(|value| value as u32);
        let (kind, message) = match raw {
            Some(ERROR_FILE_NOT_FOUND) => (
                ErrorKind::PipeMissing,
                format!("no daRPC pipe exists for PID {pid}; inject darpc.dll first"),
            ),
            Some(ERROR_PIPE_BUSY) => (
                ErrorKind::PipeBusy,
                format!(
                    "the daRPC pipe for PID {pid} is busy; disconnect darpcd.exe or another direct IPC client first"
                ),
            ),
            Some(ERROR_ACCESS_DENIED) => (
                ErrorKind::AccessDenied,
                format!("access to the daRPC pipe for PID {pid} was denied"),
            ),
            Some(ERROR_SEM_TIMEOUT) => (
                ErrorKind::Timeout,
                format!("the daRPC pipe for PID {pid} timed out"),
            ),
            _ if error.kind() == io::ErrorKind::TimedOut => (
                ErrorKind::Timeout,
                format!("{context} timed out for PID {pid}"),
            ),
            _ if error.kind() == io::ErrorKind::InvalidData => {
                (ErrorKind::Protocol, format!("{context}: {error}"))
            }
            _ => (ErrorKind::Io, format!("{context}: {error}")),
        };
        Self::new(kind, message).with_pid(pid)
    }

    pub(crate) const fn kind(&self) -> ErrorKind {
        self.kind
    }

    pub(crate) const fn pid(&self) -> Option<u32> {
        self.pid
    }

    pub(crate) fn message(&self) -> &str {
        &self.message
    }

    #[cfg_attr(not(windows), allow(dead_code))]
    pub(crate) const fn with_pid(mut self, pid: u32) -> Self {
        self.pid = Some(pid);
        self
    }
}

impl fmt::Display for ClientError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.message.fmt(formatter)
    }
}

impl std::error::Error for ClientError {}

pub(crate) type Result<T> = std::result::Result<T, ClientError>;

#[cfg(test)]
mod tests {
    use super::ErrorKind;
    use std::collections::BTreeSet;

    #[test]
    fn public_error_names_and_exit_codes_are_unique() {
        let kinds = [
            ErrorKind::InvalidArguments,
            ErrorKind::UnsupportedPlatform,
            ErrorKind::PipeMissing,
            ErrorKind::PipeBusy,
            ErrorKind::AccessDenied,
            ErrorKind::Timeout,
            ErrorKind::Incompatible,
            ErrorKind::Protocol,
            ErrorKind::Io,
        ];
        let mut names = BTreeSet::new();
        let mut exit_codes = BTreeSet::new();
        for kind in kinds {
            assert!(names.insert(kind.as_str()));
            assert!(exit_codes.insert(kind.exit_code()));
        }
    }
}
