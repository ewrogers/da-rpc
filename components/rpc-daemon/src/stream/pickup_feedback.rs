use super::{ClientEvent, EventObservation, TilePosition};
use crate::registry::ClientIdentity;
use darpc_model::{ActionUpdate, ClientMessage, MessageKind, StateEvent, StateUpdate};
use serde::Serialize;
use std::collections::{BTreeMap, VecDeque};
use utoipa::ToSchema;

const PENDING_PICKUP_CAPACITY: usize = 256;
const PENDING_PICKUP_TTL_MS: u32 = 5_000;
const LIMIT_SEPARATOR: &str = ", you can't have more than ";

#[derive(Clone, Debug)]
pub(crate) struct PickupFeedback {
    attempt: PendingPickup,
    item_name: String,
    limit: u32,
    feedback: String,
    elapsed_ms: u32,
}

impl PickupFeedback {
    pub(super) fn into_event(self, observation: EventObservation) -> ClientEvent {
        ClientEvent::ItemPickupFailed(ItemPickupFailed {
            observation,
            destination_slot: self.attempt.destination_slot,
            position: self.attempt.position.into(),
            item_name: self.item_name,
            limit: self.limit,
            reason: ItemPickupFailureReason::CarryLimit,
            feedback: self.feedback,
            submitted_tick_ms: self.attempt.submitted_tick_ms,
            elapsed_ms: self.elapsed_ms,
        })
    }
}

#[derive(Clone, Debug, Serialize, ToSchema)]
/// A submitted ground-item pickup was rejected by client feedback.
pub(crate) struct ItemPickupFailed {
    pub(super) observation: EventObservation,
    destination_slot: u8,
    position: TilePosition,
    item_name: String,
    limit: u32,
    reason: ItemPickupFailureReason,
    feedback: String,
    submitted_tick_ms: u32,
    elapsed_ms: u32,
}

#[derive(Clone, Copy, Debug, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ItemPickupFailureReason {
    CarryLimit,
}

#[derive(Default)]
pub(crate) struct PickupFeedbackTrackers {
    clients: BTreeMap<ClientIdentity, PickupFeedbackTracker>,
}

impl PickupFeedbackTrackers {
    pub(crate) fn observe(
        &mut self,
        identity: ClientIdentity,
        event: &StateEvent,
    ) -> Option<PickupFeedback> {
        self.clients.entry(identity).or_default().observe(event)
    }

    pub(crate) fn remove(&mut self, identity: ClientIdentity) {
        self.clients.remove(&identity);
    }
}

#[derive(Default)]
struct PickupFeedbackTracker {
    pending: VecDeque<PendingPickup>,
}

impl PickupFeedbackTracker {
    fn observe(&mut self, event: &StateEvent) -> Option<PickupFeedback> {
        self.expire(event.tick_ms);
        match &event.update {
            StateUpdate::Action(ActionUpdate::ItemPickedUp {
                destination_slot,
                position,
            }) => {
                self.push(PendingPickup {
                    destination_slot: *destination_slot,
                    position: *position,
                    submitted_tick_ms: event.tick_ms,
                });
                None
            }
            StateUpdate::Inventory(update) if update.after.is_some() => {
                self.complete(update.slot);
                None
            }
            StateUpdate::Message(message) if message.kind == MessageKind::System => {
                self.correlate(event.tick_ms, message)
            }
            _ => None,
        }
    }

    fn push(&mut self, pickup: PendingPickup) {
        if self.pending.len() == PENDING_PICKUP_CAPACITY {
            self.pending.pop_front();
        }
        self.pending.push_back(pickup);
    }

    fn complete(&mut self, destination_slot: u8) {
        if let Some(index) = self
            .pending
            .iter()
            .position(|pickup| pickup.destination_slot == destination_slot)
        {
            self.pending.remove(index);
        }
    }

    fn expire(&mut self, now: u32) {
        while self.pending.front().is_some_and(|pickup| {
            now.wrapping_sub(pickup.submitted_tick_ms) > PENDING_PICKUP_TTL_MS
        }) {
            self.pending.pop_front();
        }
    }

