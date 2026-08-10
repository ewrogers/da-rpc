use super::{interaction, module_base, network, read};
use crate::exchange::PendingItem;
use darpc_game_client::{
    CANCELLED_EXCHANGE_ALERT_PATCH, COMPLETED_EXCHANGE_ALERT_PATCH, GAME_MESSAGE_APPEND_RVA,
};
use darpc_protocol::{CommandFailure, ExchangeCommand};

const RESULT_MESSAGE_LIMIT: usize = 130;
const RESULT_PALETTE: u32 = 0x58;
const NEWLINE: &[u8] = b"\n\0";

type AppendMessageFn = unsafe extern "C" fn(*const u8, u32);

pub(super) fn submit(command: ExchangeCommand) -> Result<(), CommandFailure> {
    match command {
        ExchangeCommand::AddItem { slot, quantity } => add_item(slot, quantity),
        ExchangeCommand::SetGold(amount) => set_gold(amount),
        ExchangeCommand::Accept => accept(),
        ExchangeCommand::Cancel => cancel(),
    }
}

fn add_item(slot: darpc_protocol::ItemSlot, quantity: u8) -> Result<(), CommandFailure> {
    interaction::validate_item_quantity(slot, u32::from(quantity))?;
    let id = crate::exchange::active_id().ok_or(CommandFailure::InvalidState)?;
    if !crate::exchange::begin_item(slot.get(), quantity) {
        return Err(CommandFailure::Rejected);
    }
    let mut body = [0; 7];
    body[0] = 0x4A;
    body[1] = 0x01;
    body[2..6].copy_from_slice(&id.to_be_bytes());
    body[6] = slot.get();
    if let Err(error) = network::submit(&body) {
        crate::exchange::abort_item();
        return Err(error);
    }
    Ok(())
}

pub(crate) fn continue_item(id: u32, pending: PendingItem) -> Result<(), CommandFailure> {
    let mut body = [0; 8];
    body[0] = 0x4A;
    body[1] = 0x02;
    body[2..6].copy_from_slice(&id.to_be_bytes());
    body[6] = pending.slot;
    body[7] = pending.quantity;
    network::submit(&body)
}

pub(crate) fn display_result(message: &[u8], completed: bool) {
    let patch = if completed {
        &COMPLETED_EXCHANGE_ALERT_PATCH
    } else {
        &CANCELLED_EXCHANGE_ALERT_PATCH
    };
    let Ok(module_base) = module_base() else {
        return;
    };
    let Some(address) = module_base.checked_add(patch.rva as usize) else {
        return;
    };
    let Some(current) = read::<[u8; 12]>(address) else {
        return;
    };
    if current.as_slice() != patch.replacement {
        return;
    }

    let mut text = [0_u8; RESULT_MESSAGE_LIMIT + 1];
    let length = message.len().min(RESULT_MESSAGE_LIMIT);
    text[..length].copy_from_slice(&message[..length]);
    let Some(append_address) = module_base.checked_add(GAME_MESSAGE_APPEND_RVA) else {
        return;
    };
    // SAFETY: executable validation fixes the helper RVA and cdecl ABI. This
    // runs on the client main thread after the native exchange handler. Both
    // strings are bounded, null-terminated, and remain live for each call.
    unsafe {
        let append: AppendMessageFn = std::mem::transmute(append_address);
        append(text.as_ptr(), RESULT_PALETTE);
        append(NEWLINE.as_ptr(), RESULT_PALETTE);
    }
}

fn set_gold(amount: u32) -> Result<(), CommandFailure> {
    let available = crate::state::current_gold().ok_or(CommandFailure::InvalidState)?;
    if amount == 0 || amount > available {
        return Err(CommandFailure::InvalidArguments);
    }
    let id = crate::exchange::active_id().ok_or(CommandFailure::InvalidState)?;
    if !crate::exchange::begin_gold() {
        return Err(CommandFailure::Rejected);
    }
    let mut body = [0; 10];
    body[0] = 0x4A;
    body[1] = 0x03;
    body[2..6].copy_from_slice(&id.to_be_bytes());
    body[6..10].copy_from_slice(&amount.to_be_bytes());
    if let Err(error) = network::submit(&body) {
        crate::exchange::abort_gold();
        return Err(error);
    }
    Ok(())
}

fn accept() -> Result<(), CommandFailure> {
    let id = crate::exchange::active_id().ok_or(CommandFailure::InvalidState)?;
    if !crate::exchange::begin_accept() {
        return Err(CommandFailure::Rejected);
    }
    let result = simple(0x05, id);
    if result.is_err() {
        crate::exchange::abort_accept();
    }
    result
}

fn cancel() -> Result<(), CommandFailure> {
    let id = crate::exchange::active_id().ok_or(CommandFailure::InvalidState)?;
    if !crate::exchange::begin_cancel() {
        return Err(CommandFailure::Rejected);
    }
    let result = simple(0x04, id);
    if result.is_err() {
        crate::exchange::abort_cancel();
    }
    result
}

fn simple(action: u8, id: u32) -> Result<(), CommandFailure> {
    let mut body = [0; 6];
    body[0] = 0x4A;
    body[1] = action;
    body[2..6].copy_from_slice(&id.to_be_bytes());
    network::submit(&body)
}
