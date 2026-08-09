use super::*;

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
            QueuedStateUpdate::Status(update) => StateUpdate::Status(update),
            #[cfg(not(test))]
            QueuedStateUpdate::Movement(update) => StateUpdate::Movement(update),
            QueuedStateUpdate::Location(update) => StateUpdate::Location(update.into_model()),
            QueuedStateUpdate::Effect(update) => StateUpdate::Effect(update),
            QueuedStateUpdate::Object(update) => StateUpdate::Object(update.into_model()),
            QueuedStateUpdate::Message(update) => StateUpdate::Message(update.into_model()),
            QueuedStateUpdate::Collection(update) => update.into_model(self.tick_ms),
            QueuedStateUpdate::Ability(update) => StateUpdate::Ability(update.into_model()),
            QueuedStateUpdate::Action(update) => StateUpdate::Action(update),
            QueuedStateUpdate::Entity(update) => StateUpdate::Entity(update.into_model()),
            QueuedStateUpdate::Dialog(update) => StateUpdate::Dialog(crate::dialog::take(update)?),
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
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
// Queue entries are fixed, pointer-free copies. Boxing the collection variant
// would allocate in the game-thread observer.
#[allow(clippy::large_enum_variant)]
pub(super) enum QueuedStateUpdate {
    Status(StatusUpdate),
    #[cfg(not(test))]
    Movement(MovementUpdate),
    Location(QueuedLocationUpdate),
    Effect(EffectUpdate),
    Object(QueuedObjectUpdate),
    Message(QueuedMessage),
    Collection(QueuedCollectionUpdate),
    Ability(QueuedAbilityUpdate),
    Action(ActionUpdate),
    Entity(QueuedEntityUpdate),
    Dialog(crate::dialog::QueuedDialog),
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
            text: QueuedClientText::new(text)?,
        })
    }

    pub(super) fn into_model(self) -> ClientMessage {
        ClientMessage {
            kind: self.kind,
            sender: self.sender.and_then(QueuedClientText::decode),
            recipient: self.recipient.and_then(QueuedClientText::decode),
            text: self.text.decode().unwrap_or_default(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct QueuedClientText<const N: usize> {
    pub(super) length: u16,
    pub(super) bytes: [u8; N],
}

impl<const N: usize> QueuedClientText<N> {
    pub(super) fn new(text: &[u8]) -> Option<Self> {
        if text.is_empty() || text.len() > N {
            return None;
        }
        let mut bytes = [0; N];
        bytes[..text.len()].copy_from_slice(text);
        Some(Self {
            length: u16::try_from(text.len()).expect("queued client text length fits u16"),
            bytes,
        })
    }

    pub(super) fn decode(self) -> Option<String> {
        decode_client_text(&self.bytes[..usize::from(self.length)])
    }
}

#[cfg(windows)]
fn decode_client_text(bytes: &[u8]) -> Option<String> {
    crate::client_text::decode(bytes)
}

#[cfg(not(windows))]
fn decode_client_text(bytes: &[u8]) -> Option<String> {
    (!bytes.is_empty()).then(|| String::from_utf8_lossy(bytes).into_owned())
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
