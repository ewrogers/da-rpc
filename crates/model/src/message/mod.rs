#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MessageKind {
    Say,
    Shout,
    Chant,
    Whisper,
    Guild,
    Group,
    System,
    World,
}

/// World object category captured when local speech arrives.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MessageSenderType {
    Player,
    Monster,
    Mundane,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClientMessage {
    pub kind: MessageKind,
    pub sender: Option<String>,
    pub sender_id: Option<u32>,
    pub sender_type: Option<MessageSenderType>,
    pub recipient: Option<String>,
    pub text: String,
}
