use crate::packet::{PacketReader, ParseError};
use darpc_model::CollectionKind;

const ACTION_DELAY_OPCODE: u8 = 0x3F;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ActionDelay {
    pub(crate) kind: CollectionKind,
    pub(crate) slot: u8,
    pub(crate) duration_seconds: u32,
}

pub(crate) fn update(body: &[u8]) -> Result<Option<ActionDelay>, ParseError> {
    if body.first() != Some(&ACTION_DELAY_OPCODE) {
        return Ok(None);
    }
    let mut reader = PacketReader::new(body);
    reader.expect(ACTION_DELAY_OPCODE)?;
    let selector = reader.u8()?;
    let kind = match selector {
        0 => CollectionKind::Spellbook,
        1 => CollectionKind::Skillbook,
        _ => return Err(reader.invalid_u8(selector)),
    };
    let slot = reader.u8()?;
    if !(1..=90).contains(&slot) {
        return Err(reader.invalid_u8(slot));
    }
    Ok(Some(ActionDelay {
        kind,
        slot,
        duration_seconds: reader.u32_be()?,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_spell_and_skill_delays() {
        assert_eq!(
            update(&[ACTION_DELAY_OPCODE, 0, 38, 0, 0, 0, 12]).unwrap(),
            Some(ActionDelay {
                kind: CollectionKind::Spellbook,
                slot: 38,
                duration_seconds: 12,
            })
        );
        assert_eq!(
            update(&[ACTION_DELAY_OPCODE, 1, 11, 0, 0, 0, 45]).unwrap(),
            Some(ActionDelay {
                kind: CollectionKind::Skillbook,
                slot: 11,
                duration_seconds: 45,
            })
        );
    }

    #[test]
    fn rejects_invalid_or_truncated_delays() {
        assert_eq!(
            update(&[ACTION_DELAY_OPCODE, 2, 1, 0, 0, 0, 1])
                .unwrap_err()
                .offset(),
            1
        );
        assert_eq!(
            update(&[ACTION_DELAY_OPCODE, 0, 0, 0, 0, 0, 1])
                .unwrap_err()
                .offset(),
            2
        );
        assert_eq!(
            update(&[ACTION_DELAY_OPCODE, 0, 1]).unwrap_err().offset(),
            3
        );
    }

    #[test]
    fn ignores_other_packets() {
        assert_eq!(update(&[0x3E, 1]), Ok(None));
    }
}
