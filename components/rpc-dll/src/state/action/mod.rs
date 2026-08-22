use super::{QueuedStateUpdate, begin_object_reconciliation, push_event};
use darpc_model::{ActionUpdate, Direction, EquipmentSlot, TilePosition};

pub(super) fn observe_outgoing(body: &[u8], tick_ms: u32) {
    let Some(update) = parse(body) else {
        return;
    };
    if matches!(update, ActionUpdate::Resync) {
        begin_object_reconciliation();
    }
    push_event(QueuedStateUpdate::Action(update), tick_ms);
}

fn parse(body: &[u8]) -> Option<ActionUpdate> {
    match *body.first()? {
        0x07 if body.len() == 6 => Some(ActionUpdate::ItemPickedUp {
            destination_slot: body[1],
            position: position(body, 2, 4)?,
        }),
        0x08 if body.len() == 10 => Some(ActionUpdate::ItemDropped {
            slot: body[1],
            quantity: u32::from_be_bytes(body[6..10].try_into().ok()?),
            position: position(body, 2, 4)?,
        }),
        0x11 if body.len() == 2 => {
            Direction::from_raw(body[1]).map(|direction| ActionUpdate::Turned { direction })
        }
        0x1C if body.len() == 2 => Some(ActionUpdate::ItemUsed { slot: body[1] }),
        0x1D if body.len() == 2 => Some(ActionUpdate::Emoted { code: body[1] }),
        0x24 if body.len() == 9 => Some(ActionUpdate::GoldDropped {
            amount: u32::from_be_bytes(body[1..5].try_into().ok()?),
            position: position(body, 5, 7)?,
        }),
        0x29 if body.len() == 10 => Some(ActionUpdate::ItemGiven {
            slot: body[1],
            object_id: u32::from_be_bytes(body[2..6].try_into().ok()?),
            quantity: u32::from_be_bytes(body[6..10].try_into().ok()?),
        }),
        0x2A if body.len() == 9 => Some(ActionUpdate::GoldGiven {
            amount: u32::from_be_bytes(body[1..5].try_into().ok()?),
            object_id: u32::from_be_bytes(body[5..9].try_into().ok()?),
        }),
        0x38 if body.len() == 1 => Some(ActionUpdate::Resync),
        0x44 if body.len() == 2 => {
            EquipmentSlot::from_raw(body[1]).map(|slot| ActionUpdate::EquipmentUnequipped { slot })
        }
        _ => None,
    }
}

fn position(body: &[u8], x: usize, y: usize) -> Option<TilePosition> {
    Some(TilePosition {
        x: i32::from(u16::from_be_bytes(body[x..x + 2].try_into().ok()?)),
        y: i32::from(u16::from_be_bytes(body[y..y + 2].try_into().ok()?)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_item_and_gold_drops() {
        assert_eq!(
            parse(&[0x08, 3, 0, 2, 0, 8, 0, 0, 0, 4]),
            Some(ActionUpdate::ItemDropped {
                slot: 3,
                quantity: 4,
                position: TilePosition { x: 2, y: 8 },
            })
        );
        assert_eq!(
            parse(&[0x24, 0, 0, 0, 100, 0, 2, 0, 8]),
            Some(ActionUpdate::GoldDropped {
                amount: 100,
                position: TilePosition { x: 2, y: 8 },
            })
        );
    }

    #[test]
    fn parses_client_resync() {
        assert_eq!(parse(&[0x38]), Some(ActionUpdate::Resync));
        assert_eq!(parse(&[0x38, 0]), None);
    }
}
