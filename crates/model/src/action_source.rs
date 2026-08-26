/// The observed origin of a character action.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ActionSource {
    /// The action was already active when observation began or its origin is unavailable.
    Unknown,
    /// The action originated inside the game client, outside a daRPC command.
    Client,
    /// The action originated while daRPC was executing the identified command.
    Command { command_id: u32 },
}
