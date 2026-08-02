use crate::{
    registry::{ClientIdentity, ConnectionEvent, RegistrySnapshot},
    stream::EventObservation,
};
use darpc_model::{ClientMessage, MessageKind, StateUpdate};
use serde::Serialize;
use std::collections::{BTreeMap, VecDeque};
use utoipa::ToSchema;

pub(crate) const MAX_MESSAGES_PER_CLIENT: usize = 4_096;
pub(crate) const MAX_MESSAGE_BYTES_PER_CLIENT: usize = 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum MessageType {
    Say,
    Shout,
    Whisper,
    Guild,
    Group,
    System,
    World,
}

impl MessageType {
    pub(crate) const fn event_name(self) -> &'static str {
        match self {
            Self::Say => "message.say",
            Self::Shout => "message.shout",
            Self::Whisper => "message.whisper",
            Self::Guild => "message.guild",
            Self::Group => "message.group",
            Self::System => "message.system",
            Self::World => "message.world",
        }
    }
}

impl From<MessageKind> for MessageType {
    fn from(kind: MessageKind) -> Self {
        match kind {
            MessageKind::Say => Self::Say,
            MessageKind::Shout => Self::Shout,
            MessageKind::Whisper => Self::Whisper,
            MessageKind::Guild => Self::Guild,
            MessageKind::Group => Self::Group,
            MessageKind::System => Self::System,
            MessageKind::World => Self::World,
        }
    }
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct Message {
    pub(crate) observation: EventObservation,
    #[serde(rename = "type")]
    pub(crate) message_type: MessageType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) sender: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) recipient: Option<String>,
    pub(crate) text: String,
}

impl Message {
    pub(crate) fn new(observation: EventObservation, message: ClientMessage) -> Self {
        Self {
            observation,
            message_type: message.kind.into(),
            sender: message.sender,
            recipient: message.recipient,
            text: message.text,
        }
    }

    pub(crate) const fn event_name(&self) -> &'static str {
        self.message_type.event_name()
    }

    pub(crate) const fn sequence(&self) -> u32 {
        self.observation.sequence()
    }

    fn byte_size(&self) -> usize {
        self.text.len()
            + self.sender.as_ref().map_or(0, String::len)
            + self.recipient.as_ref().map_or(0, String::len)
    }
}

#[derive(Clone, Debug, Default, Serialize, ToSchema)]
pub(crate) struct Messages {
    pub(crate) messages: Vec<Message>,
}

#[derive(Default)]
struct MessageHistory {
    messages: VecDeque<Message>,
    bytes: usize,
}

impl MessageHistory {
    fn push(&mut self, message: Message) {
        self.bytes = self.bytes.saturating_add(message.byte_size());
        self.messages.push_back(message);
        while self.messages.len() > MAX_MESSAGES_PER_CLIENT
            || self.bytes > MAX_MESSAGE_BYTES_PER_CLIENT
        {
            let Some(removed) = self.messages.pop_front() else {
                break;
            };
            self.bytes = self.bytes.saturating_sub(removed.byte_size());
        }
    }
}

#[derive(Default)]
pub(crate) struct MessageStore {
    clients: BTreeMap<ClientIdentity, MessageHistory>,
}

impl MessageStore {
    pub(crate) fn observe(&mut self, event: &ConnectionEvent) {
        match event {
            ConnectionEvent::Connected { pid, hello, .. } => {
                let identity = ClientIdentity::from_hello(*hello);
                self.clients
                    .retain(|existing, _| existing.pid != *pid || *existing == identity);
            }
            ConnectionEvent::StateEvents {
                pid,
                identity,
                events,
            } => {
                for event in events {
                    let StateUpdate::Message(message) = &event.update else {
                        continue;
                    };
                    self.clients
                        .entry(*identity)
                        .or_default()
                        .push(Message::new(
                            EventObservation::new(*pid, *identity, event),
                            message.clone(),
                        ));
                }
            }
            _ => {}
        }
    }

