//! Server-event interpretation behind the bounded hook seam.
//!
//! The event hook validates client memory and copies one bounded packet body.
//! This module owns everything that follows: command-response interception,
//! packet parsing, reusable parser scratch, and ordered semantic dispatch.
//! Processing remains synchronous so observed state preserves client order and
//! is available immediately after the client dispatch returns.

use crate::{packet, state};
use darpc_game_client::RawObjects;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Observation {
    Ignored,
    Observed,
}

pub(crate) struct ServerEventProcessor {
    objects: RawObjects,
}

impl ServerEventProcessor {
    pub(crate) const fn new() -> Self {
        Self {
            objects: RawObjects::empty(),
        }
    }

    #[cfg(windows)]
    pub(crate) fn intercept(body: &[u8]) -> bool {
        match body.first() {
            Some(0x0a) => {
                crate::look::intercept_response(body, darpc_win32::pipe::sender_tick_ms())
            }
            Some(0x34) => {
                crate::player::intercept_response(body, darpc_win32::pipe::sender_tick_ms())
            }
            Some(0x36) => crate::who::intercept_response(body, darpc_win32::pipe::sender_tick_ms()),
            Some(0x39) => {
                crate::player::intercept_self_response(body, darpc_win32::pipe::sender_tick_ms())
            }
            Some(0x42) => crate::exchange::intercept_quantity(body),
            _ => false,
        }
    }

    pub(crate) fn observe(
        &mut self,
        body: &[u8],
        tick_ms: u32,
    ) -> Result<Observation, packet::ParseError> {
        state::observe_message_dialog(body, tick_ms);
        if body.first() == Some(&0x34) {
            crate::player::observe_user_response(body, tick_ms);
            return Ok(Observation::Observed);
        }

        let Some(update) = packet::update(body, &mut self.objects)? else {
            return Ok(Observation::Ignored);
        };
        self.dispatch(update, tick_ms);
        Ok(Observation::Observed)
    }

    fn dispatch(&self, update: packet::ServerUpdate<'_>, tick_ms: u32) {
        match update {
            packet::ServerUpdate::ActionDelay(update) => {
                state::observe_action_delay(
                    update.kind,
                    update.slot,
                    update.duration_seconds,
                    tick_ms,
                );
            }
            packet::ServerUpdate::Audio(update) => {
                state::observe_audio(update, tick_ms);
            }
            packet::ServerUpdate::Status(update) => {
                state::observe_status(update, tick_ms);
            }
            packet::ServerUpdate::StatPoints(stat_points) => {
                state::observe_stat_points(stat_points, tick_ms);
            }
            packet::ServerUpdate::UserAppearance(update) => {
                state::observe_status(update.status, tick_ms);
                if update.is_full {
                    state::mark_refresh_snapshot_required();
                }
            }
            packet::ServerUpdate::UserPosition(position) => {
                state::observe_user_position(position.x, position.y, tick_ms);
                state::observe_position_correction();
            }
            packet::ServerUpdate::Move(update) => {
                state::observe_move(update.position.x, update.position.y, tick_ms);
                if update.corrected {
                    state::observe_position_correction();
                }
            }
            packet::ServerUpdate::Effect(effect) => {
                state::observe_effect(effect.icon, effect.duration, tick_ms);
            }
            packet::ServerUpdate::World(update) => {
                state::observe_world(update, &self.objects, tick_ms);
                if matches!(update, packet::object::WorldUpdate::DrawPlayer) {
                    state::mark_refresh_snapshot_required();
                    if let Some(player) = self.objects.entries[0] {
                        crate::player::appeared(player);
                    }
                }
            }
            packet::ServerUpdate::Message(message) => {
                state::observe_message(message, tick_ms);
            }
            packet::ServerUpdate::Collection(collection) => {
                state::mark_collection_dirty(collection.kind, collection.slot, tick_ms);
            }
            packet::ServerUpdate::SpellCancelled => {
                state::observe_spell_cancelled(tick_ms);
            }
            packet::ServerUpdate::Visual(update) => {
                state::observe_visual(update, tick_ms);
            }
            packet::ServerUpdate::MapPart(update) => {
                state::observe_map_part(update.row_index, update.body_length, tick_ms);
            }
            packet::ServerUpdate::Bulletin(body) => {
                state::observe_bulletin(body, tick_ms);
            }
            packet::ServerUpdate::FieldMap(body) => {
                state::observe_field_map(body, tick_ms);
            }
            packet::ServerUpdate::Dialog(body) => {
                state::observe_dialog(body, tick_ms);
            }
            packet::ServerUpdate::Group(body) => {
                crate::player::observe_self_look(body, tick_ms);
                crate::group::observe_packet(body, tick_ms);
            }
            packet::ServerUpdate::Exchange(body) => {
                crate::exchange::observe_server(body, tick_ms);
            }
            packet::ServerUpdate::ResyncCompleted => {
                state::observe_resync_completed(tick_ms);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Observation, ServerEventProcessor};

    #[test]
    fn interface_classifies_recognized_and_unknown_updates() {
        let _guard = crate::state::TEST_LOCK.lock().unwrap();
        let mut processor = ServerEventProcessor::new();

        let status = [0x08, 0x10, 0, 0, 0, 1, 0, 0, 0, 2];
        assert_eq!(processor.observe(&status, 100), Ok(Observation::Observed));
        assert_eq!(processor.observe(&[0xFF], 101), Ok(Observation::Ignored));
    }

    #[test]
    fn interface_reports_parse_failures() {
        let _guard = crate::state::TEST_LOCK.lock().unwrap();
        let mut processor = ServerEventProcessor::new();

        let error = processor.observe(&[0x08], 100).unwrap_err();
        assert_eq!(error.offset(), 1);
    }
}
