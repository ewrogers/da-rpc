//! Controllable DLL used only by the loader integration test.

use darpc_win32::lifecycle::{ABI_VERSION, Status};
use std::{env, thread, time::Duration};

const TEST_MODE_ENVIRONMENT_VARIABLE: &str = "DARPC_LOADER_TEST_MODE";
const TIMEOUT_DELAY: Duration = Duration::from_secs(12);

#[unsafe(no_mangle)]
pub extern "system" fn darpc_initialize(abi_version: u32) -> Status {
    if abi_version != ABI_VERSION {
        return Status::UNSUPPORTED_ABI_VERSION;
    }

    match test_mode().as_deref() {
        Some("init-fail") => Status::INTERNAL_ERROR,
        Some("init-timeout") => {
            thread::sleep(TIMEOUT_DELAY);
            Status::OK
        }
        _ => Status::OK,
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn darpc_shutdown(reserved: u32) -> Status {
    if reserved != 0 {
        return Status::INVALID_ARGUMENT;
    }

    match test_mode().as_deref() {
        Some("shutdown-fail") => Status::INTERNAL_ERROR,
        _ => Status::OK,
    }
}

fn test_mode() -> Option<String> {
    env::var(TEST_MODE_ENVIRONMENT_VARIABLE).ok()
}
