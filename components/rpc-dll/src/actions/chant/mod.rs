use super::network;
use darpc_protocol::{ChantText, CommandFailure, MAX_CHANT_TEXT_LEN};

const MESSAGE_OPCODE: u8 = 0x0E;
const CHANT_MODE: u8 = 2;

pub(super) fn submit(text: ChantText) -> Result<(), CommandFailure> {
    let (body, length) = packet(text);
    network::submit(&body[..length])
}

fn packet(text: ChantText) -> ([u8; MAX_CHANT_TEXT_LEN + 3], usize) {
    let bytes = text.as_bytes();
    let length = bytes.len() + 3;
    let mut body = [0; MAX_CHANT_TEXT_LEN + 3];
    body[..3].copy_from_slice(&[
        MESSAGE_OPCODE,
        CHANT_MODE,
        u8::try_from(bytes.len()).expect("chant text length fits u8"),
    ]);
    body[3..length].copy_from_slice(bytes);
    (body, length)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packet_preserves_chant_text_exactly() {
        let text = ChantText::new("buy my Dark-Belt  ").unwrap();
        let (body, length) = packet(text);
        assert_eq!(&body[..length], b"\x0E\x02\x12buy my Dark-Belt  ");
    }
}
