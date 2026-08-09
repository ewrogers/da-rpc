use super::{module_base, read};
use darpc_game_client::{EVENT_DISPATCHER_POINTER_RVA, NPC_SESSION_COL_RVAS};
use darpc_model::{DialogCloseReason, DialogSubmission};
use darpc_protocol::{CommandFailure, DialogAction, DialogCommand};
use std::{ffi::c_void, mem, ptr::NonNull};

const ENTRIES_OFFSET: usize = 0x64;
const COUNT_OFFSET: usize = 0x68;
const CAPACITY_OFFSET: usize = 0x6C;
const ENTRY_SIZE: usize = 0x0C;
const VISIBLE_OFFSET: usize = 0x130;
const REGISTRATION_OFFSET: usize = 0x188;
const REGISTERED: u8 = 0x02;
const SESSION_STATE_OFFSET: usize = 0x190;
const MERCHANT_TYPE_OFFSET: usize = 0x194;
const PURSUIT_TYPE_OFFSET: usize = 0x2A0;
const PURSUIT_PREVIOUS_OFFSET: usize = 0x2B0;
const PURSUIT_NEXT_OFFSET: usize = 0x2B1;
const OUTER_OFFSET: usize = 0x3BC;
const MODEL_OFFSET: usize = 0x634;
const ANSWER_OFFSET: usize = 0x638;
const MAX_PANES: i32 = 1_024;

const MERCHANT_OUTER_ACTION_RVA: usize = 0x0013_4B70;
const MERCHANT_TEXT_SELECT_RVA: usize = 0x0013_5590;
const MERCHANT_TEXT_INPUT_RVA: usize = 0x0013_59D0;
const MERCHANT_INVENTORY_SELECT_RVA: usize = 0x0013_62A0;
const MERCHANT_ABILITY_SELECT_RVA: usize = 0x0013_6620;
const MERCHANT_BOOK_SELECT_RVA: usize = 0x0013_6D60;
const MERCHANT_ITEM_SELECT_RVA: usize = 0x0013_8710;
const PURSUIT_OUTER_ACTION_RVA: usize = 0x0013_CD90;
const PURSUIT_PREVIOUS_RVA: usize = 0x0013_D940;
const PURSUIT_NEXT_RVA: usize = 0x0013_D9D0;
const PURSUIT_SELECT_RVA: usize = 0x0013_DC30;
const PURSUIT_SAY_SELECT_RVA: usize = 0x0013_DE00;
const PURSUIT_INPUT_RVA: usize = 0x0013_E070;
const PURSUIT_SAY_INPUT_RVA: usize = 0x0013_E270;

type SelectFn = unsafe extern "thiscall" fn(*mut c_void, u8) -> u32;
type ItemSelectFn = unsafe extern "thiscall" fn(*mut c_void, u8, u8) -> u32;
type InputFn = unsafe extern "thiscall" fn(*mut c_void, *const u8) -> u32;
type OuterFn = unsafe extern "thiscall" fn(*mut c_void, i32, u8) -> u32;
type NavigateFn = unsafe extern "thiscall" fn(*mut c_void) -> u32;
type CountFn = unsafe extern "thiscall" fn(*mut c_void) -> u32;

pub(super) fn submit(command: DialogCommand) -> Result<(), CommandFailure> {
    if crate::dialog::revision() != Some(command.revision) {
        return Err(CommandFailure::InvalidState);
    }
    if crate::dialog::is_pending() && !matches!(command.action, DialogAction::Close) {
        return Err(CommandFailure::Rejected);
    }
    let context = DialogContext::resolve()?;
    let submission = match command.action {
        DialogAction::Select { index, quantity } => {
            context.select(index, quantity)?;
            DialogSubmission::Select { index, quantity }
        }
        DialogAction::Input(input) => {
            context.input(input.as_bytes())?;
            DialogSubmission::Input {
                input: String::from_utf8_lossy(input.as_bytes()).into_owned(),
            }
        }
        DialogAction::Previous => {
            context.previous()?;
            DialogSubmission::Previous
        }
        DialogAction::Next => {
            context.next()?;
            DialogSubmission::Next
        }
        DialogAction::Close => {
            context.close()?;
            DialogSubmission::Close
        }
    };
    let tick_ms = now();
    crate::state::observe_dialog_submission(submission, tick_ms);
    if matches!(command.action, DialogAction::Close) {
        crate::state::observe_dialog_closed(DialogCloseReason::Client, tick_ms);
    }
    Ok(())
}

