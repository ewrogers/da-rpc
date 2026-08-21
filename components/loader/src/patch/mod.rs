#[cfg(any(windows, test))]
use darpc_game_client::{
    ALLOW_MULTIPLE_PATCHES, COMMAND_LINE_ENDPOINT_PATCHES, DEFAULT_RUNTIME_PATCHES,
    DISABLE_ENDPOINT_FALLBACK_PATCHES, LaunchPatch, SKIP_EXCHANGE_ALERTS_PATCHES,
    SKIP_INTRO_PATCHES, SKIP_NOTICE_PATCHES,
};
#[cfg(windows)]
use darpc_game_client::{
    BOOTSTRAP_SEQUENCE_PATCH, BootstrapSequencePatch, GROUND_ITEM_REVEAL_PATCH,
    GroundItemRevealPatch,
};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct LaunchPatches {
    pub(crate) allow_multiple: bool,
    pub(crate) command_line_endpoint: bool,
    pub(crate) show_items_with_alt: bool,
    pub(crate) skip_exchange_alerts: bool,
    pub(crate) skip_intro: bool,
    pub(crate) skip_notice: bool,
}

impl LaunchPatches {
    #[cfg(any(windows, test))]
    pub(crate) const fn is_empty(self) -> bool {
        !self.allow_multiple
            && !self.command_line_endpoint
            && !self.show_items_with_alt
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

    struct ResolvedGroundItemRevealPatch<'a> {
        definition: &'a GroundItemRevealPatch,
        collector_hook: usize,
        frame_hook: usize,
        key_down_hook: usize,
        key_up_hook: usize,
        input_get_event_manager: usize,
        render_world_object: usize,
        invalidate_pane: usize,
        world_item_vtable: usize,
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
        let ground_items = selection
            .show_items_with_alt
            .then(|| resolve_ground_item_reveal(process, image_base, &GROUND_ITEM_REVEAL_PATCH))
            .transpose()?;

        for patch in resolved {
            write_patch(process, &patch)?;
            eprintln!(
                "Applied {} at RVA 0x{:08X}",
                patch.definition.name, patch.definition.rva
            );
        }

        if let Some(ground_items) = ground_items {
            install_ground_item_reveal(process, &ground_items)?;
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

    fn resolve_ground_item_reveal<'a>(
        process: &TargetProcess,
        image_base: usize,
        definition: &'a GroundItemRevealPatch,
    ) -> Result<ResolvedGroundItemRevealPatch<'a>> {
        validate_ground_item_definition(definition)?;

        let collector_hook = checked_address(image_base, definition.collector_hook_rva)?;
        let frame_hook = checked_address(image_base, definition.frame_hook_rva)?;
        let key_down_hook = checked_address(image_base, definition.key_down_hook_rva)?;
        let key_up_hook = checked_address(image_base, definition.key_up_hook_rva)?;
        let static_render_mode_selector =
            checked_address(image_base, definition.static_render_mode_selector_rva)?;

        for (address, expected, name) in [
            (
                collector_hook,
                definition.collector_hook_expected,
                "ground-item collector hook",
            ),
            (
                frame_hook,
                definition.frame_hook_expected,
                "ground-item frame hook",
            ),
            (
                key_down_hook,
                definition.key_down_hook_expected,
                "ground-item Alt key-down hook",
            ),
            (
                key_up_hook,
                definition.key_up_hook_expected,
                "ground-item Alt key-up hook",
            ),
            (
                static_render_mode_selector,
                definition.static_render_mode_selector_expected,
                "ground-item static render-mode selector",
            ),
        ] {
            validate_bytes(process, address, expected, name)?;
        }

        Ok(ResolvedGroundItemRevealPatch {
            definition,
            collector_hook,
            frame_hook,
            key_down_hook,
            key_up_hook,
            input_get_event_manager: checked_address(
                image_base,
                definition.input_get_event_manager_rva,
            )?,
            render_world_object: checked_address(image_base, definition.render_world_object_rva)?,
            invalidate_pane: checked_address(image_base, definition.invalidate_pane_rva)?,
            world_item_vtable: checked_address(image_base, definition.world_item_vtable_rva)?,
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

    fn install_ground_item_reveal(
        process: &TargetProcess,
        patch: &ResolvedGroundItemRevealPatch<'_>,
    ) -> Result<()> {
        let definition = patch.definition;
        let state = remote::RemoteAllocation::new(process, definition.state_size)?;
        let collector =
            remote::RemoteAllocation::new(process, definition.collector_stub_template.len())?;
        let frame = remote::RemoteAllocation::new(process, definition.frame_stub_template.len())?;
        let key_down =
            remote::RemoteAllocation::new(process, definition.key_down_stub_template.len())?;
        let key_up = remote::RemoteAllocation::new(process, definition.key_up_stub_template.len())?;

        let state_address = state.address() as usize;
        let collector_address = collector.address() as usize;
        let frame_address = frame.address() as usize;
        let key_down_address = key_down.address() as usize;
        let key_up_address = key_up.address() as usize;

        let collector_stub = ground_item_collector_stub(collector_address, state_address, patch)?;
        let frame_stub = ground_item_frame_stub(frame_address, state_address, patch)?;
        let key_down_stub = ground_item_key_transition_stub(
            key_down_address,
            state_address,
            patch.key_down_hook,
            definition.key_down_hook_expected.len(),
            definition.key_down_stub_template,
            patch.invalidate_pane,
            definition.state_pane_offset,
        )?;
        let key_up_stub = ground_item_key_transition_stub(
            key_up_address,
            state_address,
            patch.key_up_hook,
            definition.key_up_hook_expected.len(),
            definition.key_up_stub_template,
            patch.invalidate_pane,
            definition.state_pane_offset,
        )?;

        prepare_executable_stub(
            process,
            &collector,
            &collector_stub,
            "ground-item collector stub",
        )?;
        prepare_executable_stub(process, &frame, &frame_stub, "ground-item frame stub")?;
        prepare_executable_stub(
            process,
            &key_down,
            &key_down_stub,
            "ground-item key-down stub",
        )?;
        prepare_executable_stub(process, &key_up, &key_up_stub, "ground-item key-up stub")?;

        let hooks = [
            (
                patch.collector_hook,
                definition.collector_hook_expected,
                collector_address,
                "ground-item collector hook",
            ),
            (
                patch.frame_hook,
                definition.frame_hook_expected,
                frame_address,
                "ground-item frame hook",
            ),
            (
                patch.key_down_hook,
                definition.key_down_hook_expected,
                key_down_address,
                "ground-item Alt key-down hook",
            ),
            (
                patch.key_up_hook,
                definition.key_up_hook_expected,
                key_up_address,
                "ground-item Alt key-up hook",
            ),
        ];
        let replacements = hooks
            .iter()
            .map(|(address, expected, stub_address, _)| {
                jump_hook(*address, *stub_address, expected.len())
            })
            .collect::<Result<Vec<_>>>()?;
        let allocations = [state, collector, frame, key_down, key_up];

        for (installed, ((address, _, _, name), replacement)) in
            hooks.into_iter().zip(replacements).enumerate()
        {
            if let Err(error) = write_code(process, address, &replacement, name) {
                let mut cleanup_error = None;
                for (address, expected, _, name) in hooks[..=installed].iter().rev().copied() {
                    if let Err(error) = write_code(process, address, expected, name) {
                        cleanup_error.get_or_insert(error);
                    }
                }

                if let Some(cleanup_error) = cleanup_error {
                    for allocation in allocations {
                        let _ = allocation.persist();
                    }
                    return Err(LoaderError::new(
                        ErrorKind::RemoteOperationFailed,
                        format!(
                            "{error}; also failed to restore a ground-item hook: {cleanup_error}"
                        ),
                    ));
                }

                return Err(error);
            }
        }

        for allocation in allocations {
            let _ = allocation.persist();
        }
        eprintln!("Applied Alt ground-item reveal patch with state at 0x{state_address:08X}");
        Ok(())
    }

    fn prepare_executable_stub(
        process: &TargetProcess,
        allocation: &remote::RemoteAllocation<'_>,
        stub: &[u8],
        name: &str,
    ) -> Result<()> {
        let address = allocation.address() as usize;
        allocation.write_bytes(stub)?;
        validate_bytes(process, address, stub, name)?;
        protect(process, address, stub.len(), PAGE_EXECUTE_READ)?;
        flush(process, address, stub.len())
    }

    fn ground_item_collector_stub(
        stub_address: usize,
        state_address: usize,
        patch: &ResolvedGroundItemRevealPatch<'_>,
    ) -> Result<Vec<u8>> {
        let definition = patch.definition;
        let mut stub = definition.collector_stub_template.to_vec();
        write_u32(&mut stub, 0x3A, patch.world_item_vtable, "collector vtable")?;
        write_u32(&mut stub, 0x4A, state_address, "collector state count")?;
        write_u32(
            &mut stub,
            0x4F,
            definition.capacity as usize,
            "collector capacity",
        )?;
        write_u32(
            &mut stub,
            0x5A,
            checked_offset(state_address, definition.state_entries_offset)?,
            "collector entries",
        )?;
        write_u32(&mut stub, 0x70, state_address, "collector state count")?;
        write_u32(
            &mut stub,
            0x76,
            checked_offset(state_address, 4)?,
            "collector last item",
        )?;
        write_u32(
            &mut stub,
            0x7E,
            checked_offset(state_address, 8)?,
            "collector pane",
        )?;
        write_rel32(
            &mut stub,
            0x99,
            checked_offset(stub_address, 0x9D)?,
            checked_offset(
                patch.collector_hook,
                definition.collector_hook_expected.len(),
            )?,
            "collector continuation",
        )?;
        Ok(stub)
    }

    fn ground_item_frame_stub(
        stub_address: usize,
        state_address: usize,
        patch: &ResolvedGroundItemRevealPatch<'_>,
    ) -> Result<Vec<u8>> {
        let definition = patch.definition;
        let mut stub = definition.frame_stub_template.to_vec();
        write_u32(
            &mut stub,
            0x0D,
            checked_offset(state_address, definition.state_pane_offset)?,
            "frame pane state",
        )?;
        write_u32(&mut stub, 0x13, state_address, "frame item count")?;
        write_rel32(
            &mut stub,
            0x26,
            checked_offset(stub_address, 0x2A)?,
            patch.input_get_event_manager,
            "frame event manager call",
        )?;
        write_u32(&mut stub, 0x3B, state_address, "frame item count")?;
        write_u32(
            &mut stub,
            0x46,
            checked_offset(state_address, definition.state_entries_offset)?,
            "frame entries",
        )?;
        write_u32(&mut stub, 0x52, patch.world_item_vtable, "frame vtable")?;
        write_u32(
            &mut stub,
            0x79,
            checked_offset(state_address, 4)?,
            "frame last item",
        )?;
        write_u32(
            &mut stub,
            0x8D,
            checked_offset(state_address, 8)?,
            "frame pane",
        )?;
        write_rel32(
            &mut stub,
            0x92,
            checked_offset(stub_address, 0x96)?,
            patch.render_world_object,
            "frame render call",
        )?;
        write_rel32(
            &mut stub,
            0xB6,
            checked_offset(stub_address, 0xBA)?,
            checked_offset(patch.frame_hook, definition.frame_hook_expected.len())?,
            "frame continuation",
        )?;
        Ok(stub)
    }

    #[allow(clippy::too_many_arguments)]
    fn ground_item_key_transition_stub(
        stub_address: usize,
        state_address: usize,
        hook_address: usize,
        hook_length: usize,
        template: &[u8],
        invalidate_pane: usize,
        state_pane_offset: usize,
    ) -> Result<Vec<u8>> {
        let mut stub = template.to_vec();
        write_u32(
            &mut stub,
            0x31,
            checked_offset(state_address, state_pane_offset)?,
            "key-transition pane state",
        )?;
        write_rel32(
            &mut stub,
            0x3C,
            checked_offset(stub_address, 0x40)?,
            invalidate_pane,
            "key-transition pane invalidation",
        )?;
        write_rel32(
            &mut stub,
            0x50,
            checked_offset(stub_address, 0x54)?,
            checked_offset(hook_address, hook_length)?,
            "key-transition continuation",
        )?;
        Ok(stub)
    }

    fn jump_hook(address: usize, stub_address: usize, length: usize) -> Result<Vec<u8>> {
        if length < 5 {
            return Err(LoaderError::new(
                ErrorKind::Internal,
                "ground-item hook is shorter than a near jump",
            ));
        }
        let mut hook = vec![0x90; length];
        hook[0] = 0xE9;
        hook[1..5].copy_from_slice(&rel32(checked_offset(address, 5)?, stub_address)?);
        Ok(hook)
    }

    fn write_u32(stub: &mut [u8], offset: usize, value: usize, name: &str) -> Result<()> {
        let value = u32::try_from(value).map_err(|_| {
            LoaderError::new(
                ErrorKind::RemoteOperationFailed,
                format!("{name} address does not fit the 32-bit client"),
            )
        })?;
        let destination = stub.get_mut(offset..offset + 4).ok_or_else(|| {
            LoaderError::new(
                ErrorKind::Internal,
                format!("{name} relocation is out of range"),
            )
        })?;
        destination.copy_from_slice(&value.to_le_bytes());
        Ok(())
    }

    fn write_rel32(
        stub: &mut [u8],
        offset: usize,
        next_instruction: usize,
        target: usize,
        name: &str,
    ) -> Result<()> {
        let destination = stub.get_mut(offset..offset + 4).ok_or_else(|| {
            LoaderError::new(
                ErrorKind::Internal,
                format!("{name} relocation is out of range"),
            )
        })?;
        destination.copy_from_slice(&rel32(next_instruction, target)?);
        Ok(())
    }

    fn checked_offset(address: usize, offset: usize) -> Result<usize> {
        address
            .checked_add(offset)
            .ok_or_else(|| LoaderError::new(ErrorKind::Internal, "patch address overflow"))
    }

    fn validate_ground_item_definition(definition: &GroundItemRevealPatch) -> Result<()> {
        let hook_lengths = [
            definition.collector_hook_expected.len(),
            definition.frame_hook_expected.len(),
            definition.key_down_hook_expected.len(),
            definition.key_up_hook_expected.len(),
        ];
        if hook_lengths.iter().any(|length| *length < 5)
            || definition.static_render_mode_selector_expected.is_empty()
            || definition.collector_stub_template.len() < 0x9D
            || definition.frame_stub_template.len() < 0xBA
            || definition.key_down_stub_template.len() < 0x54
            || definition.key_up_stub_template.len() < 0x54
        {
            return Err(LoaderError::new(
                ErrorKind::Internal,
                "invalid ground-item reveal patch definition",
            ));
        }
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
            GROUND_ITEM_REVEAL_PATCH, LaunchPatch, ResolvedBootstrapPatch,
            ResolvedGroundItemRevealPatch, apply_at_base, bootstrap_sequence_stub,
            ground_item_collector_stub, ground_item_frame_stub, ground_item_key_transition_stub,
            jump_hook, rel32,
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

        #[test]
        fn ground_item_stubs_relocate_client_and_state_addresses() {
            let patch = resolved_ground_item_patch();
            let state = 0x00B7_0000;

            let collector_address = 0x1000_0000;
            let collector = ground_item_collector_stub(collector_address, state, &patch).unwrap();
            assert_eq!(read_u32(&collector, 0x3A), 0x0068_B1AC);
            assert_eq!(read_u32(&collector, 0x4A), state as u32);
            assert_eq!(read_u32(&collector, 0x4F), 255);
            assert_eq!(read_u32(&collector, 0x5A), 0x00B7_0100);
            assert_eq!(read_u32(&collector, 0x70), state as u32);
            assert_eq!(read_u32(&collector, 0x76), 0x00B7_0004);
            assert_eq!(read_u32(&collector, 0x7E), 0x00B7_0008);
            assert_eq!(
                &collector[0x99..0x9D],
                &rel32(collector_address + 0x9D, patch.collector_hook + 5).unwrap()
            );

            let frame_address = 0x2000_0000;
            let frame = ground_item_frame_stub(frame_address, state, &patch).unwrap();
            assert_eq!(read_u32(&frame, 0x0D), 0x00B7_0028);
            assert_eq!(read_u32(&frame, 0x13), state as u32);
            assert_eq!(
                &frame[0x26..0x2A],
                &rel32(frame_address + 0x2A, patch.input_get_event_manager).unwrap()
            );
            assert_eq!(read_u32(&frame, 0x3B), state as u32);
            assert_eq!(read_u32(&frame, 0x46), 0x00B7_0100);
            assert_eq!(read_u32(&frame, 0x52), 0x0068_B1AC);
            assert_eq!(read_u32(&frame, 0x79), 0x00B7_0004);
            assert_eq!(read_u32(&frame, 0x8D), 0x00B7_0008);
            assert_eq!(
                &frame[0x92..0x96],
                &rel32(frame_address + 0x96, patch.render_world_object).unwrap()
            );
            assert_eq!(
                &frame[0xB6..0xBA],
                &rel32(frame_address + 0xBA, patch.frame_hook + 6).unwrap()
            );
        }

        #[test]
        fn ground_item_key_stubs_and_hooks_preserve_full_instructions() {
            let patch = resolved_ground_item_patch();
            let state = 0x00B7_0000;
            let stub_address = 0x3000_0000;
            let stub = ground_item_key_transition_stub(
                stub_address,
                state,
                patch.key_down_hook,
                patch.definition.key_down_hook_expected.len(),
                patch.definition.key_down_stub_template,
                patch.invalidate_pane,
                patch.definition.state_pane_offset,
            )
            .unwrap();

            assert_eq!(read_u32(&stub, 0x31), 0x00B7_0028);
            assert_eq!(
                &stub[0x3C..0x40],
                &rel32(stub_address + 0x40, patch.invalidate_pane).unwrap()
            );
            assert_eq!(
                &stub[0x50..0x54],
                &rel32(stub_address + 0x54, patch.key_down_hook + 5).unwrap()
            );

            let five_byte = jump_hook(0x0046_7C10, 0x1000_0000, 5).unwrap();
            assert_eq!(five_byte[0], 0xE9);
            assert_eq!(&five_byte[1..5], &rel32(0x0046_7C15, 0x1000_0000).unwrap());
            let six_byte = jump_hook(0x005C_E280, 0x2000_0000, 6).unwrap();
            assert_eq!(six_byte[0], 0xE9);
            assert_eq!(six_byte[5], 0x90);
        }

        fn resolved_ground_item_patch() -> ResolvedGroundItemRevealPatch<'static> {
            ResolvedGroundItemRevealPatch {
                definition: &GROUND_ITEM_REVEAL_PATCH,
                collector_hook: 0x005D_3740,
                frame_hook: 0x005C_E280,
                key_down_hook: 0x0046_7C10,
                key_up_hook: 0x0046_7E30,
                input_get_event_manager: 0x0042_7380,
                render_world_object: 0x005D_3190,
                invalidate_pane: 0x0054_9F60,
                world_item_vtable: 0x0068_B1AC,
            }
        }

        fn read_u32(bytes: &[u8], offset: usize) -> u32 {
            u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
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
        assert!(LaunchPatches::default().is_empty());
        assert_eq!(LaunchPatches::default().selected(true).len(), 1);
        let ground_items = LaunchPatches {
            show_items_with_alt: true,
            ..Default::default()
        };
        assert!(ground_items.selected(false).is_empty());
        assert!(!ground_items.is_empty());
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
                show_items_with_alt: true,
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
                show_items_with_alt: true,
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
