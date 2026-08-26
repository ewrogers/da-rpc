use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct CastingState {
    pub(super) active: bool,
    pub(super) slot: Option<u8>,
    pub(super) total_lines: u8,
    pub(super) current_line: u8,
}

#[cfg(all(windows, not(test)))]
pub(super) fn casting_state() -> Option<CastingState> {
    crate::actions::spell::casting_state().map(|state| CastingState {
        active: state.active,
        slot: state.slot,
        total_lines: state.total_lines,
        current_line: state.current_line,
    })
}

#[cfg(any(not(windows), test))]
pub(super) const fn casting_state() -> Option<CastingState> {
    None
}

#[cfg(all(windows, not(test)))]
fn spell_argument_type(slot: u8) -> Option<u8> {
    crate::actions::spell::argument_type(slot)
}

#[cfg(any(not(windows), test))]
const fn spell_argument_type(_slot: u8) -> Option<u8> {
    None
}

pub(crate) fn observe_outgoing(body: &[u8], tick_ms: u32) {
    let Some((&opcode, fields)) = body.split_first() else {
        return;
    };
    match opcode {
        0x3E => {
            let Some(&slot) = fields.first().filter(|slot| **slot != 0 && **slot <= 90) else {
                return;
            };
            watch_ability_cooldown(CollectionKind::Skillbook, slot, tick_ms);
            push_event(
                QueuedStateUpdate::Ability(QueuedAbilityUpdate::SkillUsed { slot }),
                tick_ms,
            );
        }
        0x4D => {
            let Some(casting) = casting_state()
                .filter(|state| state.active && state.total_lines != 0 && state.slot.is_some())
            else {
                return;
            };
            let slot = casting.slot.expect("filtered casting slot is present");
            // SAFETY: outbound packet observation runs synchronously on the
            // client main thread.
            if let Some(cancelled_slot) = unsafe { CACHE.spell_begin(slot, casting.total_lines) } {
                push_event(
                    QueuedStateUpdate::Ability(QueuedAbilityUpdate::SpellCancelled {
                        slot: cancelled_slot,
                        source: SpellCancellationSource::Replaced,
                    }),
                    tick_ms,
                );
            }
            push_event(
                QueuedStateUpdate::Ability(QueuedAbilityUpdate::SpellBegin {
                    slot,
                    total_lines: casting.total_lines,
                }),
                tick_ms,
            );
        }
        0x4E => {
            let Some(casting) = casting_state().filter(|state| {
                state.active
                    && state.total_lines != 0
                    && state.current_line < state.total_lines
                    && state.slot.is_some()
            }) else {
                return;
            };
            let slot = casting.slot.expect("filtered casting slot is present");
            let line = casting.current_line.saturating_add(1);
            push_event(
                QueuedStateUpdate::Ability(QueuedAbilityUpdate::SpellChant {
                    slot,
                    line,
                    total_lines: casting.total_lines,
                }),
                tick_ms,
            );
        }
        0x0F => {
            let Some(&slot) = fields.first().filter(|slot| **slot != 0 && **slot <= 90) else {
                return;
            };
            let arguments = parse_spell_arguments(slot, body);
            // SAFETY: outbound packet observation runs synchronously on the
            // client main thread.
            if let Some(cancelled_slot) = unsafe { CACHE.spell_cast(slot) } {
                push_event(
                    QueuedStateUpdate::Ability(QueuedAbilityUpdate::SpellCancelled {
                        slot: cancelled_slot,
                        source: SpellCancellationSource::Replaced,
                    }),
                    tick_ms,
                );
            }
            watch_ability_cooldown(CollectionKind::Spellbook, slot, tick_ms);
            push_event(
                QueuedStateUpdate::Ability(QueuedAbilityUpdate::SpellCast { slot, arguments }),
                tick_ms,
            );
        }
        _ => {}
    }
}

