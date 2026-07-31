use std::fmt;

#[cfg(windows)]
use std::io;

#[cfg_attr(not(windows), allow(dead_code))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ErrorKind {
    InvalidArguments,
    #[cfg_attr(windows, allow(dead_code))]
    UnsupportedPlatform,
    InvalidDll,
    ProcessMissing,
    ProcessExited,
    AccessDenied,
    WrongArchitecture,
    AlreadyLoaded,
    Timeout,
    InitializationFailed,
    ShutdownFailed,
    RemoteOperationFailed,
    Internal,
    LaunchFailed,
}

impl ErrorKind {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::InvalidArguments => "invalid_arguments",
            Self::UnsupportedPlatform => "unsupported_platform",
            Self::InvalidDll => "invalid_dll",
            Self::ProcessMissing => "process_missing",
            Self::ProcessExited => "process_exited",
            Self::AccessDenied => "access_denied",
            Self::WrongArchitecture => "wrong_architecture",
            Self::AlreadyLoaded => "already_loaded",
            Self::Timeout => "timeout",
            Self::InitializationFailed => "initialization_failed",
            Self::ShutdownFailed => "shutdown_failed",
            Self::RemoteOperationFailed => "remote_operation_failed",
            Self::Internal => "internal",
            Self::LaunchFailed => "launch_failed",
        }
    }

    pub(crate) const fn exit_code(self) -> u8 {
        match self {
            Self::InvalidArguments => 2,
            Self::UnsupportedPlatform => 3,
            Self::InvalidDll => 4,
            Self::ProcessMissing => 5,
            Self::ProcessExited => 6,
            Self::AccessDenied => 7,
            Self::WrongArchitecture => 8,
            Self::AlreadyLoaded => 9,
            Self::Timeout => 10,
            Self::InitializationFailed => 11,
            Self::ShutdownFailed => 12,
            Self::RemoteOperationFailed => 13,
            Self::Internal => 14,
            Self::LaunchFailed => 15,
        }
    }
}

#[derive(Debug)]
pub(crate) struct LoaderError {
    kind: ErrorKind,
    message: String,
    pid: Option<u32>,
}

impl LoaderError {
    pub(crate) fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            pid: None,
        }
    }

    #[cfg(windows)]
    pub(crate) fn from_io(
        default_kind: ErrorKind,
        context: impl fmt::Display,
        error: io::Error,
    ) -> Self {
        let kind = if error.raw_os_error() == Some(5) {
            ErrorKind::AccessDenied
        } else {
            default_kind
        };

        Self::new(kind, format!("{context}: {error}"))
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

    #[cfg(any(windows, test))]
    pub(crate) fn with_pid(mut self, pid: u32) -> Self {
        self.pid = Some(pid);
        self
    }
}

impl fmt::Display for LoaderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.message.fmt(formatter)
    }
}

impl std::error::Error for LoaderError {}

pub(crate) type Result<T> = std::result::Result<T, LoaderError>;

#[cfg(test)]
mod tests {
    use super::ErrorKind;
    use std::collections::BTreeSet;

    #[test]
    fn public_error_kinds_and_exit_codes_are_unique() {
        let cases = [
            (ErrorKind::InvalidArguments, "invalid_arguments", 2),
            (ErrorKind::UnsupportedPlatform, "unsupported_platform", 3),
            (ErrorKind::InvalidDll, "invalid_dll", 4),
            (ErrorKind::ProcessMissing, "process_missing", 5),
            (ErrorKind::ProcessExited, "process_exited", 6),
            (ErrorKind::AccessDenied, "access_denied", 7),
            (ErrorKind::WrongArchitecture, "wrong_architecture", 8),
            (ErrorKind::AlreadyLoaded, "already_loaded", 9),
            (ErrorKind::Timeout, "timeout", 10),
            (ErrorKind::InitializationFailed, "initialization_failed", 11),
            (ErrorKind::ShutdownFailed, "shutdown_failed", 12),
            (
                ErrorKind::RemoteOperationFailed,
                "remote_operation_failed",
                13,
            ),
            (ErrorKind::Internal, "internal", 14),
            (ErrorKind::LaunchFailed, "launch_failed", 15),
        ];
        let mut names = BTreeSet::new();
        let mut exit_codes = BTreeSet::new();

        for (kind, expected_name, expected_exit_code) in cases {
            assert_eq!(kind.as_str(), expected_name);
            assert_eq!(kind.exit_code(), expected_exit_code);
            assert!(names.insert(kind.as_str()));
            assert!(exit_codes.insert(kind.exit_code()));
        }
    }
}
