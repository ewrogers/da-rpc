use crate::registry::{ClientIdentity, ConnectionEvent, RegistrySnapshot};
use chrono::{DateTime, Local, SecondsFormat, Utc};
use darpc_model::{ClientMessage, MessageKind, StateUpdate};
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    str::FromStr,
};
use utoipa::ToSchema;

pub(crate) const MAX_MESSAGES_PER_CLIENT: usize = 4_096;
pub(crate) const MAX_MESSAGE_BYTES_PER_CLIENT: usize = 1024 * 1024;
pub(crate) const DEFAULT_MESSAGE_COUNT: usize = 20;
pub(crate) const MAX_MESSAGE_COUNT: usize = 100;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum MessageChannel {
    Say,
    Shout,
    Whisper,
    Guild,
    Group,
    System,
    World,
}

impl MessageChannel {
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

impl From<MessageKind> for MessageChannel {
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

impl FromStr for MessageChannel {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "say" => Ok(Self::Say),
            "shout" => Ok(Self::Shout),
            "whisper" => Ok(Self::Whisper),
            "guild" => Ok(Self::Guild),
            "group" => Ok(Self::Group),
            "system" => Ok(Self::System),
            "world" => Ok(Self::World),
            _ => Err(()),
        }
    }
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct Message {
    #[serde(skip)]
    #[schema(ignore)]
    event_sequence: u32,
    /// Daemon observation time in ISO 8601 using the daemon's local UTC offset.
    #[schema(value_type = String, format = DateTime)]
    pub(crate) timestamp: String,
    /// Wrapping Windows millisecond tick recorded by the game client.
    pub(crate) tick_ms: u32,
    pub(crate) channel: MessageChannel,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) sender: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) recipient: Option<String>,
    pub(crate) text: String,
}

impl Message {
    pub(crate) fn new(
        event_sequence: u32,
        tick_ms: u32,
        observed_at_utc: DateTime<Utc>,
        message: ClientMessage,
    ) -> Self {
        Self {
            event_sequence,
            timestamp: local_timestamp(observed_at_utc),
            tick_ms,
            channel: message.kind.into(),
            sender: message.sender,
            recipient: message.recipient,
            text: message.text,
        }
    }

    pub(crate) const fn event_name(&self) -> &'static str {
        self.channel.event_name()
    }

    pub(crate) const fn sequence(&self) -> u32 {
        self.event_sequence
    }
}

fn local_timestamp(observed_at_utc: DateTime<Utc>) -> String {
    observed_at_utc
        .with_timezone(&Local)
        .to_rfc3339_opts(SecondsFormat::AutoSi, false)
}

#[derive(Clone, Debug, Default, Serialize, ToSchema)]
pub(crate) struct Messages {
    pub(crate) messages: Vec<Message>,
}

#[derive(Default)]
struct MessageHistory {
    messages: VecDeque<StoredMessage>,
    bytes: usize,
}

#[derive(Clone, Debug)]
struct StoredMessage {
    event_sequence: u32,
    tick_ms: u32,
    observed_at_utc: DateTime<Utc>,
    channel: MessageChannel,
    sender: Option<String>,
    recipient: Option<String>,
    text: String,
}

impl StoredMessage {
    fn new(
        event: &darpc_model::StateEvent,
        observed_at_utc: DateTime<Utc>,
        message: &ClientMessage,
    ) -> Self {
        Self {
            event_sequence: event.sequence,
            tick_ms: event.tick_ms,
            observed_at_utc,
            channel: message.kind.into(),
            sender: message.sender.clone(),
            recipient: message.recipient.clone(),
            text: message.text.clone(),
        }
    }

    fn to_api(&self) -> Message {
        Message {
            event_sequence: self.event_sequence,
            timestamp: local_timestamp(self.observed_at_utc),
            tick_ms: self.tick_ms,
            channel: self.channel,
            sender: self.sender.clone(),
            recipient: self.recipient.clone(),
            text: self.text.clone(),
        }
    }

    fn byte_size(&self) -> usize {
        self.text.len()
            + self.sender.as_ref().map_or(0, String::len)
            + self.recipient.as_ref().map_or(0, String::len)
    }
}

