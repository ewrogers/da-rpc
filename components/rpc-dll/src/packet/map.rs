use super::{PacketReader, ParseError};

const MAP_PART_OPCODE: u8 = 0x3C;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct MapPart {
    pub(crate) row_index: u16,
    pub(crate) body_length: usize,
}

pub(crate) fn update(body: &[u8]) -> Result<Option<MapPart>, ParseError> {
    if body.first() != Some(&MAP_PART_OPCODE) {
        return Ok(None);
    }
    let mut reader = PacketReader::new(body);
    reader.expect(MAP_PART_OPCODE)?;
    Ok(Some(MapPart {
        row_index: reader.u16_be()?,
        body_length: body.len(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_row_index_and_preserves_the_body_length() {
        assert_eq!(
            update(&[0x3C, 0x01, 0x02, 1, 2, 3]),
            Ok(Some(MapPart {
                row_index: 0x0102,
                body_length: 6,
            }))
        );
        assert_eq!(update(&[0x3C, 0]), Err(ParseError::truncated(1, 2, 1)));
    }
}