    fn correlate(&mut self, tick_ms: u32, message: &ClientMessage) -> Option<PickupFeedback> {
        let feedback = message.text.trim();
        let (item_name, limit) = parse_limit_feedback(feedback)?;
        if self.pending.len() != 1 {
            // Feedback contains the item name but no tile or destination slot.
            // Clear ambiguous candidates so later feedback cannot inherit them.
            self.pending.clear();
            return None;
        }
        let attempt = self.pending.pop_front()?;
        Some(PickupFeedback {
            elapsed_ms: tick_ms.wrapping_sub(attempt.submitted_tick_ms),
            attempt,
            item_name,
            limit,
            feedback: feedback.to_owned(),
        })
    }
}

#[derive(Clone, Copy, Debug)]
struct PendingPickup {
    destination_slot: u8,
    position: darpc_model::TilePosition,
    submitted_tick_ms: u32,
}

fn parse_limit_feedback(message: &str) -> Option<(String, u32)> {
    let trimmed = message.trim().trim_end_matches(['.', '!', '?']);
    let normalized = trimmed.to_ascii_lowercase();
    let separator = normalized.rfind(LIMIT_SEPARATOR)?;
    let item_name = trimmed.get(..separator)?.trim();
    let limit = trimmed
        .get(separator + LIMIT_SEPARATOR.len()..)?
        .trim()
        .parse::<u32>()
        .ok()?;
    (!item_name.is_empty() && limit > 0).then(|| (item_name.to_owned(), limit))
}

#[cfg(test)]
mod tests {
    use super::*;
    use darpc_model::{CollectionChange, InventoryItem, SlotUpdate};

    fn pickup(sequence: u32, tick_ms: u32, destination_slot: u8) -> StateEvent {
        StateEvent {
            sequence,
            revision: sequence,
            tick_ms,
            update: StateUpdate::Action(ActionUpdate::ItemPickedUp {
                destination_slot,
                position: darpc_model::TilePosition { x: 12, y: 34 },
            }),
        }
    }

    fn message(sequence: u32, tick_ms: u32, text: &str) -> StateEvent {
        StateEvent {
            sequence,
            revision: sequence,
            tick_ms,
            update: StateUpdate::Message(ClientMessage {
                kind: MessageKind::System,
                sender_id: None,
                sender_type: None,
                sender: None,
                recipient: None,
                text: text.into(),
            }),
        }
    }

    fn inventory_added(sequence: u32, tick_ms: u32, slot: u8) -> StateEvent {
        StateEvent {
            sequence,
            revision: sequence,
            tick_ms,
            update: StateUpdate::Inventory(SlotUpdate {
                batch_index: 0,
                batch_count: 1,
                change: CollectionChange::Added,
                slot,
                before: None,
                after: Some(InventoryItem {
                    slot,
                    sprite: 1,
                    dye_color: 0,
                    name: Some("Centipede's Gland".into()),
                    quantity: 1,
                    can_stack: true,
                    durability: 0,
                    max_durability: 0,
                }),
            }),
        }
    }

    #[test]
    fn parses_item_name_and_limit_from_feedback() {
        assert_eq!(
            parse_limit_feedback("Centipede's Gland, You can't have more than 15."),
            Some(("Centipede's Gland".into(), 15))
        );
        assert_eq!(
            parse_limit_feedback("Hy-Brasyl, Ore, YOU CAN'T HAVE MORE THAN 2!"),
            Some(("Hy-Brasyl, Ore".into(), 2))
        );
    }

    #[test]
    fn rejects_inexact_or_invalid_feedback() {
        for message in [
            "You can't carry any more.",
            ", You can't have more than 15.",
            "Centipede's Gland, You can't have more than 0.",
            "Centipede's Gland, You can't have more than many.",
        ] {
            assert_eq!(parse_limit_feedback(message), None);
        }
    }

