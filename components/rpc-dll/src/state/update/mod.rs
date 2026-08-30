use super::*;

pub(super) type QueuedClientText<const N: usize> = crate::inline_bytes::InlineBytes<N>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct QueuedStateEvent {
    pub(super) sequence: u32,
    pub(super) revision: u32,
    pub(super) tick_ms: u32,
    pub(super) update: QueuedStateUpdate,
}

impl QueuedStateEvent {
    pub(crate) const fn sequence(&self) -> u32 {
        self.sequence
    }

    pub(crate) fn collection_batch(&self) -> Option<(CollectionKind, CollectionBatch)> {
        self.update.collection_batch()
    }

    pub(crate) fn into_model(self) -> Option<StateEvent> {
        let update = match self.update {
            #[cfg(not(test))]
            QueuedStateUpdate::Lifecycle(update) => StateUpdate::Lifecycle(update),
            QueuedStateUpdate::Audio(update) => StateUpdate::Audio(update),
            QueuedStateUpdate::Command(update) => StateUpdate::Command(update.into_model()?),
            QueuedStateUpdate::Status(update) => StateUpdate::Status(update),
            #[cfg(not(test))]
            QueuedStateUpdate::Movement(update) => StateUpdate::Movement(update),
            QueuedStateUpdate::Location(update) => StateUpdate::Location(update.into_model()),
            QueuedStateUpdate::MapDownload(update) => StateUpdate::MapDownload(update),
            QueuedStateUpdate::Effect(update) => StateUpdate::Effect(update),
            QueuedStateUpdate::Object(update) => StateUpdate::Object(update.into_model()),
            QueuedStateUpdate::Message(update) => StateUpdate::Message(update.into_model()),
            QueuedStateUpdate::Collection(update) => update.into_model(self.tick_ms),
            QueuedStateUpdate::Ability(update) => StateUpdate::Ability(update.into_model()),
            QueuedStateUpdate::Action(update) => StateUpdate::Action(update),
            QueuedStateUpdate::Entity(update) => StateUpdate::Entity(update.into_model()),
            QueuedStateUpdate::Dialog(update) => StateUpdate::Dialog(crate::dialog::take(update)?),
            QueuedStateUpdate::FieldMap(update) => {
                StateUpdate::FieldMap(crate::field_map::take(update)?)
            }
            QueuedStateUpdate::Bulletin(update) => {
                StateUpdate::Bulletin(crate::bulletin::take(update)?)
            }
            QueuedStateUpdate::MessageDialogs(update) => {
                StateUpdate::MessageDialogs(crate::message_dialog::take(update)?)
            }
            QueuedStateUpdate::Group(update) => StateUpdate::Group(crate::group::take(update)?),
            QueuedStateUpdate::Exchange(update) => {
                StateUpdate::Exchange(crate::exchange::take(update)?)
            }
            QueuedStateUpdate::Legend(update) => StateUpdate::Legend(crate::legend::take(update)?),
            QueuedStateUpdate::Player(update) => StateUpdate::Player(crate::player::take(update)?),
            QueuedStateUpdate::CharacterProfile(update) => {
                StateUpdate::CharacterProfile(crate::player::take_identity(update)?)
            }
            QueuedStateUpdate::PlannedRoute(update) => {
                StateUpdate::PlannedRoute(crate::route::take(update)?)
            }
            QueuedStateUpdate::Look(update) => StateUpdate::Look(update.into_model()),
        };
        Some(StateEvent {
            sequence: self.sequence,
            revision: self.revision,
            tick_ms: self.tick_ms,
            update,
        })
    }

