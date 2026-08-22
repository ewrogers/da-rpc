mod ability;
mod chant;
pub(crate) mod dialog;
pub(crate) mod exchange;
pub(crate) mod field_map;
pub(crate) mod group;
mod interaction;
mod message;
pub(crate) mod movement;
pub(crate) mod network;
mod skill;
pub(crate) mod spell;
mod stat;

use darpc_protocol::{CommandFailure, CommandKind, WalkTarget};
use std::ptr;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;

use crate::{movement_transition::LocalMovementTransition, process_memory::read};

pub(crate) fn execute(command: CommandKind) -> Result<(), CommandFailure> {
    match command {
        CommandKind::Diagnostic => Ok(()),
        CommandKind::Turn(direction) => movement::turn(direction),
        CommandKind::Walk(WalkTarget::Direction(direction)) => movement::walk(direction),
        CommandKind::Walk(WalkTarget::Destination { x, y }) => movement::walk_to(x, y),
        CommandKind::Walk(WalkTarget::Route(route)) => movement::walk_route(route),
        CommandKind::Walk(WalkTarget::Cancel) => movement::cancel_walk(),
        CommandKind::UseSkill(slot) => skill::use_skill(slot),
        CommandKind::CastSpell(cast) => spell::cast(cast),
        CommandKind::UseItem(slot) => interaction::use_item(slot),
        CommandKind::DropItem(transfer) => interaction::drop_item(transfer),
        CommandKind::DropGold(transfer) => interaction::drop_gold(transfer),
        CommandKind::GiveItem(transfer) => interaction::give_item(transfer),
        CommandKind::GiveGold(transfer) => interaction::give_gold(transfer),
        CommandKind::SwapSlots(swap) => interaction::swap_slots(swap),
        CommandKind::PickupItem(position) => interaction::pickup_item(position),
        CommandKind::Unequip(slot) => interaction::unequip(slot),
        CommandKind::Emote(code) => interaction::emote(code),
        CommandKind::Interact(id) => movement::interact(id),
        CommandKind::Dialog(command) => dialog::submit(command),
        CommandKind::Group(command) => group::submit(command),
        CommandKind::Who | CommandKind::InspectPlayer(_) => Err(CommandFailure::Internal),
        CommandKind::Exchange(command) => exchange::submit(command),
        CommandKind::Chant(text) => chant::submit(text),
        CommandKind::Legend => network::submit(&[0x2D]),
        CommandKind::Raw(packet) => network::raw(packet),
        CommandKind::Assail => network::submit(&[0x13]),
        CommandKind::Resync => Err(CommandFailure::Internal),
        CommandKind::Message(message) => message::submit(message),
        CommandKind::AddStat(stat) => stat::add(stat),
        CommandKind::SelectFieldMapDestination(command) => field_map::submit(command),
        CommandKind::DismissMessageDialog(command) => {
            let update = crate::message_dialog::dismiss(command)?;
            if let Some(update) = update {
                crate::state::observe_message_dialogs(update, darpc_win32::pipe::sender_tick_ms());
            }
            Ok(())
        }
    }
}

pub(crate) fn begin_resync_transition() -> Result<LocalMovementTransition, CommandFailure> {
    movement::begin_resync()
}

pub(crate) fn resync_transition() -> Result<LocalMovementTransition, CommandFailure> {
    movement::resync_transition()
}

pub(crate) fn submit_resync_packet() -> Result<(), CommandFailure> {
    network::submit(&[0x38])
}

fn module_base() -> Result<usize, CommandFailure> {
    // SAFETY: a null module name requests the executable module for the
    // current process and does not transfer ownership.
    let module = unsafe { GetModuleHandleW(ptr::null()) } as usize;
    (module != 0)
        .then_some(module)
        .ok_or(CommandFailure::InvalidState)
}
