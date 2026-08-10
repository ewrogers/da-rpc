use super::{module_base, movement, network, read};
use darpc_game_client::{GUI_BACK_PANE_GET_RVA, ITEM_ACTIVATE_RVA};
use darpc_model::EquipmentSlot;
use darpc_protocol::{
    CommandFailure, GoldTransfer, ItemSlot, ItemTransfer, SlotSwap, TilePosition, TransferTarget,
};
use std::{ffi::c_void, mem, ptr::NonNull};

const ITEMS_OFFSET: usize = 0x4F88;
const ITEM_ARRAY_OFFSET: usize = 0x1A0;
const ITEM_SLOT_OFFSET: usize = 0x214;
const ITEM_QUANTITY_OFFSET: usize = 0x240;
const ITEM_STACKABLE_OFFSET: usize = 0x244;

type GuiBackPaneGetFn = unsafe extern "C" fn() -> *mut c_void;
type ItemActivateFn = unsafe extern "thiscall" fn(*mut c_void, u8);

pub(super) fn use_item(slot: ItemSlot) -> Result<(), CommandFailure> {
    let inventory = Inventory::resolve()?;
    inventory.item(slot)?;
    // SAFETY: the validated RVA and ABI identify the normal inventory
    // activation routine. The pane and slot were resolved from live UI state.
    unsafe { inventory.activate_fn()(inventory.pane.as_ptr(), slot.get()) };
    Ok(())
}

pub(crate) fn validate_item_quantity(slot: ItemSlot, quantity: u32) -> Result<(), CommandFailure> {
    Inventory::resolve()?
        .item(slot)?
        .validate_quantity(quantity)
}

pub(super) fn drop_item(transfer: ItemTransfer) -> Result<(), CommandFailure> {
    let item = Inventory::resolve()?.item(transfer.slot)?;
    item.validate_quantity(transfer.quantity)?;
    match transfer.target {
        TransferTarget::Tile(position) => {
            let (x, y) = movement::validate_tile(position.x, position.y)?;
            let mut body = [0; 10];
            body[0] = 0x08;
            body[1] = transfer.slot.get();
            body[2..4].copy_from_slice(&x.to_be_bytes());
            body[4..6].copy_from_slice(&y.to_be_bytes());
            body[6..10].copy_from_slice(&transfer.quantity.to_be_bytes());
            network::submit(&body)
        }
        TransferTarget::Object(_) => Err(CommandFailure::InvalidArguments),
    }
}

pub(super) fn give_item(transfer: ItemTransfer) -> Result<(), CommandFailure> {
    let item = Inventory::resolve()?.item(transfer.slot)?;
    item.validate_quantity(transfer.quantity)?;
    let TransferTarget::Object(id) = transfer.target else {
        return Err(CommandFailure::InvalidArguments);
    };
    validate_object_target(id.get())?;
    let mut body = [0; 10];
    body[0] = 0x29;
    body[1] = transfer.slot.get();
    body[2..6].copy_from_slice(&id.get().to_be_bytes());
    body[6..10].copy_from_slice(&transfer.quantity.to_be_bytes());
    network::submit(&body)
}

pub(super) fn drop_gold(transfer: GoldTransfer) -> Result<(), CommandFailure> {
    if transfer.amount == 0 {
        return Err(CommandFailure::InvalidArguments);
    }
    match transfer.target {
        TransferTarget::Tile(position) => {
            let (x, y) = movement::validate_tile(position.x, position.y)?;
            let mut body = [0; 9];
            body[0] = 0x24;
            body[1..5].copy_from_slice(&transfer.amount.to_be_bytes());
            body[5..7].copy_from_slice(&x.to_be_bytes());
            body[7..9].copy_from_slice(&y.to_be_bytes());
            network::submit(&body)
        }
        TransferTarget::Object(_) => Err(CommandFailure::InvalidArguments),
    }
}

pub(super) fn give_gold(transfer: GoldTransfer) -> Result<(), CommandFailure> {
    if transfer.amount == 0 {
        return Err(CommandFailure::InvalidArguments);
    }
    let TransferTarget::Object(id) = transfer.target else {
        return Err(CommandFailure::InvalidArguments);
    };
    validate_object_target(id.get())?;
    let mut body = [0; 9];
    body[0] = 0x2A;
    body[1..5].copy_from_slice(&transfer.amount.to_be_bytes());
    body[5..9].copy_from_slice(&id.get().to_be_bytes());
    network::submit(&body)
}