    pub(crate) fn discard(self) {
        if let QueuedStateUpdate::Dialog(update) = self.update {
            crate::dialog::release(update);
        }
        if let QueuedStateUpdate::FieldMap(update) = self.update {
            crate::field_map::release(update);
        }
        if let QueuedStateUpdate::Bulletin(update) = self.update {
            crate::bulletin::release(update);
        }
        if let QueuedStateUpdate::MessageDialogs(update) = self.update {
            crate::message_dialog::release(update);
        }
        if let QueuedStateUpdate::Group(update) = self.update {
            crate::group::release(update);
        }
        if let QueuedStateUpdate::Exchange(update) = self.update {
            crate::exchange::release(update);
        }
        if let QueuedStateUpdate::Legend(update) = self.update {
            crate::legend::release(update);
        }
        if let QueuedStateUpdate::Player(update) = self.update {
            crate::player::release(update);
        }
        if let QueuedStateUpdate::CharacterProfile(update) = self.update {
            crate::player::release_identity(update);
        }
        if let QueuedStateUpdate::PlannedRoute(update) = self.update {
            crate::route::release(update);
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
// Queue entries are fixed, pointer-free copies. Boxing the collection or map
// configuration variants would allocate in the game-thread observer.
#[allow(clippy::large_enum_variant)]
pub(super) enum QueuedStateUpdate {
    #[cfg(not(test))]
    Lifecycle(LifecycleUpdate),
    Audio(AudioUpdate),
    Command(QueuedCommand),
    Status(StatusUpdate),
    #[cfg(not(test))]
    Movement(MovementUpdate),
    Location(QueuedLocationUpdate),
    MapDownload(MapDownloadUpdate),
    Effect(EffectUpdate),
    Object(QueuedObjectUpdate),
    Message(QueuedMessage),
    Collection(QueuedCollectionUpdate),
    Ability(QueuedAbilityUpdate),
    Action(ActionUpdate),
    Entity(QueuedEntityUpdate),
    Dialog(crate::dialog::QueuedDialog),
    FieldMap(crate::field_map::QueuedFieldMap),
    Bulletin(crate::bulletin::QueuedBulletin),
    MessageDialogs(crate::message_dialog::QueuedMessageDialogs),
    Group(crate::group::QueuedGroup),
    Exchange(crate::exchange::QueuedExchange),
    Legend(crate::legend::QueuedLegend),
    Player(crate::player::QueuedPlayer),
    CharacterProfile(crate::player::QueuedCharacterProfile),
    PlannedRoute(crate::route::QueuedRoute),
    Look(QueuedLookResult),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct QueuedLookResult {
    command_id: u32,
    target: LookResultTarget,
    text: QueuedClientText<MAX_LOOK_RESULT_TEXT_LEN>,
}

impl QueuedLookResult {
    pub(super) fn new(command_id: u32, target: LookResultTarget, text: &[u8]) -> Option<Self> {
        Some(Self {
            command_id: (command_id != 0).then_some(command_id)?,
            target,
            text: QueuedClientText::try_nonempty(text)?,
        })
    }

    fn into_model(self) -> LookResult {
        LookResult {
            command_id: self.command_id,
            target: self.target,
            text: decode_client_text(self.text.as_bytes()).unwrap_or_default(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct QueuedCommand {
    text: QueuedClientText<MAX_EVENT_COMMAND_BYTES>,
}

impl QueuedCommand {
    pub(super) fn new(text: &[u8]) -> Option<Self> {
        Some(Self {
            text: QueuedClientText::try_nonempty(text)?,
        })
    }

    fn into_model(self) -> Option<ClientCommand> {
        ClientCommand::parse(&decode_client_text(self.text.as_bytes())?)
    }
}

impl QueuedStateUpdate {
    pub(super) fn collection_batch(self) -> Option<(CollectionKind, CollectionBatch)> {
        match self {
            Self::Collection(update) => Some((update.kind(), update.batch())),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum QueuedEntityUpdate {
    Animated {
        entity: RawWorldObject,
        animation: u8,
        duration_10ms: u16,
    },
    Effect {
        entity: RawWorldObject,
        effect: u16,
        source: Option<RawWorldObject>,
        frame_interval_ms: Option<i16>,
    },
    Damaged {
        entity: RawWorldObject,
        health_percent: u8,
    },
}

impl QueuedEntityUpdate {
    fn into_model(self) -> EntityUpdate {
        match self {
            Self::Animated {
                entity,
                animation,
                duration_10ms,
            } => EntityUpdate::Animated {
                entity: crate::objects::object_model(entity),
                animation,
                duration_10ms,
            },
            Self::Effect {
                entity,
                effect,
                source,
                frame_interval_ms,
            } => EntityUpdate::Effect {
                entity: crate::objects::object_model(entity),
                effect,
                source: source.map(crate::objects::object_model),
                frame_interval_ms,
            },
            Self::Damaged {
                entity,
                health_percent,
            } => EntityUpdate::Damaged {
                entity: crate::objects::object_model(entity),
                health_percent,
            },
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]

pub(super) struct QueuedMessage {
    pub(super) kind: MessageKind,
    pub(super) sender: Option<QueuedClientText<MAX_EVENT_MESSAGE_NAME_BYTES>>,
    pub(super) recipient: Option<QueuedClientText<MAX_EVENT_MESSAGE_NAME_BYTES>>,
    pub(super) text: QueuedClientText<MAX_EVENT_MESSAGE_TEXT_BYTES>,
}

impl QueuedMessage {
    pub(super) fn new(
        kind: MessageKind,
        sender: Option<QueuedClientText<MAX_EVENT_MESSAGE_NAME_BYTES>>,
        recipient: Option<QueuedClientText<MAX_EVENT_MESSAGE_NAME_BYTES>>,
        text: &[u8],
    ) -> Option<Self> {
        Some(Self {
            kind,
            sender,
            recipient,
            text: QueuedClientText::try_nonempty(text)?,
        })
    }

    pub(super) fn into_model(self) -> ClientMessage {
        ClientMessage {
            kind: self.kind,
            sender: self
                .sender
                .and_then(|text| decode_client_text(text.as_bytes())),
            recipient: self
                .recipient
                .and_then(|text| decode_client_text(text.as_bytes())),
            text: decode_client_text(self.text.as_bytes()).unwrap_or_default(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct QueuedLocationUpdate {
    pub(super) x: i32,
    pub(super) y: i32,
    pub(super) map: Option<QueuedMapChange>,
}

impl QueuedLocationUpdate {
    pub(super) fn into_model(self) -> LocationUpdate {
        LocationUpdate {
            x: self.x,
            y: self.y,
            map: self.map.map(QueuedMapChange::into_model),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct QueuedMapChange {
    pub(super) id: u32,
    pub(super) width: i32,
    pub(super) height: i32,
    pub(super) name_length: u8,
    pub(super) name: [u8; MAX_EVENT_MAP_NAME_BYTES],
}
