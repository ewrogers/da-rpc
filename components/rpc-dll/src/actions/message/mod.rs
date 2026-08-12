use super::network;
use darpc_protocol::{
    CommandFailure, MAX_MESSAGE_CONTENT_LEN, MAX_MESSAGE_RECIPIENT_LEN, MessageCommand,
    MessageContent, MessageRecipient,
};

const TALK_OPCODE: u8 = 0x0E;
const WHISPER_OPCODE: u8 = 0x19;
const SAY_MODE: u8 = 0;
const SHOUT_MODE: u8 = 1;
const MAX_PACKET_LEN: usize = MAX_MESSAGE_RECIPIENT_LEN + MAX_MESSAGE_CONTENT_LEN + 3;

pub(super) fn submit(message: MessageCommand) -> Result<(), CommandFailure> {
    let (body, length) = packet(message);
    network::submit(&body[..length])
}

fn packet(message: MessageCommand) -> ([u8; MAX_PACKET_LEN], usize) {
    match message {
        MessageCommand::Say(content) => talk_packet(SAY_MODE, content, true),
        MessageCommand::Shout(content) => talk_packet(SHOUT_MODE, content, false),
        MessageCommand::Whisper { recipient, content } => whisper_packet(recipient, content),
        MessageCommand::Guild(content) => whisper_packet(channel_recipient("!"), content),
        MessageCommand::Group(content) => whisper_packet(channel_recipient("!!"), content),
    }
}

fn talk_packet(
    mode: u8,
    content: MessageContent,
    escape_command_prefix: bool,
) -> ([u8; MAX_PACKET_LEN], usize) {
    let content = content.as_bytes();
    let escaped = escape_command_prefix && content.starts_with(b"/");
    let content_length = content.len() + usize::from(escaped);
    let length = content_length + 3;
    let mut body = [0; MAX_PACKET_LEN];
    body[..3].copy_from_slice(&[
        TALK_OPCODE,
        mode,
        u8::try_from(content_length).expect("message content length fits u8"),
    ]);
    let start = 3 + usize::from(escaped);
    if escaped {
        body[3] = b'/';
    }
    body[start..length].copy_from_slice(content);
    (body, length)
}

fn whisper_packet(
    recipient: MessageRecipient,
    content: MessageContent,
) -> ([u8; MAX_PACKET_LEN], usize) {
    let recipient = recipient.as_bytes();
    let content = content.as_bytes();
    let content_start = recipient.len() + 2;
    let length = content_start + content.len() + 1;
    let mut body = [0; MAX_PACKET_LEN];
    body[0] = WHISPER_OPCODE;
    body[1] = u8::try_from(recipient.len()).expect("message recipient length fits u8");
    body[2..content_start].copy_from_slice(recipient);
    body[content_start] = u8::try_from(content.len()).expect("message content length fits u8");
    body[content_start + 1..length].copy_from_slice(content);
    (body, length)
}

fn channel_recipient(value: &str) -> MessageRecipient {
    MessageRecipient::channel(value).expect("fixed message channel recipient is valid")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn content(value: &str) -> MessageContent {
        MessageContent::new(value).unwrap()
    }

    #[test]
    fn public_message_packets_use_talk_modes() {
        let (say, say_length) = packet(MessageCommand::Say(content("hello")));
        assert_eq!(&say[..say_length], b"\x0E\x00\x05hello");
        let (shout, shout_length) = packet(MessageCommand::Shout(content("hello")));
        assert_eq!(&shout[..shout_length], b"\x0E\x01\x05hello");
    }

    #[test]
    fn say_escapes_the_local_command_interceptor() {
        let (body, length) = packet(MessageCommand::Say(content("/walk 1,2")));
        assert_eq!(&body[..length], b"\x0E\x00\x0A//walk 1,2");
    }

    #[test]
    fn directed_message_packets_use_whisper_targets() {
        let recipient = MessageRecipient::new("Eidolon").unwrap();
        let (whisper, whisper_length) = packet(MessageCommand::Whisper {
            recipient,
            content: content("hello"),
        });
        assert_eq!(&whisper[..whisper_length], b"\x19\x07Eidolon\x05hello");
        let (guild, guild_length) = packet(MessageCommand::Guild(content("hello")));
        assert_eq!(&guild[..guild_length], b"\x19\x01!\x05hello");
        let (group, group_length) = packet(MessageCommand::Group(content("hello")));
        assert_eq!(&group[..group_length], b"\x19\x02!!\x05hello");
    }
}
