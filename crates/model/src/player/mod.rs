use crate::{EquipmentSlot, LegendMark, UserState};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Nation {
    None,
    Suomi,
    Unknown1,
    Loures,
    Mileth,
    Tagor,
    Rucesion,
    Noes,
    Unknown2,
    Piet,
    Unknown3,
    Abel,
    Undine,
    Unknown4,
}

impl Nation {
    #[must_use]
    pub const fn from_raw(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::None),
            1 => Some(Self::Suomi),
            2 => Some(Self::Unknown1),
            3 => Some(Self::Loures),
            4 => Some(Self::Mileth),
            5 => Some(Self::Tagor),
            6 => Some(Self::Rucesion),
            7 => Some(Self::Noes),
            8 => Some(Self::Unknown2),
            9 => Some(Self::Piet),
            10 => Some(Self::Unknown3),
            11 => Some(Self::Abel),
            12 => Some(Self::Undine),
            13 => Some(Self::Unknown4),
            _ => None,
        }
    }

    #[must_use]
    pub const fn raw(self) -> u8 {
        match self {
            Self::None => 0,
            Self::Suomi => 1,
            Self::Unknown1 => 2,
            Self::Loures => 3,
            Self::Mileth => 4,
            Self::Tagor => 5,
            Self::Rucesion => 6,
            Self::Noes => 7,
            Self::Unknown2 => 8,
            Self::Piet => 9,
            Self::Unknown3 => 10,
            Self::Abel => 11,
            Self::Undine => 12,
            Self::Unknown4 => 13,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlayerIdentity {
    pub nation: Nation,
    pub title: String,
    pub guild_rank: String,
    pub display_class: String,
    pub guild: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PlayerEquipmentItem {
    pub slot: EquipmentSlot,
    pub sprite: u16,
    pub dye_color: u8,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlayerProfile {
    pub identity: PlayerIdentity,
    pub user_state: UserState,
    pub is_group_open: bool,
    pub equipment: Vec<PlayerEquipmentItem>,
    pub legend: Vec<LegendMark>,
    pub inspected_tick_ms: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PlayerInspectionChanges {
    pub info: bool,
    pub equipment: bool,
    pub legend: bool,
}

impl PlayerInspectionChanges {
    #[must_use]
    pub const fn all() -> Self {
        Self {
            info: true,
            equipment: true,
            legend: true,
        }
    }

    #[must_use]
    pub fn between(previous: Option<&PlayerProfile>, current: &PlayerProfile) -> Self {
        let Some(previous) = previous else {
            return Self::all();
        };
        Self {
            info: previous.identity != current.identity
                || previous.user_state != current.user_state
                || previous.is_group_open != current.is_group_open,
            equipment: previous.equipment != current.equipment,
            legend: previous.legend != current.legend,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlayerInspectionTrigger {
    Appeared,
    Manual,
    User,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlayerUpdate {
    pub player: crate::WorldObject,
    pub changes: PlayerInspectionChanges,
    pub trigger: PlayerInspectionTrigger,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CharacterProfileUpdate {
    pub previous: Option<PlayerIdentity>,
    pub current: PlayerIdentity,
}

#[cfg(test)]
mod tests {
    use super::Nation;

    #[test]
    fn nation_values_round_trip() {
        for value in 0..=13 {
            let nation = Nation::from_raw(value).expect("known nation value");
            assert_eq!(nation.raw(), value);
        }
        assert_eq!(Nation::from_raw(14), None);
        assert_eq!(Nation::from_raw(u8::MAX), None);
    }
}