pub(crate) fn is_open() -> bool {
    DialogContext::resolve().is_ok()
}

struct DialogContext {
    module_base: usize,
    state: u32,
    subtype: u8,
    previous: bool,
    next: bool,
    outer: NonNull<c_void>,
    model: Option<NonNull<c_void>>,
    answer: Option<NonNull<c_void>>,
}

impl DialogContext {
    fn resolve() -> Result<Self, CommandFailure> {
        let module_base = module_base()?;
        let dispatcher = read::<u32>(module_base + EVENT_DISPATCHER_POINTER_RVA)
            .filter(|value| *value != 0)
            .ok_or(CommandFailure::InvalidState)? as usize;
        let entries =
            read::<u32>(dispatcher + ENTRIES_OFFSET).ok_or(CommandFailure::InvalidState)? as usize;
        let count = read::<i32>(dispatcher + COUNT_OFFSET).ok_or(CommandFailure::InvalidState)?;
        let capacity =
            read::<i32>(dispatcher + CAPACITY_OFFSET).ok_or(CommandFailure::InvalidState)?;
        if count < 0 || count > capacity || capacity > MAX_PANES || (count != 0 && entries == 0) {
            return Err(CommandFailure::InvalidState);
        }
        for index in 0..count as usize {
            let pane = read::<u32>(entries + index * ENTRY_SIZE)
                .filter(|value| *value != 0)
                .map(|value| value as usize);
            let Some(pane) = pane else { continue };
            let Some(vtable) = read::<u32>(pane).map(|value| value as usize) else {
                continue;
            };
            let Some(locator) = vtable
                .checked_sub(4)
                .and_then(read::<u32>)
                .map(|value| value as usize)
            else {
                continue;
            };
            if !NPC_SESSION_COL_RVAS
                .iter()
                .any(|rva| module_base + rva == locator)
            {
                continue;
            }
            if read::<u8>(pane + VISIBLE_OFFSET) != Some(1)
                || read::<u8>(pane + REGISTRATION_OFFSET).unwrap_or(0) & REGISTERED == 0
            {
                continue;
            }
            let state =
                read::<u32>(pane + SESSION_STATE_OFFSET).ok_or(CommandFailure::InvalidState)?;
            let subtype = match state {
                1 => read::<u8>(pane + MERCHANT_TYPE_OFFSET),
                2 => read::<u8>(pane + PURSUIT_TYPE_OFFSET),
                _ => None,
            }
            .ok_or(CommandFailure::InvalidState)?;
            let outer = read::<u32>(pane + OUTER_OFFSET)
                .and_then(|value| NonNull::new(value as *mut c_void))
                .ok_or(CommandFailure::InvalidState)?;
            let model = read::<u32>(outer.as_ptr() as usize + MODEL_OFFSET)
                .and_then(|value| NonNull::new(value as *mut c_void));
            let answer = read::<u32>(outer.as_ptr() as usize + ANSWER_OFFSET)
                .and_then(|value| NonNull::new(value as *mut c_void));
            return Ok(Self {
                module_base,
                state,
                subtype,
                previous: state == 2 && read::<u8>(pane + PURSUIT_PREVIOUS_OFFSET) == Some(1),
                next: state == 2 && read::<u8>(pane + PURSUIT_NEXT_OFFSET) == Some(1),
                outer,
                model,
                answer,
            });
        }
        Err(CommandFailure::InvalidState)
    }

    fn select(&self, index: u16, quantity: u8) -> Result<(), CommandFailure> {
        let model = self.response_model()?;
        let row = u8::try_from(index).map_err(|_| CommandFailure::InvalidArguments)?;
        if u32::from(row) >= self.row_count(model)? {
            return Err(CommandFailure::InvalidArguments);
        }
        match (self.state, self.subtype) {
            (1, 0 | 1) => self.call_select(MERCHANT_TEXT_SELECT_RVA, model, row),
            (1, 4 | 10) => {
                if quantity == 0 {
                    return Err(CommandFailure::InvalidArguments);
                }
                // SAFETY: the live model and native ABI were validated above.
                unsafe {
                    self.function::<ItemSelectFn>(MERCHANT_ITEM_SELECT_RVA)(
                        model.as_ptr(),
                        row,
                        quantity,
                    )
                };
            }
            (1, 5 | 11) => self.call_select(MERCHANT_INVENTORY_SELECT_RVA, model, row),
            (1, 6 | 7) => self.call_select(MERCHANT_ABILITY_SELECT_RVA, model, row),
            (1, 8 | 9) => self.call_select(MERCHANT_BOOK_SELECT_RVA, model, row),
            (2, 2 | 6) => self.call_select(PURSUIT_SELECT_RVA, model, row),
            (2, 3) => self.call_select(PURSUIT_SAY_SELECT_RVA, model, row),
            _ => return Err(CommandFailure::InvalidArguments),
        }
        Ok(())
    }

