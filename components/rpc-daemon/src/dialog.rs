use crate::{state::ObservationMetadata, stream::EventObservation};
use darpc_model as model;
use serde::Serialize;
use utoipa::ToSchema;

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct DialogSnapshot {
    /// Metadata for the retained client snapshot.
    observation: ObservationMetadata,
    /// Current NPC dialog, or null when no observed dialog is open.
    dialog: Option<DialogState>,
}

impl DialogSnapshot {
    pub(crate) fn from_model(pid: u32, snapshot: &model::ClientSnapshot) -> Self {
        Self {
            observation: ObservationMetadata::from_model(pid, snapshot),
            dialog: snapshot.dialog.as_ref().map(DialogState::from),
        }
    }
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct DialogState {
    /// Wrapping nonzero revision required by dialog actions.
    revision: u32,
    /// Native client dialog family.
    kind: DialogKind,
    /// Server object that owns this conversation.
    target: DialogTarget,
    /// Name and graphic shown by the client.
    speaker: DialogSpeaker,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
    /// True while the client is waiting for the server's next page.
    response_pending: bool,
    /// Native navigation actions currently available.
    navigation: DialogNavigation,
    /// Current response type and its displayed rows or prompt.
    interaction: DialogInteraction,
}

#[derive(Clone, Copy, Debug, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DialogKind {
    Merchant,
    Pursuit,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct DialogTarget {
    id: u32,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct DialogSpeaker {
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    sprite: u16,
    sprite_type: DialogSpriteType,
    color: u8,
    show_graphic: bool,
}

#[derive(Clone, Copy, Debug, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DialogSpriteType {
    Creature,
    Item,
    Unknown,
}

#[derive(Clone, Copy, Debug, Serialize, ToSchema)]
pub(crate) struct DialogNavigation {
    previous: bool,
    next: bool,
    close: bool,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub(crate) enum DialogInteraction {
    Message,
    Choices(Vec<DialogChoice>),
    Input(DialogInput),
    Items(Vec<DialogItem>),
    Inventory(Vec<DialogSlot>),
    Spells(Vec<DialogSlot>),
    Skills(Vec<DialogSlot>),
    Protected,
    Unsupported,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct DialogChoice {
    index: u16,
    text: String,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct DialogInput {
    #[serde(skip_serializing_if = "Option::is_none")]
    prolog: Option<String>,
    maximum_bytes: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    epilog: Option<String>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct DialogItem {
    index: u16,
    sprite: u16,
    color: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    value: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    available_quantity: Option<u8>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct DialogSlot {
    index: u16,
    #[serde(skip_serializing_if = "is_zero")]
    slot: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    value: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sprite: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    color: Option<u8>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct DialogOpened {
    pub(crate) observation: EventObservation,
    dialog: DialogState,
}
#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct DialogChanged {
    pub(crate) observation: EventObservation,
    dialog: DialogState,
}
#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct DialogSubmitted {
    pub(crate) observation: EventObservation,
    previous_revision: u32,
    dialog: DialogState,
    submission: DialogSubmission,
}
#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct DialogClosed {
    pub(crate) observation: EventObservation,
    #[serde(skip_serializing_if = "Option::is_none")]
    previous: Option<DialogState>,
    reason: DialogCloseReason,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
#[serde(tag = "action", rename_all = "snake_case")]
pub(crate) enum DialogSubmission {
    Select { index: u16, quantity: u8 },
    Input { input: String },
    Previous,
    Next,
    Close,
}

#[derive(Clone, Copy, Debug, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DialogCloseReason {
    Client,
    Server,
    WorldChanged,
    Disconnected,
    Replaced,
}

impl DialogOpened {
    pub(crate) fn new(observation: EventObservation, dialog: model::DialogState) -> Self {
        Self {
            observation,
            dialog: DialogState::from(&dialog),
        }
    }
}
impl DialogChanged {
    pub(crate) fn new(observation: EventObservation, dialog: model::DialogState) -> Self {
        Self {
            observation,
            dialog: DialogState::from(&dialog),
        }
    }
}
impl DialogSubmitted {
    pub(crate) fn new(
        observation: EventObservation,
        previous_revision: u32,
        dialog: model::DialogState,
        submission: model::DialogSubmission,
    ) -> Self {
        Self {
            observation,
            previous_revision,
            dialog: DialogState::from(&dialog),
            submission: submission.into(),
        }
    }
}
impl DialogClosed {
    pub(crate) fn new(
        observation: EventObservation,
        previous: Option<model::DialogState>,
        reason: model::DialogCloseReason,
    ) -> Self {
        Self {
            observation,
            previous: previous.as_ref().map(DialogState::from),
            reason: reason.into(),
        }
    }
}

impl From<&model::DialogState> for DialogState {
    fn from(value: &model::DialogState) -> Self {
        Self {
            revision: value.revision,
            kind: match value.kind {
                model::DialogKind::Merchant => DialogKind::Merchant,
                model::DialogKind::Pursuit => DialogKind::Pursuit,
            },
            target: DialogTarget {
                id: value.target.id,
            },
            speaker: DialogSpeaker {
                name: value.speaker.name.clone(),
                sprite: value.speaker.sprite,
                sprite_type: match value.speaker.sprite_type {
                    model::DialogSpriteType::Creature => DialogSpriteType::Creature,
                    model::DialogSpriteType::Item => DialogSpriteType::Item,
                    model::DialogSpriteType::Unknown => DialogSpriteType::Unknown,
                },
                color: value.speaker.color,
                show_graphic: value.speaker.show_graphic,
            },
            content: value.content.clone(),
            response_pending: value.response_pending,
            navigation: DialogNavigation {
                previous: value.navigation.previous,
                next: value.navigation.next,
                close: value.navigation.close,
            },
            interaction: DialogInteraction::from(&value.interaction),
        }
    }
}

impl From<&model::DialogInteraction> for DialogInteraction {
    fn from(value: &model::DialogInteraction) -> Self {
        match value {
            model::DialogInteraction::Message => Self::Message,
            model::DialogInteraction::Choices(values) => Self::Choices(
                values
                    .iter()
                    .map(|v| DialogChoice {
                        index: v.index,
                        text: v.text.clone(),
                    })
                    .collect(),
            ),
            model::DialogInteraction::Input(v) => Self::Input(DialogInput {
                prolog: v.prolog.clone(),
                maximum_bytes: v.maximum_bytes,
                epilog: v.epilog.clone(),
            }),
            model::DialogInteraction::Items(values) => Self::Items(
                values
                    .iter()
                    .map(|v| DialogItem {
                        index: v.index,
                        sprite: v.sprite,
                        color: v.color,
                        name: v.name.clone(),
                        description: v.description.clone(),
                        value: v.value,
                        available_quantity: v.available_quantity,
                    })
                    .collect(),
            ),
            model::DialogInteraction::Inventory(values) => {
                Self::Inventory(values.iter().map(DialogSlot::from).collect())
            }
            model::DialogInteraction::Spells(values) => {
                Self::Spells(values.iter().map(DialogSlot::from).collect())
            }
            model::DialogInteraction::Skills(values) => {
                Self::Skills(values.iter().map(DialogSlot::from).collect())
            }
            model::DialogInteraction::Protected => Self::Protected,
            model::DialogInteraction::Unsupported => Self::Unsupported,
        }
    }
}

impl From<&model::DialogSlot> for DialogSlot {
    fn from(v: &model::DialogSlot) -> Self {
        Self {
            index: v.index,
            slot: v.slot,
            value: v.value,
            name: v.name.clone(),
            sprite: v.sprite,
            color: v.color,
        }
    }
}
impl From<model::DialogSubmission> for DialogSubmission {
    fn from(v: model::DialogSubmission) -> Self {
        match v {
            model::DialogSubmission::Select { index, quantity } => Self::Select { index, quantity },
            model::DialogSubmission::Input { input } => Self::Input { input },
            model::DialogSubmission::Previous => Self::Previous,
            model::DialogSubmission::Next => Self::Next,
            model::DialogSubmission::Close => Self::Close,
        }
    }
}
impl From<model::DialogCloseReason> for DialogCloseReason {
    fn from(v: model::DialogCloseReason) -> Self {
        match v {
            model::DialogCloseReason::Client => Self::Client,
            model::DialogCloseReason::Server => Self::Server,
            model::DialogCloseReason::WorldChanged => Self::WorldChanged,
            model::DialogCloseReason::Disconnected => Self::Disconnected,
            model::DialogCloseReason::Replaced => Self::Replaced,
        }
    }
}

const fn is_zero(value: &u8) -> bool {
    *value == 0
}
