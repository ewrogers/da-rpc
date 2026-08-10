#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LegendIcon {
    Aisling,
    Warrior,
    Rogue,
    Wizard,
    Priest,
    Monk,
    Heart,
    Victory,
    None,
    Unknown(u8),
}

impl LegendIcon {
    #[must_use]
    pub const fn from_raw(value: u8) -> Self {
        match value {
            0 => Self::Aisling,
            1 => Self::Warrior,
            2 => Self::Rogue,
            3 => Self::Wizard,
            4 => Self::Priest,
            5 => Self::Monk,
            6 => Self::Heart,
            7 => Self::Victory,
            8 => Self::None,
            value => Self::Unknown(value),
        }
    }

    #[must_use]
    pub const fn raw(self) -> u8 {
        match self {
            Self::Aisling => 0,
            Self::Warrior => 1,
            Self::Rogue => 2,
            Self::Wizard => 3,
            Self::Priest => 4,
            Self::Monk => 5,
            Self::Heart => 6,
            Self::Victory => 7,
            Self::None => 8,
            Self::Unknown(value) => value,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LegendMark {
    pub text: String,
    pub tag: String,
    pub color: u8,
    pub icon: LegendIcon,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LegendUpdate {
    MarkAdded {
        mark: LegendMark,
    },
    MarkChanged {
        previous: LegendMark,
        current: LegendMark,
    },
    MarkRemoved {
        mark: LegendMark,
    },
}