    pub(crate) fn retain(&mut self, registry: &RegistrySnapshot) {
        self.clients.retain(|identity, _| {
            registry
                .clients
                .iter()
                .any(|client| client.identity == Some(*identity))
        });
    }

    pub(crate) fn get(&self, identity: ClientIdentity) -> Messages {
        Messages {
            messages: self
                .clients
                .get(&identity)
                .map_or_else(Vec::new, |history| {
                    history.messages.iter().cloned().collect()
                }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use darpc_model::StateEvent;
    use darpc_protocol::{Architecture, ComponentVersion, Hello, SUPPORTED_VERSIONS};

    fn hello(instance: u8) -> Hello {
        Hello {
            protocol_versions: SUPPORTED_VERSIONS,
            process_id: 42,
            process_creation_time: 100,
            dll_instance_id: [instance; 16],
            architecture: Architecture::X86,
            dll_version: ComponentVersion {
                major: 0,
                minor: 1,
                patch: 0,
            },
            executable_fingerprint: [0; 32],
            client_version: 741,
        }
    }

    fn event(sequence: u32, text: &str) -> StateEvent {
        StateEvent {
            sequence,
            revision: sequence,
            tick_ms: sequence * 10,
            update: StateUpdate::Message(ClientMessage {
                kind: MessageKind::Say,
                sender: Some("Aisling".into()),
                recipient: None,
                text: text.into(),
            }),
        }
    }

    #[test]
    fn retains_messages_for_the_current_dll_identity() {
        let hello = hello(1);
        let identity = ClientIdentity::from_hello(hello);
        let mut store = MessageStore::default();
        store.observe(&ConnectionEvent::Connected {
            pid: 42,
            hello,
            selected_version: SUPPORTED_VERSIONS.max,
        });
        store.observe(&ConnectionEvent::StateEvents {
            pid: 42,
            identity,
            events: vec![event(1, "hello")],
        });

        let messages = store.get(identity);
        assert_eq!(messages.messages.len(), 1);
        assert_eq!(messages.messages[0].text, "hello");
        assert_eq!(messages.messages[0].message_type, MessageType::Say);
    }

    #[test]
    fn a_reloaded_dll_discards_the_previous_identity_history() {
        let first = hello(1);
        let first_identity = ClientIdentity::from_hello(first);
        let mut store = MessageStore::default();
        store.observe(&ConnectionEvent::StateEvents {
            pid: 42,
            identity: first_identity,
            events: vec![event(1, "old")],
        });
        store.observe(&ConnectionEvent::Connected {
            pid: 42,
            hello: hello(2),
            selected_version: SUPPORTED_VERSIONS.max,
        });

        assert!(store.get(first_identity).messages.is_empty());
    }

    #[test]
    fn history_evicts_the_oldest_messages_at_the_count_limit() {
        let identity = ClientIdentity::from_hello(hello(1));
        let mut store = MessageStore::default();
        for sequence in 1..=u32::try_from(MAX_MESSAGES_PER_CLIENT + 1).unwrap() {
            store.observe(&ConnectionEvent::StateEvents {
                pid: 42,
                identity,
                events: vec![event(sequence, "x")],
            });
        }

        let messages = store.get(identity);
        assert_eq!(messages.messages.len(), MAX_MESSAGES_PER_CLIENT);
        assert_eq!(messages.messages[0].observation.sequence(), 2);
    }

    #[test]
    fn history_evicts_the_oldest_messages_at_the_byte_limit() {
        let identity = ClientIdentity::from_hello(hello(1));
        let mut store = MessageStore::default();
        let text = "x".repeat(4 * 1024);
        for sequence in 1..=257 {
            store.observe(&ConnectionEvent::StateEvents {
                pid: 42,
                identity,
                events: vec![event(sequence, &text)],
            });
        }

        let messages = store.get(identity);
        assert_eq!(messages.messages.len(), 255);
        assert_eq!(messages.messages[0].observation.sequence(), 3);
    }
}