    fn input(&self, input: &[u8]) -> Result<(), CommandFailure> {
        let model = self.response_model()?;
        let maximum = if self.state == 2 {
            read::<u8>(model.as_ptr() as usize + 0x108).ok_or(CommandFailure::InvalidState)?
        } else {
            u8::MAX
        };
        if input.is_empty() || input.len() > usize::from(maximum) || input.contains(&0) {
            return Err(CommandFailure::InvalidArguments);
        }
        let mut text = [0_u8; 256];
        text[..input.len()].copy_from_slice(input);
        let rva = match (self.state, self.subtype) {
            (1, 2 | 3) => MERCHANT_TEXT_INPUT_RVA,
            (2, 4) => PURSUIT_INPUT_RVA,
            (2, 5) => PURSUIT_SAY_INPUT_RVA,
            _ => return Err(CommandFailure::InvalidArguments),
        };
        // SAFETY: the model is live, input is bounded and NUL terminated, and
        // the supported executable fixes the ABI.
        unsafe { self.function::<InputFn>(rva)(model.as_ptr(), text.as_ptr()) };
        Ok(())
    }

    fn previous(&self) -> Result<(), CommandFailure> {
        if self.state != 2 || !self.previous {
            return Err(CommandFailure::InvalidArguments);
        }
        // SAFETY: outer is the current live pursuit message pane.
        unsafe { self.function::<NavigateFn>(PURSUIT_PREVIOUS_RVA)(self.outer.as_ptr()) };
        Ok(())
    }

    fn next(&self) -> Result<(), CommandFailure> {
        if self.state != 2 || !self.next {
            return Err(CommandFailure::InvalidArguments);
        }
        // SAFETY: outer is the current live pursuit message pane.
        unsafe { self.function::<NavigateFn>(PURSUIT_NEXT_RVA)(self.outer.as_ptr()) };
        Ok(())
    }

    fn close(&self) -> Result<(), CommandFailure> {
        let (rva, action) = if self.state == 1 {
            (MERCHANT_OUTER_ACTION_RVA, 5)
        } else if self.state == 2 {
            (PURSUIT_OUTER_ACTION_RVA, 6)
        } else {
            return Err(CommandFailure::InvalidState);
        };
        // SAFETY: outer is the current dialog and the action is valid for its family.
        unsafe { self.function::<OuterFn>(rva)(self.outer.as_ptr(), action, 0) };
        Ok(())
    }

    fn response_model(&self) -> Result<NonNull<c_void>, CommandFailure> {
        self.answer.ok_or(CommandFailure::InvalidState)?;
        self.model.ok_or(CommandFailure::InvalidState)
    }

    fn row_count(&self, model: NonNull<c_void>) -> Result<u32, CommandFailure> {
        let vtable =
            read::<u32>(model.as_ptr() as usize).ok_or(CommandFailure::InvalidState)? as usize;
        let count = read::<u32>(vtable + 4)
            .filter(|value| *value != 0)
            .ok_or(CommandFailure::InvalidState)? as usize;
        // SAFETY: response models expose their displayed row count at vtable +4.
        Ok(unsafe { mem::transmute::<usize, CountFn>(count)(model.as_ptr()) })
    }

    fn call_select(&self, rva: usize, model: NonNull<c_void>, row: u8) {
        // SAFETY: the caller matched the current model subtype and row bounds.
        unsafe { self.function::<SelectFn>(rva)(model.as_ptr(), row) };
    }

    unsafe fn function<T: Copy>(&self, rva: usize) -> T {
        // SAFETY: each caller supplies the exact validated RVA and ABI type.
        unsafe { mem::transmute_copy(&(self.module_base + rva)) }
    }
}

fn now() -> u32 {
    #[cfg(windows)]
    {
        darpc_win32::pipe::sender_tick_ms()
    }
    #[cfg(not(windows))]
    {
        0
    }
}
