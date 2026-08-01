#[cfg(debug_assertions)]
use darpc_game_client::DEBUG_UNSUPPORTED_CLIENT_BYPASS_ENVIRONMENT_VARIABLE;
use darpc_game_client::{CLIENT_VERSION_CODE, ClientExecutable, EXECUTABLE_SHA256};
use darpc_protocol::{Architecture, ComponentVersion, Hello, SUPPORTED_VERSIONS};
use std::{env, io};
use windows_sys::{
    Win32::{
        Foundation::FILETIME,
        System::{
            Com::CoCreateGuid,
            Threading::{GetCurrentProcess, GetProcessTimes},
        },
    },
    core::GUID,
};

pub(crate) fn hello() -> io::Result<Hello> {
    let (executable_fingerprint, client_version) = client_identity()?;

    Ok(Hello {
        protocol_versions: SUPPORTED_VERSIONS,
        dll_instance_id: instance_id()?,
        process_id: std::process::id(),
        process_creation_time: process_creation_time()?,
        architecture: architecture()?,
        dll_version: ComponentVersion {
            major: version_component(env!("CARGO_PKG_VERSION_MAJOR"))?,
            minor: version_component(env!("CARGO_PKG_VERSION_MINOR"))?,
            patch: version_component(env!("CARGO_PKG_VERSION_PATCH"))?,
        },
        executable_fingerprint,
        client_version,
    })
}

fn client_identity() -> io::Result<([u8; 32], u32)> {
    let executable_path = env::current_exe()?;
    match ClientExecutable::validate(&executable_path) {
        Ok(_) => Ok((EXECUTABLE_SHA256, CLIENT_VERSION_CODE)),
        Err(error) => {
            #[cfg(debug_assertions)]
            if env::var_os(DEBUG_UNSUPPORTED_CLIENT_BYPASS_ENVIRONMENT_VARIABLE).as_deref()
                == Some(std::ffi::OsStr::new("1"))
            {
                return Ok(([0; 32], 0));
            }

            Err(io::Error::new(io::ErrorKind::InvalidData, error))
        }
    }
}

fn instance_id() -> io::Result<[u8; 16]> {
    let mut guid = GUID::default();
    // SAFETY: CoCreateGuid writes one GUID to the valid output pointer and does
    // not require COM apartment initialization.
    let result = unsafe { CoCreateGuid(&raw mut guid) };
    if result != 0 {
        return Err(io::Error::other(format!(
            "CoCreateGuid failed with HRESULT 0x{:08X}",
            result as u32
        )));
    }

    let mut bytes = [0_u8; 16];
    bytes[0..4].copy_from_slice(&guid.data1.to_le_bytes());
    bytes[4..6].copy_from_slice(&guid.data2.to_le_bytes());
    bytes[6..8].copy_from_slice(&guid.data3.to_le_bytes());
    bytes[8..16].copy_from_slice(&guid.data4);
    Ok(bytes)
}

fn process_creation_time() -> io::Result<u64> {
    let mut creation = FILETIME::default();
    let mut exit = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();

    // SAFETY: GetCurrentProcess returns a valid pseudo-handle and every
    // FILETIME output pointer is writable for the duration of the call.
    if unsafe {
        GetProcessTimes(
            GetCurrentProcess(),
            &raw mut creation,
            &raw mut exit,
            &raw mut kernel,
            &raw mut user,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }

    Ok((u64::from(creation.dwHighDateTime) << 32) | u64::from(creation.dwLowDateTime))
}

fn architecture() -> io::Result<Architecture> {
    #[cfg(target_arch = "x86")]
    {
        Ok(Architecture::X86)
    }
    #[cfg(target_arch = "x86_64")]
    {
        Ok(Architecture::X86_64)
    }
    #[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
    {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "unsupported DLL architecture",
        ))
    }
}

fn version_component(value: &str) -> io::Result<u16> {
    value.parse().map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("invalid DLL version component `{value}`: {error}"),
        )
    })
}
