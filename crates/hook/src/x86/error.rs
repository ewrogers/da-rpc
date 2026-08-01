use std::{error::Error, fmt, io};

#[derive(Debug)]
pub enum DetourError {
    AlreadyReserved {
        target: usize,
    },
    InvalidCodeRange,
    InvalidInstruction {
        address: usize,
    },
    EarlyTerminatingInstruction {
        address: usize,
    },
    PrologueTooLong,
    Relocation(String),
    RegistryPoisoned,
    InvalidState,
    TargetChanged,
    TooManyThreads {
        limit: usize,
    },
    BusyInstructionPointer {
        thread_id: u32,
        instruction_pointer: usize,
    },
    ActiveDetourCalls {
        count: u32,
    },
    Windows {
        operation: &'static str,
        source: io::Error,
    },
    CommitFailed {
        operation: &'static str,
        source: io::Error,
        rollback: Option<io::Error>,
    },
}

impl DetourError {
    pub(super) fn windows(operation: &'static str) -> Self {
        Self::Windows {
            operation,
            source: io::Error::last_os_error(),
        }
    }

    pub fn is_transient(&self) -> bool {
        matches!(
            self,
            Self::BusyInstructionPointer { .. } | Self::ActiveDetourCalls { .. }
        )
    }
}

impl fmt::Display for DetourError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AlreadyReserved { target } => {
                write!(
                    formatter,
                    "target 0x{target:08X} already has a detour reservation"
                )
            }
            Self::InvalidCodeRange => formatter.write_str("code range is empty or wraps"),
            Self::InvalidInstruction { address } => {
                write!(formatter, "invalid x86 instruction at 0x{address:08X}")
            }
            Self::EarlyTerminatingInstruction { address } => write!(
                formatter,
                "target terminates before a complete detour jump at 0x{address:08X}"
            ),
            Self::PrologueTooLong => {
                formatter.write_str("target prologue exceeds the relocation limit")
            }
            Self::Relocation(message) => {
                write!(formatter, "instruction relocation failed: {message}")
            }
            Self::RegistryPoisoned => {
                formatter.write_str("detour reservation registry is poisoned")
            }
            Self::InvalidState => formatter.write_str("detour is not in the required state"),
            Self::TargetChanged => {
                formatter.write_str("target bytes changed after detour preparation")
            }
            Self::TooManyThreads { limit } => {
                write!(
                    formatter,
                    "process has more than {limit} enlistable threads"
                )
            }
            Self::BusyInstructionPointer {
                thread_id,
                instruction_pointer,
            } => write!(
                formatter,
                "thread {thread_id} is executing protected code at 0x{instruction_pointer:08X}"
            ),
            Self::ActiveDetourCalls { count } => {
                write!(formatter, "{count} detour call(s) remain active")
            }
            Self::Windows { operation, source } => write!(formatter, "{operation}: {source}"),
            Self::CommitFailed {
                operation,
                source,
                rollback,
            } => {
                write!(formatter, "{operation}: {source}")?;
                if let Some(rollback) = rollback {
                    write!(formatter, "; rollback also failed: {rollback}")?;
                }
                Ok(())
            }
        }
    }
}

impl Error for DetourError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Windows { source, .. } | Self::CommitFailed { source, .. } => Some(source),
            _ => None,
        }
    }
}
