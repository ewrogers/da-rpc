use crate::packet::ParseError;

const DAMAGE_EFFECT_OPCODE: u8 = 0x13;
const MOTION_OPCODE: u8 = 0x1A;
const EFFECT_LAYER_OPCODE: u8 = 0x29;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum VisualUpdate {
    Motion {
        object_id: u32,
        animation: u8,
        duration_10ms: u16,
    },
    Effect {
        target_id: u32,
        source_id: u32,
        target_effect: u16,
        source_effect: u16,
        frame_interval_ms: i16,
    },
    Damage {
        object_id: u32,
        health_percent: u8,
    },
}

pub(crate) fn update(body: &[u8]) -> Result<Option<VisualUpdate>, ParseError> {
    match body.first().copied() {
        Some(MOTION_OPCODE) => parse_motion(body).map(Some),
        Some(EFFECT_LAYER_OPCODE) => parse_effect(body),
        Some(DAMAGE_EFFECT_OPCODE) => parse_damage(body).map(Some),
        _ => Ok(None),
    }
}

fn parse_motion(body: &[u8]) -> Result<VisualUpdate, ParseError> {
    let mut reader = Reader::new(body);
    reader.expect(MOTION_OPCODE)?;
    Ok(VisualUpdate::Motion {
        object_id: reader.u32_be()?,
        animation: reader.u8()?,
        duration_10ms: reader.u16_be()?,
    })
}

fn parse_effect(body: &[u8]) -> Result<Option<VisualUpdate>, ParseError> {
    let mut reader = Reader::new(body);
    reader.expect(EFFECT_LAYER_OPCODE)?;
    let target_id = reader.u32_be()?;
    if target_id == 0 {
        reader.u16_be()?;
        reader.u16_be()?;
        reader.u16_be()?;
        reader.u16_be()?;
        return Ok(None);
    }
    Ok(Some(VisualUpdate::Effect {
        target_id,
        source_id: reader.u32_be()?,
        target_effect: reader.u16_be()?,
        source_effect: reader.u16_be()?,
        frame_interval_ms: reader.u16_be()? as i16,
    }))
}

fn parse_damage(body: &[u8]) -> Result<VisualUpdate, ParseError> {
    let mut reader = Reader::new(body);
    reader.expect(DAMAGE_EFFECT_OPCODE)?;
    let object_id = reader.u32_be()?;
    reader.u8()?;
    let health_percent = reader.u8()?;
    reader.u8()?;
    Ok(VisualUpdate::Damage {
        object_id,
        health_percent,
    })
}

struct Reader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Reader<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn expect(&mut self, expected: u8) -> Result<(), ParseError> {
        let actual = self.u8()?;
        debug_assert_eq!(actual, expected);
        Ok(())
    }

    fn u8(&mut self) -> Result<u8, ParseError> {
        Ok(self.take(1)?[0])
    }

    fn u16_be(&mut self) -> Result<u16, ParseError> {
        Ok(u16::from_be_bytes(
            self.take(2)?.try_into().expect("two-byte slice"),
        ))
    }

    fn u32_be(&mut self) -> Result<u32, ParseError> {
        Ok(u32::from_be_bytes(
            self.take(4)?.try_into().expect("four-byte slice"),
        ))
    }

    fn take(&mut self, length: usize) -> Result<&'a [u8], ParseError> {
        let remaining = self.bytes.len().saturating_sub(self.offset);
        if length > remaining {
            return Err(ParseError::truncated(self.offset, length, remaining));
        }
        let end = self.offset + length;
        let bytes = &self.bytes[self.offset..end];
        self.offset = end;
        Ok(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_entity_visual_packets() {
        assert_eq!(
            update(&[0x1A, 0, 0, 0, 7, 13, 0, 25]).unwrap(),
            Some(VisualUpdate::Motion {
                object_id: 7,
                animation: 13,
                duration_10ms: 25,
            })
        );
        assert_eq!(
            update(&[0x29, 0, 0, 0, 7, 0, 0, 0, 8, 0, 42, 0, 17, 0, 50]).unwrap(),
            Some(VisualUpdate::Effect {
                target_id: 7,
                source_id: 8,
                target_effect: 42,
                source_effect: 17,
                frame_interval_ms: 50,
            })
        );
        assert_eq!(
            update(&[0x13, 0, 0, 0, 7, 0, 75, 0xFF]).unwrap(),
            Some(VisualUpdate::Damage {
                object_id: 7,
                health_percent: 75,
            })
        );
    }

    #[test]
    fn ignores_ground_effects_after_validating_the_body() {
        assert_eq!(
            update(&[0x29, 0, 0, 0, 0, 0, 42, 0, 50, 0, 3, 0, 6]).unwrap(),
            None
        );
        assert!(update(&[0x29, 0, 0, 0, 0, 0, 42]).is_err());
    }

    #[test]
    fn rejects_every_truncated_visual_packet() {
        for body in [
            &[0x1A, 0, 0, 0, 7, 13, 0, 25][..],
            &[0x29, 0, 0, 0, 7, 0, 0, 0, 8, 0, 42, 0, 17, 0, 50][..],
            &[0x13, 0, 0, 0, 7, 0, 75, 0xFF][..],
        ] {
            for length in 1..body.len() {
                assert!(update(&body[..length]).is_err(), "accepted {length} bytes");
            }
        }
    }
}
