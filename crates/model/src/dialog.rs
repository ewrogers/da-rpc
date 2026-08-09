#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DialogState {
    pub revision: u32,
    pub kind: DialogKind,
    pub target: DialogTarget,
    pub speaker: DialogSpeaker,
    pub content: Option<String>,
    pub response_pending: bool,
    pub navigation: DialogNavigation,
    pub interaction: DialogInteraction,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DialogKind {
    Merchant,
    Pursuit,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DialogTarget {
    pub id: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DialogSpeaker {
    pub name: Option<String>,
    pub sprite: u16,
    pub sprite_type: DialogSpriteType,
    pub color: u8,
    pub show_graphic: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DialogSpriteType {
    Creature,
    Item,
    Unknown,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DialogNavigation {
    pub previous: bool,
    pub next: bool,
    pub close: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DialogInteraction {
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DialogChoice {
    pub index: u16,
    pub text: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DialogInput {
    pub prolog: Option<String>,
    pub maximum_bytes: u8,
    pub epilog: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DialogItem {
    pub index: u16,
    pub sprite: u16,
    pub color: u8,
    pub name: Option<String>,
    pub description: Option<String>,
    pub value: Option<u32>,
    pub available_quantity: Option<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DialogSlot {
    pub index: u16,
    pub slot: u8,
    pub value: Option<u32>,
    pub name: Option<String>,
    pub sprite: Option<u16>,
    pub color: Option<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DialogUpdate {
    Opened(DialogState),
    Changed(DialogState),
    Submitted {
        state: DialogState,
        previous_revision: u32,
        submission: DialogSubmission,
    },
    Closed {
        previous: Option<DialogState>,
        reason: DialogCloseReason,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DialogSubmission {
    Select { index: u16, quantity: u8 },
    Input { input: String },
    Previous,
    Next,
    Close,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DialogCloseReason {
    Client,
    Server,
    WorldChanged,
    Disconnected,
    Replaced,
}
