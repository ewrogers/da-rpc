#[cfg(any(windows, test))]
use darpc_game_client::{
    ALLOW_MULTIPLE_PATCHES, COMMAND_LINE_ENDPOINT_PATCHES, DISABLE_ENDPOINT_FALLBACK_PATCHES,
    LaunchPatch, SKIP_INTRO_PATCHES, SKIP_NOTICE_PATCHES,
};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct LaunchPatches {
    pub(crate) allow_multiple: bool,
    pub(crate) command_line_endpoint: bool,
    pub(crate) skip_intro: bool,
    pub(crate) skip_notice: bool,
}

impl LaunchPatches {
    #[cfg(windows)]
    pub(crate) const fn is_empty(self) -> bool {
        !self.allow_multiple && !self.command_line_endpoint && !self.skip_intro && !self.skip_notice
    }

    #[cfg(any(windows, test))]
    fn selected(self) -> Vec<&'static LaunchPatch> {
        let mut selected = Vec::new();

        if self.allow_multiple {
            selected.extend(ALLOW_MULTIPLE_PATCHES);
        }
        if self.command_line_endpoint {
            selected.extend(COMMAND_LINE_ENDPOINT_PATCHES);
            selected.extend(DISABLE_ENDPOINT_FALLBACK_PATCHES);
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
            Memory::{PAGE_EXECUTE_READWRITE, PAGE_PROTECTION_FLAGS, VirtualProtectEx},
            Threading::{PEB, PROCESS_BASIC_INFORMATION},
        },
    };

    struct ResolvedPatch<'a> {
        definition: &'a LaunchPatch,
        address: usize,
    }

    pub(crate) fn apply(process: &TargetProcess, selection: LaunchPatches) -> Result<()> {
        if selection.is_empty() {
            return Ok(());
        }

        let image_base = main_image_base(process)?;
        let selected = selection.selected();
        apply_at_base(process, image_base, &selected)
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

    fn apply_at_base(
        process: &TargetProcess,
        image_base: usize,
        definitions: &[&LaunchPatch],
    ) -> Result<()> {
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

        for patch in resolved {
            write_patch(process, &patch)?;
            eprintln!(
                "Applied {} at RVA 0x{:08X}",
                patch.definition.name, patch.definition.rva
            );
        }

        Ok(())
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
        let old_protection = protect(
            process,
            patch.address,
            patch.definition.replacement.len(),
            PAGE_EXECUTE_READWRITE,
        )?;

        let operation = (|| {
            remote::write(process, patch.address, patch.definition.replacement)?;
            flush(process, patch.address, patch.definition.replacement.len())?;

            let actual = remote::read(process, patch.address, patch.definition.replacement.len())?;
            if actual != patch.definition.replacement {
                return Err(LoaderError::new(
                    ErrorKind::RemoteOperationFailed,
                    format!(
                        "{} replacement did not persist at 0x{:08X}",
                        patch.definition.name, patch.address
                    ),
                ));
            }

            Ok(())
        })();

        let restore = protect(
            process,
            patch.address,
            patch.definition.replacement.len(),
            old_protection,
        )
        .map(|_| ());

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
        use super::{LaunchPatch, apply_at_base};
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
    }
}

#[cfg(windows)]
pub(crate) use platform::apply;

#[cfg(test)]
mod tests {
    use super::LaunchPatches;

    #[test]
    fn selects_independent_and_combined_patch_groups() {
        assert!(LaunchPatches::default().selected().is_empty());
        assert_eq!(
            LaunchPatches {
                allow_multiple: true,
                ..Default::default()
            }
            .selected()
            .len(),
            1
        );
        assert_eq!(
            LaunchPatches {
                skip_intro: true,
                ..Default::default()
            }
            .selected()
            .len(),
            1
        );
        assert_eq!(
            LaunchPatches {
                command_line_endpoint: true,
                ..Default::default()
            }
            .selected()
            .len(),
            2
        );
        assert_eq!(
            LaunchPatches {
                skip_notice: true,
                ..Default::default()
            }
            .selected()
            .len(),
            4
        );
        assert_eq!(
            LaunchPatches {
                allow_multiple: true,
                command_line_endpoint: true,
                skip_intro: true,
                skip_notice: true,
            }
            .selected()
            .len(),
            8
        );
    }
}
