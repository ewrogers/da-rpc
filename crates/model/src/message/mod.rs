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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClientMessage {
    pub kind: MessageKind,
    pub sender: Option<String>,
    pub recipient: Option<String>,
    pub text: String,
}
