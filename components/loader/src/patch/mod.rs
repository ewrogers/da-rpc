#[cfg(any(windows, test))]
use darpc_game_client::{
    ALLOW_MULTIPLE_PATCHES, COMMAND_LINE_ENDPOINT_PATCHES, DEFAULT_RUNTIME_PATCHES,
    DISABLE_ENDPOINT_FALLBACK_PATCHES, LaunchPatch, SKIP_EXCHANGE_ALERTS_PATCHES,
    SKIP_INTRO_PATCHES, SKIP_NOTICE_PATCHES,
};
#[cfg(windows)]
use darpc_game_client::{BOOTSTRAP_SEQUENCE_PATCH, BootstrapSequencePatch};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct LaunchPatches {
    pub(crate) allow_multiple: bool,
    pub(crate) command_line_endpoint: bool,
    pub(crate) skip_exchange_alerts: bool,
    pub(crate) skip_intro: bool,
    pub(crate) skip_notice: bool,
}

impl LaunchPatches {
    #[cfg(windows)]
    pub(crate) const fn is_empty(self) -> bool {
        !self.allow_multiple
            && !self.command_line_endpoint
            && !self.skip_exchange_alerts
            && !self.skip_intro
            && !self.skip_notice
    }

    #[cfg(any(windows, test))]
    fn selected(self, include_defaults: bool) -> Vec<&'static LaunchPatch> {
        let mut selected = Vec::new();

        if include_defaults {
            selected.extend(DEFAULT_RUNTIME_PATCHES);
        }
        if self.allow_multiple {
            selected.extend(ALLOW_MULTIPLE_PATCHES);
        }
        if self.command_line_endpoint {
            selected.extend(COMMAND_LINE_ENDPOINT_PATCHES);
            selected.extend(DISABLE_ENDPOINT_FALLBACK_PATCHES);
        }
        if self.skip_exchange_alerts {
            selected.extend(SKIP_EXCHANGE_ALERTS_PATCHES);
        }
        if self.skip_intro {
            selected.extend(SKIP_INTRO_PATCHES);
        }
        if self.skip_notice {
            selected.extend(SKIP_NOTICE_PATCHES);
        }

        selected
    }
}

#[cfg(windows)]
mod platform {
    use super::*;
    use crate::{
        error::{ErrorKind, LoaderError, Result},
        process::TargetProcess,
        remote,
    };
    use std::{
        ffi::c_void,
        io,
        mem::{offset_of, size_of},
        os::windows::io::AsRawHandle,
        ptr,
    };
    use windows_sys::{
        Wdk::System::Threading::{NtQueryInformationProcess, ProcessBasicInformation},
        Win32::System::{
            Diagnostics::Debug::FlushInstructionCache,
            Memory::{
                PAGE_EXECUTE_READ, PAGE_EXECUTE_READWRITE, PAGE_PROTECTION_FLAGS, VirtualProtectEx,
            },
            Threading::{PEB, PROCESS_BASIC_INFORMATION},
        },
    };

