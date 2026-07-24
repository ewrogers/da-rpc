pub const ABI_VERSION: u32 = 1;

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

    pub const fn as_u32(self) -> u32 {
        self.0
    }
}

pub type InitializeFn = unsafe extern "system" fn(abi_version: u32) -> Status;

pub type ShutdownFn = unsafe extern "system" fn(reserved: u32) -> Status;
