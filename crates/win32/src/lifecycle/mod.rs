pub const ABI_VERSION: u32 = 1;
const ABI_VERSION_MASK: u32 = 0x0000_FFFF;
const HOOK_TIMING_FLAG: u32 = 1 << 16;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct InitializeOptions {
    hook_timing: bool,
}

impl InitializeOptions {
    #[must_use]
    pub const fn new() -> Self {
        Self { hook_timing: false }
    }

    #[must_use]
    pub const fn with_hook_timing(mut self, enabled: bool) -> Self {
        self.hook_timing = enabled;
        self
    }

    #[must_use]
    pub const fn hook_timing(self) -> bool {
        self.hook_timing
    }

    #[must_use]
    pub const fn encode(self) -> u32 {
        ABI_VERSION
            | if self.hook_timing {
                HOOK_TIMING_FLAG
            } else {
                0
            }
    }

    pub const fn decode(value: u32) -> Option<Self> {
        if value & ABI_VERSION_MASK != ABI_VERSION
            || value & !(ABI_VERSION_MASK | HOOK_TIMING_FLAG) != 0
        {
            return None;
        }
        Some(Self {
            hook_timing: value & HOOK_TIMING_FLAG != 0,
        })
    }
}

pub const INITIALIZE_EXPORT: &[u8] = b"darpc_initialize\0";
pub const SHUTDOWN_EXPORT: &[u8] = b"darpc_shutdown\0";

#[repr(transparent)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Status(u32);

impl Status {
    pub const OK: Self = Self(0);
    pub const UNSUPPORTED_ABI_VERSION: Self = Self(1);
    pub const INVALID_ARGUMENT: Self = Self(2);
    pub const INTERNAL_ERROR: Self = Self(3);
    pub const UNLOAD_UNSAFE: Self = Self(4);

    pub const fn as_u32(self) -> u32 {
        self.0
    }
}

pub type InitializeFn = unsafe extern "system" fn(options: u32) -> Status;

pub type ShutdownFn = unsafe extern "system" fn(reserved: u32) -> Status;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initialize_options_round_trip_and_reject_unknown_bits() {
        let options = InitializeOptions::new().with_hook_timing(true);
        assert_eq!(InitializeOptions::decode(options.encode()), Some(options));
        assert_eq!(
            InitializeOptions::decode(ABI_VERSION),
            Some(InitializeOptions::new())
        );
        assert_eq!(InitializeOptions::decode(2), None);
        assert_eq!(InitializeOptions::decode(ABI_VERSION | (1 << 17)), None);
    }
}