    struct ResolvedPatch<'a> {
        definition: &'a LaunchPatch,
        address: usize,
    }

    struct ResolvedBootstrapPatch {
        binary_encrypt_call: usize,
        text_encrypt_call: usize,
        late_reset_call: usize,
        reset_sequence: usize,
        encrypt_packet: usize,
    }

    pub(crate) fn apply(
        process: &TargetProcess,
        selection: LaunchPatches,
        apply_default_patches: bool,
    ) -> Result<()> {
        if selection.is_empty() && !apply_default_patches {
            return Ok(());
        }

        let image_base = main_image_base(process)?;
        let selected = selection.selected(apply_default_patches);
        let resolved = resolve_at_base(process, image_base, &selected)?;
        let bootstrap = apply_default_patches
            .then(|| resolve_bootstrap(process, image_base, BOOTSTRAP_SEQUENCE_PATCH))
            .transpose()?;

        for patch in resolved {
            write_patch(process, &patch)?;
            eprintln!(
                "Applied {} at RVA 0x{:08X}",
                patch.definition.name, patch.definition.rva
            );
        }

        if let Some(bootstrap) = bootstrap {
            install_bootstrap_sequence_patch(process, &bootstrap)?;
        }

        Ok(())
    }

    fn main_image_base(process: &TargetProcess) -> Result<usize> {
        let mut information = PROCESS_BASIC_INFORMATION::default();
        let information_length =
            u32::try_from(size_of::<PROCESS_BASIC_INFORMATION>()).map_err(|_| {
                LoaderError::new(ErrorKind::Internal, "process information is too large")
            })?;
        let mut returned_length = 0;

        // SAFETY: `process` owns a valid queryable process handle,
        // `information` is writable for `information_length` bytes, and
        // `returned_length` is a writable output.
        let status = unsafe {
            NtQueryInformationProcess(
                process.handle().as_raw_handle(),
                ProcessBasicInformation,
                ptr::from_mut(&mut information).cast(),
                information_length,
                &mut returned_length,
            )
        };

        if status < 0 {
            return Err(LoaderError::new(
                ErrorKind::RemoteOperationFailed,
                format!(
                    "failed to locate the target process environment block: NTSTATUS=0x{status:08X}"
                ),
            ));
        }

        if information.PebBaseAddress.is_null() {
            return Err(LoaderError::new(
                ErrorKind::RemoteOperationFailed,
                "target process environment block address is null",
            ));
        }

        let image_base_offset = offset_of!(PEB, Reserved3)
            .checked_add(size_of::<*mut c_void>())
            .ok_or_else(|| LoaderError::new(ErrorKind::Internal, "PEB field offset overflow"))?;
        let image_base_address = (information.PebBaseAddress as usize)
            .checked_add(image_base_offset)
            .ok_or_else(|| LoaderError::new(ErrorKind::Internal, "PEB address overflow"))?;
        let bytes = remote::read(process, image_base_address, size_of::<usize>())?;
        let bytes: [u8; size_of::<usize>()] = bytes.try_into().map_err(|_| {
            LoaderError::new(ErrorKind::Internal, "image base pointer has the wrong size")
        })?;
        let image_base = usize::from_ne_bytes(bytes);

        if image_base == 0 {
            return Err(LoaderError::new(
                ErrorKind::RemoteOperationFailed,
                "target main-module base is null",
            ));
        }

        eprintln!("Located target main module at 0x{image_base:08X}");
        Ok(image_base)
    }

    #[cfg(test)]
    fn apply_at_base(
        process: &TargetProcess,
        image_base: usize,
        definitions: &[&LaunchPatch],
    ) -> Result<()> {
        let resolved = resolve_at_base(process, image_base, definitions)?;

        for patch in resolved {
            write_patch(process, &patch)?;
            eprintln!(
                "Applied {} at RVA 0x{:08X}",
                patch.definition.name, patch.definition.rva
            );
        }

        Ok(())
    }

    fn resolve_at_base<'a>(
        process: &TargetProcess,
        image_base: usize,
        definitions: &[&'a LaunchPatch],
    ) -> Result<Vec<ResolvedPatch<'a>>> {
        let mut resolved = Vec::with_capacity(definitions.len());

        for definition in definitions {
            validate_definition(definition)?;
            let address = image_base
                .checked_add(definition.rva as usize)
                .ok_or_else(|| LoaderError::new(ErrorKind::Internal, "patch address overflow"))?;
            let actual = remote::read(process, address, definition.expected.len())?;

            if actual != definition.expected {
                return Err(LoaderError::new(
                    ErrorKind::RemoteOperationFailed,
                    format!(
                        "{} bytes differ at 0x{address:08X}: expected={:02X?} actual={actual:02X?}",
                        definition.name, definition.expected
                    ),
                ));
            }

            resolved.push(ResolvedPatch {
                definition,
                address,
            });
        }

        Ok(resolved)
    }

    fn resolve_bootstrap(
        process: &TargetProcess,
        image_base: usize,
        definition: BootstrapSequencePatch,
    ) -> Result<ResolvedBootstrapPatch> {
        let binary_encrypt_call = checked_address(image_base, definition.binary_encrypt_call_rva)?;
        let text_encrypt_call = checked_address(image_base, definition.text_encrypt_call_rva)?;
        let late_reset_call = checked_address(image_base, definition.late_reset_call_rva)?;
        validate_bytes(
            process,
            binary_encrypt_call,
            definition.binary_encrypt_call_expected,
            "binary packet encryption call",
        )?;
        validate_bytes(
            process,
            text_encrypt_call,
            definition.text_encrypt_call_expected,
            "text packet encryption call",
        )?;
        validate_bytes(
            process,
            late_reset_call,
            definition.late_reset_call_expected,
            "bootstrap late sequence reset call",
        )?;

        Ok(ResolvedBootstrapPatch {
            binary_encrypt_call,
            text_encrypt_call,
            late_reset_call,
            reset_sequence: checked_address(image_base, definition.reset_sequence_rva)?,
            encrypt_packet: checked_address(image_base, definition.encrypt_packet_rva)?,
        })
    }

    fn checked_address(image_base: usize, rva: u32) -> Result<usize> {
        image_base
            .checked_add(rva as usize)
            .ok_or_else(|| LoaderError::new(ErrorKind::Internal, "patch address overflow"))
    }

    fn validate_bytes(
        process: &TargetProcess,
        address: usize,
        expected: &[u8],
        name: &str,
    ) -> Result<()> {
        let actual = remote::read(process, address, expected.len())?;
        if actual != expected {
            return Err(LoaderError::new(
                ErrorKind::RemoteOperationFailed,
                format!(
                    "{name} bytes differ at 0x{address:08X}: expected={expected:02X?} actual={actual:02X?}"
                ),
            ));
        }
        Ok(())
    }

    fn install_bootstrap_sequence_patch(
        process: &TargetProcess,
        patch: &ResolvedBootstrapPatch,
    ) -> Result<()> {
        let allocation = remote::RemoteAllocation::new(process, 21)?;
        let stub_address = allocation.address() as usize;
        let stub = bootstrap_sequence_stub(stub_address, patch)?;

        allocation.write_bytes(&stub)?;
        validate_bytes(process, stub_address, &stub, "bootstrap sequence stub")?;
        protect(process, stub_address, stub.len(), PAGE_EXECUTE_READ)?;
        flush(process, stub_address, stub.len())?;

        for (call, name) in [
            (patch.binary_encrypt_call, "binary packet encryption call"),
            (patch.text_encrypt_call, "text packet encryption call"),
        ] {
            let mut replacement = [0_u8; 5];
            replacement[0] = 0xE8;
            let return_address = call
                .checked_add(replacement.len())
                .ok_or_else(|| LoaderError::new(ErrorKind::Internal, "patch address overflow"))?;
            replacement[1..].copy_from_slice(&rel32(return_address, stub_address)?);
            write_code(process, call, &replacement, name)?;
        }
        write_code(
            process,
            patch.late_reset_call,
            &[0x90; 5],
            "bootstrap late sequence reset call",
        )?;

        let _ = allocation.persist();
        eprintln!("Applied default bootstrap sequence patch with stub at 0x{stub_address:08X}");
        Ok(())
    }

    fn bootstrap_sequence_stub(
        stub_address: usize,
        patch: &ResolvedBootstrapPatch,
    ) -> Result<[u8; 21]> {
        let mut stub = [0_u8; 21];
        stub[0..4].copy_from_slice(&[0x8B, 0x44, 0x24, 0x04]);
        stub[4..7].copy_from_slice(&[0x80, 0x38, 0x62]);
        stub[7..9].copy_from_slice(&[0x75, 0x07]);
        stub[9] = 0x51;
        stub[10] = 0xE8;
        stub[11..15].copy_from_slice(&rel32(stub_address + 15, patch.reset_sequence)?);
        stub[15] = 0x59;
        stub[16] = 0xE9;
        stub[17..21].copy_from_slice(&rel32(stub_address + 21, patch.encrypt_packet)?);
        Ok(stub)
    }

    fn rel32(next_instruction: usize, target: usize) -> Result<[u8; 4]> {
        let next_instruction = i64::try_from(next_instruction).map_err(|_| {
            LoaderError::new(
                ErrorKind::Internal,
                "relative branch source does not fit i64",
            )
        })?;
        let target = i64::try_from(target).map_err(|_| {
            LoaderError::new(
                ErrorKind::Internal,
                "relative branch target does not fit i64",
            )
        })?;
        let displacement = target.checked_sub(next_instruction).ok_or_else(|| {
            LoaderError::new(ErrorKind::Internal, "relative branch displacement overflow")
        })?;
        let displacement = i32::try_from(displacement).map_err(|_| {
            LoaderError::new(
                ErrorKind::RemoteOperationFailed,
                "bootstrap sequence stub is outside rel32 reach",
            )
        })?;
        Ok(displacement.to_le_bytes())
    }

    fn validate_definition(definition: &LaunchPatch) -> Result<()> {
        if definition.expected.is_empty()
            || definition.expected.len() != definition.replacement.len()
        {
            return Err(LoaderError::new(
                ErrorKind::Internal,
                format!("invalid {} patch definition", definition.name),
            ));
        }

        Ok(())
    }

    fn write_patch(process: &TargetProcess, patch: &ResolvedPatch<'_>) -> Result<()> {
        write_code(
            process,
            patch.address,
            patch.definition.replacement,
            patch.definition.name,
        )
    }

    fn write_code(
        process: &TargetProcess,
        address: usize,
        replacement: &[u8],
        name: &str,
    ) -> Result<()> {
        let old_protection = protect(process, address, replacement.len(), PAGE_EXECUTE_READWRITE)?;

        let operation = (|| {
            remote::write(process, address, replacement)?;
            flush(process, address, replacement.len())?;

            let actual = remote::read(process, address, replacement.len())?;
            if actual != replacement {
                return Err(LoaderError::new(
                    ErrorKind::RemoteOperationFailed,
                    format!("{name} replacement did not persist at 0x{address:08X}"),
                ));
            }

            Ok(())
        })();

        let restore = protect(process, address, replacement.len(), old_protection).map(|_| ());

        match (operation, restore) {
            (Ok(()), Ok(())) => Ok(()),
            (Err(error), Ok(())) => Err(error),
            (Ok(()), Err(error)) => Err(error),
            (Err(error), Err(restore_error)) => Err(LoaderError::new(
                error.kind(),
                format!("{error}; also failed to restore page protection: {restore_error}"),
            )),
        }
    }

    fn protect(
        process: &TargetProcess,
        address: usize,
        size: usize,
        protection: PAGE_PROTECTION_FLAGS,
    ) -> Result<PAGE_PROTECTION_FLAGS> {
        let mut old_protection = 0;

        // SAFETY: `process` has VM operation access, `address` and `size`
        // identify the already validated patch range, and `old_protection` is
        // a writable output. Windows aligns the affected pages internally.
        let succeeded = unsafe {
            VirtualProtectEx(
                process.handle().as_raw_handle(),
                address as *const c_void,
                size,
                protection,
                &mut old_protection,
            )
        };

        if succeeded == 0 {
            return Err(LoaderError::from_io(
                ErrorKind::RemoteOperationFailed,
                format!("failed to change target memory protection at 0x{address:08X}"),
                io::Error::last_os_error(),
            ));
        }

        Ok(old_protection)
    }

    fn flush(process: &TargetProcess, address: usize, size: usize) -> Result<()> {
        // SAFETY: `process` owns a valid process handle and `address..size`
        // is the patch range just written by this loader.
        let succeeded = unsafe {
            FlushInstructionCache(
                process.handle().as_raw_handle(),
                address as *const c_void,
                size,
            )
        };

        if succeeded == 0 {
            return Err(LoaderError::from_io(
                ErrorKind::RemoteOperationFailed,
                format!("failed to flush target instruction cache at 0x{address:08X}"),
                io::Error::last_os_error(),
            ));
        }

        Ok(())
    }

    #[cfg(all(test, target_arch = "x86"))]
    mod tests {
        use super::{
            LaunchPatch, ResolvedBootstrapPatch, apply_at_base, bootstrap_sequence_stub, rel32,
        };
        use crate::process::TargetProcess;
        use std::{ptr, slice};
        use windows_sys::Win32::System::Memory::{
            MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_EXECUTE_READ, PAGE_READWRITE, VirtualAlloc,
            VirtualFree, VirtualProtect,
        };

        #[test]
        fn validates_every_patch_before_writing() {
            const PAGE_SIZE: usize = 4096;
            let expected = [0x75, 0x07, 0x6A, 0x01];

            // SAFETY: Windows chooses a fresh page-sized allocation.
            let address = unsafe {
                VirtualAlloc(
                    ptr::null(),
                    PAGE_SIZE,
                    MEM_COMMIT | MEM_RESERVE,
                    PAGE_READWRITE,
                )
            };
            assert!(!address.is_null());

            // SAFETY: `address` is writable for PAGE_SIZE bytes and `expected`
            // fits entirely within that allocation.
            unsafe { ptr::copy_nonoverlapping(expected.as_ptr(), address.cast(), expected.len()) };

            let mut old_protection = 0;
            // SAFETY: `address` owns a live PAGE_SIZE allocation and the output
            // protection pointer is writable.
            assert_ne!(
                unsafe {
                    VirtualProtect(address, PAGE_SIZE, PAGE_EXECUTE_READ, &mut old_protection)
                },
                0
            );

            let process = TargetProcess::open(std::process::id()).expect("open current process");
            let first = LaunchPatch {
                name: "first",
                rva: 0,
                expected: &[0x75, 0x07],
                replacement: &[0xEB, 0x07],
            };
            let mismatched = LaunchPatch {
                name: "mismatched",
                rva: 2,
                expected: &[0xFF, 0x01],
                replacement: &[0x6A, 0x02],
            };

            let error = apply_at_base(&process, address as usize, &[&first, &mismatched])
                .expect_err("mismatched patch unexpectedly succeeded");
            assert!(error.to_string().contains("mismatched bytes differ"));

            // SAFETY: the allocation remains readable for `expected.len()`.
            let actual = unsafe { slice::from_raw_parts(address.cast::<u8>(), expected.len()) };
            assert_eq!(actual, expected);

            apply_at_base(&process, address as usize, &[&first])
                .expect("matching patch should succeed");
            // SAFETY: the allocation remains readable for two bytes.
            let actual = unsafe { slice::from_raw_parts(address.cast::<u8>(), 2) };
            assert_eq!(actual, [0xEB, 0x07]);

            // SAFETY: `address` is the base of the live allocation and has not
            // previously been released.
            assert_ne!(unsafe { VirtualFree(address, 0, MEM_RELEASE) }, 0);
        }

        #[test]
        fn encodes_forward_and_backward_relative_branches() {
            assert_eq!(rel32(0x1000, 0x1234).unwrap(), 0x234_i32.to_le_bytes());
            assert_eq!(rel32(0x1234, 0x1000).unwrap(), (-0x234_i32).to_le_bytes());
        }

        #[test]
        fn bootstrap_stub_resets_only_hello_at_worker_delivery() {
            let patch = ResolvedBootstrapPatch {
                binary_encrypt_call: 0,
                text_encrypt_call: 0,
                late_reset_call: 0,
                reset_sequence: 0x2000,
                encrypt_packet: 0x3000,
            };
            let stub = bootstrap_sequence_stub(0x1000, &patch).unwrap();

            assert_eq!(
                &stub[..9],
                &[0x8B, 0x44, 0x24, 0x04, 0x80, 0x38, 0x62, 0x75, 0x07]
            );
            assert_eq!(stub[9], 0x51);
            assert_eq!(stub[10], 0xE8);
            assert_eq!(&stub[11..15], &rel32(0x100F, 0x2000).unwrap());
            assert_eq!(stub[15], 0x59);
            assert_eq!(stub[16], 0xE9);
            assert_eq!(&stub[17..], &rel32(0x1015, 0x3000).unwrap());
        }
    }
}