pub(crate) fn observe_spell_cancelled(tick_ms: u32) {
    let native = casting_state();
    // SAFETY: decoded server events run on the client main thread after the
    // native handler has cleared the casting controller.
    let slot = unsafe { CACHE.spell_cancelled(native.and_then(|state| state.slot)) };
    let Some(slot) = slot else {
        return;
    };
    push_event(
        QueuedStateUpdate::Ability(QueuedAbilityUpdate::SpellCancelled {
            slot,
            source: SpellCancellationSource::Server,
        }),
        tick_ms,
    );
}

fn parse_spell_arguments(slot: u8, body: &[u8]) -> QueuedSpellArguments {
    match spell_argument_type(slot) {
        Some(1) => QueuedClientText::try_nonempty(body.get(2..).unwrap_or_default())
            .map_or(QueuedSpellArguments::Unknown, QueuedSpellArguments::Input),
        Some(2) if body.len() == 10 => QueuedSpellArguments::Target {
            id: nonzero(u32::from_be_bytes(
                body[2..6].try_into().expect("target ID field"),
            )),
            x: i32::from(u16::from_be_bytes(
                body[6..8].try_into().expect("target X field"),
            )),
            y: i32::from(u16::from_be_bytes(
                body[8..10].try_into().expect("target Y field"),
            )),
        },
        Some(3 | 4 | 6 | 7) => parse_spell_values(body),
        Some(5) if body.len() == 2 => QueuedSpellArguments::None,
        _ => QueuedSpellArguments::Unknown,
    }
}

fn parse_spell_values(body: &[u8]) -> QueuedSpellArguments {
    let bytes = body.get(2..).unwrap_or_default();
    if bytes.is_empty() || bytes.len() > 8 || bytes.len() % 2 != 0 {
        return QueuedSpellArguments::Unknown;
    }
    let mut values = [0; 4];
    for (destination, source) in values.iter_mut().zip(bytes.as_chunks::<2>().0) {
        *destination = u16::from_be_bytes([source[0], source[1]]);
    }
    QueuedSpellArguments::Values {
        count: (bytes.len() / 2) as u8,
        values,
    }
}

const fn nonzero(value: u32) -> Option<u32> {
    if value == 0 { None } else { Some(value) }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum QueuedAbilityUpdate {
    SkillUsed {
        slot: u8,
    },
    SpellBegin {
        slot: u8,
        total_lines: u8,
    },
    SpellChant {
        slot: u8,
        line: u8,
        total_lines: u8,
    },
    SpellCast {
        slot: u8,
        arguments: QueuedSpellArguments,
    },
    SpellCancelled {
        slot: u8,
        source: SpellCancellationSource,
    },
}

impl QueuedAbilityUpdate {
    pub(super) fn into_model(self) -> AbilityUpdate {
        match self {
            Self::SkillUsed { slot } => AbilityUpdate::SkillUsed { slot },
            Self::SpellBegin { slot, total_lines } => {
                AbilityUpdate::SpellBegin { slot, total_lines }
            }
            Self::SpellChant {
                slot,
                line,
                total_lines,
            } => AbilityUpdate::SpellChant {
                slot,
                line,
                total_lines,
            },
            Self::SpellCast { slot, arguments } => AbilityUpdate::SpellCast {
                slot,
                arguments: arguments.into_model(),
            },
            Self::SpellCancelled { slot, source } => AbilityUpdate::SpellCancelled { slot, source },
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum QueuedSpellArguments {
    Unknown,
    None,
    Target { id: Option<u32>, x: i32, y: i32 },
    Input(QueuedClientText<MAX_SPELL_INPUT_BYTES>),
    Values { count: u8, values: [u16; 4] },
}

impl QueuedSpellArguments {
    pub(super) fn into_model(self) -> SpellCastArguments {
        match self {
            Self::Unknown => SpellCastArguments::Unknown,
            Self::None => SpellCastArguments::None,
            Self::Target { id, x, y } => SpellCastArguments::Target { id, x, y },
            Self::Input(input) => decode_client_text(input.as_bytes())
                .map(SpellCastArguments::Input)
                .unwrap_or(SpellCastArguments::Unknown),
            Self::Values { count, values } => {
                SpellCastArguments::Values(values[..usize::from(count)].to_vec())
            }
        }
    }
}