pub(crate) struct MessageFilter {
    pub(crate) channels: Option<BTreeSet<MessageChannel>>,
    pub(crate) since: Option<DateTime<Utc>>,
    pub(crate) skip: usize,
    pub(crate) count: usize,
}

impl Default for MessageFilter {
    fn default() -> Self {
        Self {
            channels: None,
            since: None,
            skip: 0,
            count: DEFAULT_MESSAGE_COUNT,
        }
    }
}

impl MessageHistory {
    fn push(&mut self, message: StoredMessage) {
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
    pub(crate) fn observe(&mut self, event: &ConnectionEvent, observed_at_utc: DateTime<Utc>) {
        match event {
            ConnectionEvent::Connected { pid, hello, .. } => {
                let identity = ClientIdentity::from_hello(*hello);
                self.clients
                    .retain(|existing, _| existing.pid != *pid || *existing == identity);
            }
            ConnectionEvent::StateEvents {
                identity, events, ..
            } => {
                for event in events {
                    let StateUpdate::Message(message) = &event.update else {
                        continue;
                    };
                    self.clients
                        .entry(*identity)
                        .or_default()
                        .push(StoredMessage::new(event, observed_at_utc, message));
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

    pub(crate) fn get(&self, identity: ClientIdentity, filter: &MessageFilter) -> Messages {
        Messages {
            messages: self
                .clients
                .get(&identity)
                .map_or_else(Vec::new, |history| {
                    let mut messages = history
                        .messages
                        .iter()
                        .rev()
                        .filter(|message| {
                            filter
                                .channels
                                .as_ref()
                                .is_none_or(|channels| channels.contains(&message.channel))
                                && filter
                                    .since
                                    .is_none_or(|since| message.observed_at_utc > since)
                        })
                        .collect::<Vec<_>>();
                    messages.sort_by_key(|message| std::cmp::Reverse(message.observed_at_utc));
                    messages
                        .into_iter()
                        .skip(filter.skip)
                        .take(filter.count)
                        .map(StoredMessage::to_api)
                        .collect()
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

    fn observed_at(seconds: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(seconds, 0).unwrap()
    }

    fn all_messages() -> MessageFilter {
        MessageFilter {
            count: MAX_MESSAGES_PER_CLIENT,
            ..MessageFilter::default()
        }
    }

    fn event(sequence: u32, text: &str) -> StateEvent {
        event_on(sequence, MessageKind::Say, text)
    }

    fn event_on(sequence: u32, kind: MessageKind, text: &str) -> StateEvent {
        StateEvent {
            sequence,
            revision: sequence,
            tick_ms: sequence * 10,
            update: StateUpdate::Message(ClientMessage {
                kind,
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
        store.observe(
            &ConnectionEvent::Connected {
                pid: 42,
                hello,
                selected_version: SUPPORTED_VERSIONS.max,
            },
            observed_at(1),
        );
        store.observe(
            &ConnectionEvent::StateEvents {
                pid: 42,
                identity,
                events: vec![event(1, "hello")],
            },
            observed_at(2),
        );

        let messages = store.get(identity, &MessageFilter::default());
        assert_eq!(messages.messages.len(), 1);
        assert_eq!(messages.messages[0].text, "hello");
        assert_eq!(messages.messages[0].channel, MessageChannel::Say);
        assert_eq!(messages.messages[0].tick_ms, 10);
        assert_eq!(
            DateTime::parse_from_rfc3339(&messages.messages[0].timestamp)
                .unwrap()
                .with_timezone(&Utc),
            observed_at(2)
        );
    }

    #[test]
    fn a_reloaded_dll_discards_the_previous_identity_history() {
        let first = hello(1);
        let first_identity = ClientIdentity::from_hello(first);
        let mut store = MessageStore::default();
        store.observe(
            &ConnectionEvent::StateEvents {
                pid: 42,
                identity: first_identity,
                events: vec![event(1, "old")],
            },
            observed_at(1),
        );
        store.observe(
            &ConnectionEvent::Connected {
                pid: 42,
                hello: hello(2),
                selected_version: SUPPORTED_VERSIONS.max,
            },
            observed_at(2),
        );

        assert!(
            store
                .get(first_identity, &MessageFilter::default())
                .messages
                .is_empty()
        );
    }

    #[test]
    fn history_evicts_the_oldest_messages_at_the_count_limit() {
        let identity = ClientIdentity::from_hello(hello(1));
        let mut store = MessageStore::default();
        for sequence in 1..=u32::try_from(MAX_MESSAGES_PER_CLIENT + 1).unwrap() {
            store.observe(
                &ConnectionEvent::StateEvents {
                    pid: 42,
                    identity,
                    events: vec![event(sequence, "x")],
                },
                observed_at(i64::from(sequence)),
            );
        }

        let messages = store.get(identity, &all_messages());
        assert_eq!(messages.messages.len(), MAX_MESSAGES_PER_CLIENT);
        assert_eq!(messages.messages.last().unwrap().sequence(), 2);
    }

    #[test]
    fn history_evicts_the_oldest_messages_at_the_byte_limit() {
        let identity = ClientIdentity::from_hello(hello(1));
        let mut store = MessageStore::default();
        let text = "x".repeat(4 * 1024);
        for sequence in 1..=257 {
            store.observe(
                &ConnectionEvent::StateEvents {
                    pid: 42,
                    identity,
                    events: vec![event(sequence, &text)],
                },
                observed_at(i64::from(sequence)),
            );
        }

        let messages = store.get(identity, &all_messages());
        assert_eq!(messages.messages.len(), 255);
        assert_eq!(messages.messages.last().unwrap().sequence(), 3);
    }

    #[test]
    fn history_defaults_to_the_latest_twenty_messages() {
        let identity = ClientIdentity::from_hello(hello(1));
        let mut store = MessageStore::default();
        for sequence in 1..=25 {
            store.observe(
                &ConnectionEvent::StateEvents {
                    pid: 42,
                    identity,
                    events: vec![event(sequence, "x")],
                },
                observed_at(i64::from(sequence)),
            );
        }

        let messages = store.get(identity, &MessageFilter::default());
        assert_eq!(messages.messages.len(), DEFAULT_MESSAGE_COUNT);
        assert_eq!(messages.messages[0].sequence(), 25);
        assert_eq!(messages.messages[19].sequence(), 6);
    }

    #[test]
    fn filters_channels_time_and_pagination_before_formatting() {
        let identity = ClientIdentity::from_hello(hello(1));
        let mut store = MessageStore::default();
        for (sequence, kind) in [
            (1, MessageKind::Say),
            (2, MessageKind::Shout),
            (3, MessageKind::Say),
            (4, MessageKind::Shout),
            (5, MessageKind::Whisper),
        ] {
            store.observe(
                &ConnectionEvent::StateEvents {
                    pid: 42,
                    identity,
                    events: vec![event_on(sequence, kind, "x")],
                },
                observed_at(i64::from(sequence)),
            );
        }
        let filter = MessageFilter {
            channels: Some(BTreeSet::from([MessageChannel::Say, MessageChannel::Shout])),
            since: Some(observed_at(1)),
            skip: 1,
            count: 2,
        };

        let messages = store.get(identity, &filter);
        assert_eq!(
            messages
                .messages
                .iter()
                .map(Message::sequence)
                .collect::<Vec<_>>(),
            vec![3, 2]
        );
    }

    #[test]
    fn history_is_sorted_by_timestamp_even_if_the_clock_moves() {
        let identity = ClientIdentity::from_hello(hello(1));
        let mut store = MessageStore::default();
        for (sequence, timestamp) in [(1, 10), (2, 30), (3, 20)] {
            store.observe(
                &ConnectionEvent::StateEvents {
                    pid: 42,
                    identity,
                    events: vec![event(sequence, "x")],
                },
                observed_at(timestamp),
            );
        }

        let messages = store.get(identity, &MessageFilter::default());
        assert_eq!(
            messages
                .messages
                .iter()
                .map(Message::sequence)
                .collect::<Vec<_>>(),
            vec![2, 3, 1]
        );
    }
}