    #[test]
    fn correlates_one_pending_pickup() {
        let mut tracker = PickupFeedbackTracker::default();
        tracker.observe(&pickup(1, 100, 7));
        let feedback = tracker
            .observe(&message(
                2,
                145,
                "Centipede's Gland, You can't have more than 15.",
            ))
            .unwrap();

        assert_eq!(feedback.attempt.destination_slot, 7);
        assert_eq!(feedback.attempt.position.x, 12);
        assert_eq!(feedback.item_name, "Centipede's Gland");
        assert_eq!(feedback.limit, 15);
        assert_eq!(feedback.elapsed_ms, 45);
        assert!(tracker.pending.is_empty());
    }

    #[test]
    fn successful_inventory_update_retires_the_attempt() {
        let mut tracker = PickupFeedbackTracker::default();
        tracker.observe(&pickup(1, 100, 7));
        tracker.observe(&inventory_added(2, 120, 7));

        assert!(
            tracker
                .observe(&message(
                    3,
                    145,
                    "Centipede's Gland, You can't have more than 15.",
                ))
                .is_none()
        );
        assert!(tracker.pending.is_empty());
    }

    #[test]
    fn unrelated_feedback_and_other_slots_do_not_consume_the_attempt() {
        let mut tracker = PickupFeedbackTracker::default();
        tracker.observe(&pickup(1, 100, 7));
        tracker.observe(&message(2, 110, "You have gained experience."));
        tracker.observe(&inventory_added(3, 120, 8));

        assert!(
            tracker
                .observe(&message(
                    4,
                    145,
                    "Centipede's Gland, You can't have more than 15.",
                ))
                .is_some()
        );
    }

    #[test]
    fn expired_attempt_does_not_receive_late_feedback() {
        let mut tracker = PickupFeedbackTracker::default();
        tracker.observe(&pickup(1, 100, 7));

        assert!(
            tracker
                .observe(&message(
                    2,
                    PENDING_PICKUP_TTL_MS + 101,
                    "Centipede's Gland, You can't have more than 15.",
                ))
                .is_none()
        );
        assert!(tracker.pending.is_empty());
    }

    #[test]
    fn ambiguous_feedback_clears_pending_attempts() {
        let mut tracker = PickupFeedbackTracker::default();
        tracker.observe(&pickup(1, 100, 7));
        tracker.observe(&pickup(2, 110, 8));

        assert!(
            tracker
                .observe(&message(
                    3,
                    145,
                    "Centipede's Gland, You can't have more than 15.",
                ))
                .is_none()
        );
        assert!(tracker.pending.is_empty());
    }

    #[test]
    fn semantic_event_retains_attempt_and_feedback_context() {
        let mut tracker = PickupFeedbackTracker::default();
        tracker.observe(&pickup(1, 100, 7));
        let feedback = tracker
            .observe(&message(
                2,
                145,
                "Centipede's Gland, You can't have more than 15.",
            ))
            .unwrap();
        let observation_event = message(2, 145, "Centipede's Gland, You can't have more than 15.");
        let event = feedback.into_event(EventObservation::new(
            42,
            ClientIdentity {
                pid: 42,
                process_creation_time: 100,
                dll_instance_id: [1; 16],
            },
            &observation_event,
        ));

        assert_eq!(event.name(), "item.pickup_failed");
        let event = serde_json::to_value(event).unwrap();
        assert_eq!(event["type"], "item_pickup_failed");
        assert_eq!(event["data"]["destination_slot"], 7);
        assert_eq!(event["data"]["position"]["x"], 12);
        assert_eq!(event["data"]["position"]["y"], 34);
        assert_eq!(event["data"]["item_name"], "Centipede's Gland");
        assert_eq!(event["data"]["limit"], 15);
        assert_eq!(event["data"]["reason"], "carry_limit");
        assert_eq!(event["data"]["submitted_tick_ms"], 100);
        assert_eq!(event["data"]["elapsed_ms"], 45);
    }
}