#[cfg(windows)]
pub(crate) use platform::apply;

#[cfg(test)]
mod tests {
    use super::LaunchPatches;

    #[test]
    fn selects_independent_and_combined_patch_groups() {
        assert!(LaunchPatches::default().selected(false).is_empty());
        assert_eq!(LaunchPatches::default().selected(true).len(), 1);
        assert_eq!(
            LaunchPatches {
                skip_exchange_alerts: true,
                ..Default::default()
            }
            .selected(false)
            .len(),
            2
        );
        assert_eq!(
            LaunchPatches {
                allow_multiple: true,
                ..Default::default()
            }
            .selected(false)
            .len(),
            1
        );
        assert_eq!(
            LaunchPatches {
                skip_intro: true,
                ..Default::default()
            }
            .selected(false)
            .len(),
            1
        );
        assert_eq!(
            LaunchPatches {
                command_line_endpoint: true,
                ..Default::default()
            }
            .selected(false)
            .len(),
            2
        );
        assert_eq!(
            LaunchPatches {
                skip_notice: true,
                ..Default::default()
            }
            .selected(false)
            .len(),
            4
        );
        assert_eq!(
            LaunchPatches {
                allow_multiple: true,
                command_line_endpoint: true,
                skip_exchange_alerts: true,
                skip_intro: true,
                skip_notice: true,
            }
            .selected(false)
            .len(),
            10
        );
        assert_eq!(
            LaunchPatches {
                allow_multiple: true,
                command_line_endpoint: true,
                skip_exchange_alerts: true,
                skip_intro: true,
                skip_notice: true,
            }
            .selected(true)
            .len(),
            11
        );
    }
}
