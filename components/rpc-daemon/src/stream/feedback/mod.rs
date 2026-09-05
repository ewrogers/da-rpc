use super::{ClientEvent, EventObservation, SpellCastArguments, spell_arguments};
use crate::{registry::ClientIdentity, state::WorldObject};
use darpc_model::{AbilityUpdate, ClientMessage, MessageKind, StateEvent, StateUpdate};
use serde::Serialize;
use std::collections::{BTreeMap, VecDeque};
use utoipa::ToSchema;

const PENDING_CAST_CAPACITY: usize = 256;
const PENDING_CAST_TTL_MS: u32 = 5_000;

#[derive(Clone, Debug)]
pub(crate) enum SpellFeedback {
    Succeeded(ResolvedCast),
    Failed {
        cast: ResolvedCast,
        reason: SpellFailureReason,
        active_spell: Option<String>,
    },
    Received {
        caster: String,
        caster_object: Option<WorldObject>,
        name: String,
        kind: ReceivedSpellKind,
        feedback: String,
    },
}

impl SpellFeedback {
    pub(super) fn into_event(self, observation: EventObservation) -> ClientEvent {
        match self {
            Self::Succeeded(cast) => ClientEvent::SpellSucceeded(SpellSucceeded {
                observation,
                slot: cast.slot,
                name: cast.name,
                arguments: spell_arguments(cast.arguments, cast.target_name),
                feedback: cast.feedback,
                submitted_tick_ms: cast.submitted_tick_ms,
                elapsed_ms: cast.elapsed_ms,
            }),
            Self::Failed {
                cast,
                reason,
                active_spell,
            } => ClientEvent::SpellFailed(SpellFailed {
                observation,
                slot: cast.slot,
                name: cast.name,
                arguments: spell_arguments(cast.arguments, cast.target_name),
                reason,
                active_spell,
                feedback: cast.feedback,
                submitted_tick_ms: cast.submitted_tick_ms,
                elapsed_ms: cast.elapsed_ms,
            }),
            Self::Received {
                caster,
                caster_object,
                name,
                kind,
                feedback,
            } => ClientEvent::SpellReceived(SpellReceived {
                observation,
                caster,
                caster_object,
                name,
                kind,
                feedback,
            }),
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct ResolvedCast {
    slot: u8,
    name: Option<String>,
    arguments: darpc_model::SpellCastArguments,
    target_name: Option<String>,
    feedback: String,
    submitted_tick_ms: u32,
    elapsed_ms: u32,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
/// A submitted spell was confirmed by client feedback.
pub(crate) struct SpellSucceeded {
    pub(super) observation: EventObservation,
    slot: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    arguments: Option<SpellCastArguments>,
    feedback: String,
    submitted_tick_ms: u32,
    elapsed_ms: u32,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
/// A submitted spell was rejected or failed according to client feedback.
pub(crate) struct SpellFailed {
    pub(super) observation: EventObservation,
    slot: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    arguments: Option<SpellCastArguments>,
    reason: SpellFailureReason,
    /// Existing spell named by feedback such as the curse conflict message.
    #[serde(skip_serializing_if = "Option::is_none")]
    active_spell: Option<String>,
    feedback: String,
    submitted_tick_ms: u32,
    elapsed_ms: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SpellFailureReason {
    Failed,
    Error,
    Resisted,
    AlreadyActive,
    ConflictingEffect,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
/// Another creature or player cast a spell on this character.
pub(crate) struct SpellReceived {
    pub(super) observation: EventObservation,
    caster: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    caster_object: Option<WorldObject>,
    name: String,
    kind: ReceivedSpellKind,
    feedback: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ReceivedSpellKind {
    Cast,
    Attack,
}

#[derive(Default)]
pub(crate) struct SpellFeedbackTrackers {
    clients: BTreeMap<ClientIdentity, SpellFeedbackTracker>,
}

impl SpellFeedbackTrackers {
    pub(crate) fn observe(
        &mut self,
        identity: ClientIdentity,
        snapshot: Option<&darpc_model::ClientSnapshot>,
        event: &StateEvent,
        ability_name: Option<&str>,
        target_name: Option<&str>,
    ) -> Option<SpellFeedback> {
        self.clients.entry(identity).or_default().observe(
            snapshot,
            event,
            ability_name,
            target_name,
        )
    }

    pub(crate) fn remove(&mut self, identity: ClientIdentity) {
        self.clients.remove(&identity);
    }
}

#[derive(Default)]
struct SpellFeedbackTracker {
    pending: VecDeque<PendingCast>,
}

impl SpellFeedbackTracker {
    fn observe(
        &mut self,
        snapshot: Option<&darpc_model::ClientSnapshot>,
        event: &StateEvent,
        ability_name: Option<&str>,
        target_name: Option<&str>,
    ) -> Option<SpellFeedback> {
        self.expire(event.tick_ms);
        match &event.update {
            StateUpdate::Ability(AbilityUpdate::SpellCast { slot, arguments }) => {
                self.push(PendingCast {
                    slot: *slot,
                    name: ability_name.map(str::to_owned),
                    arguments: arguments.clone(),
                    target_name: target_name.map(str::to_owned),
                    submitted_tick_ms: event.tick_ms,
                });
                None
            }
            StateUpdate::Message(message) if message.kind == MessageKind::System => {
                self.correlate(snapshot, event.tick_ms, message)
            }
            _ => None,
        }
    }

    fn push(&mut self, cast: PendingCast) {
        if self.pending.len() == PENDING_CAST_CAPACITY {
            self.pending.pop_front();
        }
        self.pending.push_back(cast);
    }

    fn expire(&mut self, now: u32) {
        while self
            .pending
            .front()
            .is_some_and(|cast| now.wrapping_sub(cast.submitted_tick_ms) > PENDING_CAST_TTL_MS)
        {
            self.pending.pop_front();
        }
    }

    fn correlate(
        &mut self,
        snapshot: Option<&darpc_model::ClientSnapshot>,
        tick_ms: u32,
        message: &ClientMessage,
    ) -> Option<SpellFeedback> {
        let feedback = message.text.trim();
        if feedback.is_empty() {
            return None;
        }
        if let Some((caster, name, kind)) = parse_received(feedback) {
            return Some(SpellFeedback::Received {
                caster_object: resolve_caster(snapshot, &caster),
                caster,
                name,
                kind,
                feedback: feedback.to_owned(),
            });
        }
        if let Some(name) = parse_success(feedback) {
            let index = self.pending.iter().position(|cast| {
                cast.name
                    .as_deref()
                    .is_some_and(|pending| pending.eq_ignore_ascii_case(&name))
            })?;
            let cast = self.pending.remove(index)?;
            return Some(SpellFeedback::Succeeded(cast.resolve(tick_ms, feedback)));
        }
        if is_fas_spiorad_concentration_failure(feedback) {
            let index = self.pending.iter().position(|cast| {
                cast.name
                    .as_deref()
                    .is_some_and(|name| name.eq_ignore_ascii_case("Fas Spiorad"))
            })?;
            let cast = self.pending.remove(index)?;
            return Some(SpellFeedback::Failed {
                cast: cast.resolve(tick_ms, feedback),
                reason: SpellFailureReason::Failed,
                active_spell: None,
            });
        }
        let (reason, active_spell) = parse_failure(feedback)?;
        if self.pending.len() != 1 {
            // Identifier-free feedback cannot distinguish these casts. Clear
            // every candidate so later feedback cannot inherit stale context.
            self.pending.clear();
            return None;
        }
        let cast = self.pending.pop_front()?;
        Some(SpellFeedback::Failed {
            cast: cast.resolve(tick_ms, feedback),
            reason,
            active_spell,
        })
    }
}

fn is_fas_spiorad_concentration_failure(message: &str) -> bool {
    message
        .trim()
        .trim_end_matches(['.', '!', '?'])
        .eq_ignore_ascii_case("you failed to concentrate")
}

#[derive(Clone, Debug)]
struct PendingCast {
    slot: u8,
    name: Option<String>,
    arguments: darpc_model::SpellCastArguments,
    target_name: Option<String>,
    submitted_tick_ms: u32,
}

impl PendingCast {
    fn resolve(self, tick_ms: u32, feedback: &str) -> ResolvedCast {
        ResolvedCast {
            slot: self.slot,
            name: self.name,
            arguments: self.arguments,
            target_name: self.target_name,
            feedback: feedback.to_owned(),
            submitted_tick_ms: self.submitted_tick_ms,
            elapsed_ms: tick_ms.wrapping_sub(self.submitted_tick_ms),
        }
    }
}

fn parse_success(message: &str) -> Option<String> {
    const PREFIX: &str = "you cast ";
    let trimmed = message.trim().trim_end_matches(['.', '!', '?']);
    trimmed
        .get(..PREFIX.len())
        .filter(|prefix| prefix.eq_ignore_ascii_case(PREFIX))?;
    let name = trimmed.get(PREFIX.len()..)?.trim();
    (!name.is_empty()).then(|| name.to_owned())
}

fn parse_failure(message: &str) -> Option<(SpellFailureReason, Option<String>)> {
    let normalized = message
        .trim()
        .trim_end_matches(['.', '!', '?'])
        .to_ascii_lowercase();
    match normalized.as_str() {
        "failed" => Some((SpellFailureReason::Failed, None)),
        "something went wrong" => Some((SpellFailureReason::Error, None)),
        "the magic has been deflected" => Some((SpellFailureReason::Resisted, None)),
        "you already cast that spell" => Some((SpellFailureReason::AlreadyActive, None)),
        _ if normalized.starts_with("another curse afflicts thee") => Some((
            SpellFailureReason::ConflictingEffect,
            bracketed_name(message),
        )),
        _ => None,
    }
}

fn parse_received(message: &str) -> Option<(String, String, ReceivedSpellKind)> {
    let trimmed = message.trim().trim_end_matches(['.', '!', '?']);
    strip_case_insensitive_suffix(trimmed, " spell on you")
        .and_then(|body| split_case_insensitive(body, " cast "))
        .map(|(caster, name)| (caster, name, ReceivedSpellKind::Cast))
        .or_else(|| {
            strip_case_insensitive_suffix(trimmed, " spell")
                .and_then(|body| split_case_insensitive(body, " attacks you with "))
                .map(|(caster, name)| (caster, name, ReceivedSpellKind::Attack))
        })
        .filter(|(caster, name, _)| !caster.is_empty() && !name.is_empty())
}

fn strip_case_insensitive_suffix<'a>(value: &'a str, suffix: &str) -> Option<&'a str> {
    let start = value.len().checked_sub(suffix.len())?;
    if !value.get(start..)?.eq_ignore_ascii_case(suffix) {
        return None;
    }
    value.get(..start)
}

fn split_case_insensitive(value: &str, separator: &str) -> Option<(String, String)> {
    let index = value.to_ascii_lowercase().find(separator)?;
    Some((
        value[..index].trim().to_owned(),
        value[index + separator.len()..].trim().to_owned(),
    ))
}

fn bracketed_name(message: &str) -> Option<String> {
    let start = message.rfind('[')? + 1;
    let end = message[start..].find(']')? + start;
    let name = message[start..end].trim();
    (!name.is_empty()).then(|| name.to_owned())
}

fn resolve_caster(
    snapshot: Option<&darpc_model::ClientSnapshot>,
    caster: &str,
) -> Option<WorldObject> {
    snapshot?
        .objects
        .as_ref()?
        .iter()
        .find(|object| match object {
            darpc_model::WorldObject::Player { name, .. }
            | darpc_model::WorldObject::Creature { name, .. } => name
                .as_deref()
                .is_some_and(|name| name.eq_ignore_ascii_case(caster)),
            darpc_model::WorldObject::Item { .. } => false,
        })
        .map(WorldObject::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use darpc_model::{SpellCastArguments as ModelArguments, StateEvent};

    fn cast(sequence: u32, tick_ms: u32, slot: u8) -> StateEvent {
        StateEvent {
            sequence,
            revision: sequence,
            tick_ms,
            update: StateUpdate::Ability(AbilityUpdate::SpellCast {
                slot,
                arguments: ModelArguments::None,
            }),
        }
    }

    fn targeted_cast(sequence: u32, tick_ms: u32, slot: u8, id: u32) -> StateEvent {
        StateEvent {
            sequence,
            revision: sequence,
            tick_ms,
            update: StateUpdate::Ability(AbilityUpdate::SpellCast {
                slot,
                arguments: ModelArguments::Target {
                    id: Some(id),
                    x: 0,
                    y: 0,
                },
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

    #[test]
    fn success_matches_the_oldest_cast_with_the_same_name() {
        let mut tracker = SpellFeedbackTracker::default();
        tracker.observe(None, &cast(1, 100, 1), Some("Mist"), None);
        tracker.observe(None, &cast(2, 110, 2), Some("Dion"), None);

        let feedback = tracker
            .observe(None, &message(3, 130, "You cast Dion."), None, None)
            .unwrap();
        let SpellFeedback::Succeeded(cast) = feedback else {
            panic!("expected successful cast");
        };
        assert_eq!(cast.slot, 2);
        assert_eq!(cast.name.as_deref(), Some("Dion"));
        assert_eq!(cast.elapsed_ms, 20);
        assert_eq!(tracker.pending.len(), 1);
    }

    #[test]
    fn generic_failure_consumes_the_oldest_pending_cast() {
        let mut tracker = SpellFeedbackTracker::default();
        tracker.observe(None, &cast(1, 100, 1), Some("Dion"), None);
        let feedback = tracker
            .observe(None, &message(2, 125, "failed"), None, None)
            .unwrap();
        let SpellFeedback::Failed { cast, reason, .. } = feedback else {
            panic!("expected failed cast");
        };
        assert_eq!(cast.slot, 1);
        assert!(matches!(reason, SpellFailureReason::Failed));
        assert!(tracker.pending.is_empty());
    }

    #[test]
    fn out_of_order_failure_feedback_does_not_claim_cast_context() {
        let identity = ClientIdentity {
            pid: 42,
            process_creation_time: 100,
            dll_instance_id: [1; 16],
        };
        let mut trackers = SpellFeedbackTrackers::default();
        trackers.observe(
            identity,
            None,
            &targeted_cast(1, 100, 4, 54_691),
            Some("Ard Fas Nadur"),
            Some("first monster"),
        );
        trackers.observe(
            identity,
            None,
            &targeted_cast(2, 110, 4, 54_689),
            Some("Ard Fas Nadur"),
            Some("second monster"),
        );

        assert!(
            trackers
                .observe(
                    identity,
                    None,
                    &message(3, 130, "You already cast that spell."),
                    None,
                    None,
                )
                .is_none()
        );
        assert!(
            trackers
                .observe(
                    identity,
                    None,
                    &message(4, 140, "You already cast that spell."),
                    None,
                    None,
                )
                .is_none()
        );
    }

    #[test]
    fn concentration_failure_matches_fas_spiorad_by_name() {
        let mut tracker = SpellFeedbackTracker::default();
        tracker.observe(None, &cast(1, 100, 1), Some("Mist"), None);
        tracker.observe(None, &cast(2, 110, 2), Some("Fas Spiorad"), None);

        let feedback = tracker
            .observe(
                None,
                &message(3, 135, "You failed to concentrate."),
                None,
                None,
            )
            .unwrap();
        let SpellFeedback::Failed { cast, reason, .. } = feedback else {
            panic!("expected failed cast");
        };
        assert_eq!(cast.slot, 2);
        assert_eq!(cast.name.as_deref(), Some("Fas Spiorad"));
        assert_eq!(cast.elapsed_ms, 25);
        assert!(matches!(reason, SpellFailureReason::Failed));
        assert_eq!(tracker.pending.len(), 1);
        assert_eq!(tracker.pending.front().unwrap().slot, 1);
    }

    #[test]
    fn expired_casts_and_overflow_are_discarded() {
        let mut tracker = SpellFeedbackTracker::default();
        for index in 0..=PENDING_CAST_CAPACITY {
            tracker.observe(
                None,
                &cast(index as u32 + 1, index as u32, index as u8),
                Some("Mist"),
                None,
            );
        }
        assert_eq!(tracker.pending.len(), PENDING_CAST_CAPACITY);
        assert_eq!(tracker.pending.front().unwrap().submitted_tick_ms, 1);

        assert!(
            tracker
                .observe(
                    None,
                    &message(300, PENDING_CAST_TTL_MS + 300, "failed"),
                    None,
                    None,
                )
                .is_none()
        );
        assert!(tracker.pending.is_empty());
    }

    #[test]
    fn curse_conflict_reports_the_existing_spell() {
        let mut tracker = SpellFeedbackTracker::default();
        tracker.observe(None, &cast(1, 100, 3), Some("Ard Cradh"), None);
        let feedback = tracker
            .observe(
                None,
                &message(2, 120, "Another curse afflicts thee. [beag cradh]"),
                None,
                None,
            )
            .unwrap();
        let SpellFeedback::Failed {
            active_spell,
            reason,
            ..
        } = feedback
        else {
            panic!("expected failed cast");
        };
        assert!(matches!(reason, SpellFailureReason::ConflictingEffect));
        assert_eq!(active_spell.as_deref(), Some("beag cradh"));
    }

    #[test]
    fn received_cast_and_attack_messages_are_parsed_without_a_pending_cast() {
        for (text, caster, name, kind) in [
            (
                "ZiLo cast Mor Fas Nadur spell on you.",
                "ZiLo",
                "Mor Fas Nadur",
                ReceivedSpellKind::Cast,
            ),
            (
                "Beggar attacks you with Beag Cradh spell!",
                "Beggar",
                "Beag Cradh",
                ReceivedSpellKind::Attack,
            ),
        ] {
            let mut tracker = SpellFeedbackTracker::default();
            let feedback = tracker
                .observe(None, &message(1, 100, text), None, None)
                .unwrap();
            let SpellFeedback::Received {
                caster: actual_caster,
                name: actual_name,
                kind: actual_kind,
                ..
            } = feedback
            else {
                panic!("expected received spell");
            };
            assert_eq!(actual_caster, caster);
            assert_eq!(actual_name, name);
            assert_eq!(actual_kind, kind);
        }
    }

    #[test]
    fn semantic_feedback_keeps_cast_context_in_the_public_event() {
        let mut tracker = SpellFeedbackTracker::default();
        tracker.observe(
            None,
            &StateEvent {
                sequence: 1,
                revision: 1,
                tick_ms: 100,
                update: StateUpdate::Ability(AbilityUpdate::SpellCast {
                    slot: 4,
                    arguments: ModelArguments::Target {
                        id: Some(77),
                        x: 10,
                        y: 12,
                    },
                }),
            },
            Some("Ao Puinsein"),
            Some("ZiLo"),
        );
        let feedback = tracker
            .observe(
                None,
                &message(2, 145, "The magic has been deflected."),
                None,
                None,
            )
            .unwrap();
        let observation_event = message(2, 145, "The magic has been deflected.");
        let event = feedback.into_event(EventObservation::new(
            42,
            ClientIdentity {
                pid: 42,
                process_creation_time: 100,
                dll_instance_id: [1; 16],
            },
            &observation_event,
        ));

        assert_eq!(event.name(), "spell.failed");
        let event = serde_json::to_value(event).unwrap();
        assert_eq!(event["data"]["slot"], 4);
        assert_eq!(event["data"]["name"], "Ao Puinsein");
        assert_eq!(event["data"]["arguments"]["type"], "target");
        assert_eq!(event["data"]["arguments"]["name"], "ZiLo");
        assert_eq!(event["data"]["reason"], "resisted");
        assert_eq!(event["data"]["submitted_tick_ms"], 100);
        assert_eq!(event["data"]["elapsed_ms"], 45);
    }

    #[test]
    fn unrelated_system_message_does_not_consume_a_cast() {
        let mut tracker = SpellFeedbackTracker::default();
        tracker.observe(None, &cast(1, 100, 1), Some("Mist"), None);
        assert!(
            tracker
                .observe(
                    None,
                    &message(2, 120, "You have gained experience."),
                    None,
                    None
                )
                .is_none()
        );
        assert_eq!(tracker.pending.len(), 1);
    }
}
