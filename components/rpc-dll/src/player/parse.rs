use super::*;

#[derive(Clone)]
pub(super) struct Sections {
    pub(super) id: u32,
    pub(super) equipment: Range<usize>,
    pub(super) user_state: usize,
    pub(super) info: Range<usize>,
    pub(super) legend: Range<usize>,
}

pub(super) fn scan(body: &[u8]) -> Option<Sections> {
    let mut reader = Reader::new(body);
    reader.expect(RESPONSE_OPCODE)?;
    let id = reader.u32_be()?;
    let equipment = reader.position..reader.position.checked_add(18 * 3)?;
    reader.take(18 * 3)?;
    let user_state = reader.position;
    reader.u8()?;
    reader.string8()?;
    let info_start = reader.position;
    Nation::from_raw(reader.u8()?)?;
    reader.string8()?;
    reader.u8()?;
    reader.string8()?;
    reader.string8()?;
    reader.string8()?;
    let info = info_start..reader.position;
    let legend_start = reader.position;
    let count = reader.u8()?;
    for _ in 0..count {
        reader.take(2)?;
        reader.string8()?;
        reader.string8()?;
    }
    let legend = legend_start..reader.position;
    let content_length = usize::from(reader.u16_be()?);
    if content_length != 0 {
        let portrait_length = usize::from(reader.u16_be()?);
        reader.take(portrait_length)?;
        reader.string16()?;
    }
    Some(Sections {
        id,
        equipment,
        user_state,
        info,
        legend,
    })
}

pub(super) fn parse_profile(body: &[u8], inspected_tick_ms: u32) -> Option<PlayerProfile> {
    scan(body)?;
    let mut reader = Reader::new(body);
    reader.expect(RESPONSE_OPCODE)?;
    reader.u32_be()?;
    let mut equipment = Vec::with_capacity(18);
    for slot in EQUIPMENT_SLOTS {
        let sprite = reader.u16_be()?;
        let dye_color = reader.u8()?;
        if sprite != 0 {
            equipment.push(PlayerEquipmentItem {
                slot: EquipmentSlot::from_raw(slot)?,
                sprite,
                dye_color,
            });
        }
    }
    let user_state = UserState::from_raw(reader.u8()?);
    reader.string8()?;
    let nation = Nation::from_raw(reader.u8()?)?;
    let title = decode(reader.string8()?)?;
    let is_group_open = reader.u8()? != 0;
    let guild_rank = decode(reader.string8()?)?;
    let display_class = decode(reader.string8()?)?;
    let guild = decode(reader.string8()?)?;
    let count = reader.u8()?;
    let mut legend = Vec::with_capacity(usize::from(count));
    for _ in 0..count {
        let icon = LegendIcon::from_raw(reader.u8()?);
        let color = reader.u8()?;
        let tag = decode(reader.string8()?)?;
        let text = decode(reader.string8()?)?;
        legend.push(LegendMark {
            icon,
            color,
            tag,
            text,
        });
    }
    Some(PlayerProfile {
        identity: PlayerIdentity {
            nation,
            title,
            guild_rank,
            display_class,
            guild,
        },
        user_state,
        is_group_open,
        equipment,
        legend,
        inspected_tick_ms,
    })
}

pub(super) fn parse_self_identity(body: &[u8]) -> Option<RawIdentity> {
    let mut reader = Reader::new(body);
    reader.expect(0x39)?;
    let nation = reader.u8()?;
    Nation::from_raw(nation)?;
    let guild_rank = RawText::try_from_bytes(reader.string8()?)?;
    let title = RawText::try_from_bytes(reader.string8()?)?;
    reader.string8()?;
    reader.take(1)?;
    let recruiting = reader.u8()?;
    if recruiting == 1 {
        reader.string8()?;
        reader.string8()?;
        reader.string8()?;
        reader.take(12)?;
    }
    reader.take(3)?;
    let display_class = RawText::try_from_bytes(reader.string8()?)?;
    let guild = RawText::try_from_bytes(reader.string8()?)?;
    Some(RawIdentity {
        nation,
        title,
        guild_rank,
        display_class,
        guild,
    })
}

struct Reader<'a> {
    body: &'a [u8],
    position: usize,
}

impl<'a> Reader<'a> {
    const fn new(body: &'a [u8]) -> Self {
        Self { body, position: 0 }
    }

    fn expect(&mut self, expected: u8) -> Option<()> {
        (self.u8()? == expected).then_some(())
    }

    fn u8(&mut self) -> Option<u8> {
        let value = *self.body.get(self.position)?;
        self.position += 1;
        Some(value)
    }

    fn u16_be(&mut self) -> Option<u16> {
        Some(u16::from_be_bytes(self.take(2)?.try_into().ok()?))
    }

    fn u32_be(&mut self) -> Option<u32> {
        Some(u32::from_be_bytes(self.take(4)?.try_into().ok()?))
    }

    fn string8(&mut self) -> Option<&'a [u8]> {
        let length = usize::from(self.u8()?);
        self.take(length)
    }

    fn string16(&mut self) -> Option<&'a [u8]> {
        let length = usize::from(self.u16_be()?);
        self.take(length)
    }

    fn take(&mut self, length: usize) -> Option<&'a [u8]> {
        let end = self.position.checked_add(length)?;
        let value = self.body.get(self.position..end)?;
        self.position = end;
        Some(value)
    }
}