pub(super) fn swap_slots(swap: SlotSwap) -> Result<(), CommandFailure> {
    let (panel, source, destination) = match swap {
        SlotSwap::Inventory {
            source,
            destination,
        } => (0, source.get(), destination.get()),
        SlotSwap::Spellbook {
            source,
            destination,
        } => (1, source.get(), destination.get()),
        SlotSwap::Skillbook {
            source,
            destination,
        } => (2, source.get(), destination.get()),
    };
    if source == destination {
        return Err(CommandFailure::InvalidArguments);
    }
    network::submit(&[0x30, panel, source, destination])
}

fn validate_object_target(object_id: u32) -> Result<(), CommandFailure> {
    if object_id == movement::local_object_id()? {
        return Err(CommandFailure::InvalidTarget);
    }
    Ok(())
}

pub(super) fn pickup_item(position: TilePosition) -> Result<(), CommandFailure> {
    let (x, y) = movement::validate_tile(position.x, position.y)?;
    let slot = Inventory::resolve()?.first_empty_slot()?;
    let mut body = [0; 6];
    body[0] = 0x07;
    body[1] = slot;
    body[2..4].copy_from_slice(&x.to_be_bytes());
    body[4..6].copy_from_slice(&y.to_be_bytes());
    network::submit(&body)
}

pub(super) fn unequip(slot: EquipmentSlot) -> Result<(), CommandFailure> {
    network::submit(&[0x44, slot.raw()])
}

pub(super) fn emote(code: u8) -> Result<(), CommandFailure> {
    if !((0..=8).contains(&code) || (12..=35).contains(&code)) {
        return Err(CommandFailure::InvalidArguments);
    }
    network::submit(&[0x1D, code])
}

struct Inventory {
    module_base: usize,
    pane: NonNull<c_void>,
}

impl Inventory {
    fn resolve() -> Result<Self, CommandFailure> {
        let module_base = module_base()?;
        let address = module_base
            .checked_add(GUI_BACK_PANE_GET_RVA)
            .ok_or(CommandFailure::Internal)?;
        // SAFETY: exact client validation fixes the accessor RVA and ABI. It
        // returns a borrowed live pane or null outside the lower-tray lifetime.
        let gui = unsafe {
            let get: GuiBackPaneGetFn = mem::transmute(address);
            NonNull::new(get())
        }
        .ok_or(CommandFailure::InvalidState)?;
        let pane = read::<u32>(gui.as_ptr() as usize + ITEMS_OFFSET)
            .and_then(|pointer| NonNull::new(pointer as *mut c_void))
            .ok_or(CommandFailure::InvalidState)?;
        Ok(Self { module_base, pane })
    }

    fn item(&self, slot: ItemSlot) -> Result<Item, CommandFailure> {
        let index = usize::from(slot.get() - 1);
        let address = self.pane.as_ptr() as usize + ITEM_ARRAY_OFFSET + index * 4;
        let pane = read::<u32>(address)
            .and_then(|pointer| NonNull::new(pointer as *mut c_void))
            .ok_or(CommandFailure::InvalidArguments)?;
        let retained_slot = read::<u8>(pane.as_ptr() as usize + ITEM_SLOT_OFFSET)
            .ok_or(CommandFailure::InvalidState)?;
        if retained_slot != slot.get() {
            return Err(CommandFailure::InvalidArguments);
        }
        Ok(Item { pane })
    }

    fn first_empty_slot(&self) -> Result<u8, CommandFailure> {
        (1..=59)
            .find(|slot| {
                let address =
                    self.pane.as_ptr() as usize + ITEM_ARRAY_OFFSET + usize::from(*slot - 1) * 4;
                read::<u32>(address) == Some(0)
            })
            .ok_or(CommandFailure::Rejected)
    }

    fn activate_fn(&self) -> ItemActivateFn {
        // SAFETY: module identity validation fixes the RVA and ABI.
        unsafe { mem::transmute(self.module_base + ITEM_ACTIVATE_RVA) }
    }
}

struct Item {
    pane: NonNull<c_void>,
}

impl Item {
    fn validate_quantity(&self, quantity: u32) -> Result<(), CommandFailure> {
        let base = self.pane.as_ptr() as usize;
        let available =
            read::<u32>(base + ITEM_QUANTITY_OFFSET).ok_or(CommandFailure::InvalidState)?;
        let can_stack =
            read::<u8>(base + ITEM_STACKABLE_OFFSET).ok_or(CommandFailure::InvalidState)? != 0;
        if quantity == 0 || quantity > available || (!can_stack && quantity != 1) {
            return Err(CommandFailure::InvalidArguments);
        }
        Ok(())
    }
}
