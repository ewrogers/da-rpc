use crate::CharacterClass;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WhoList {
    pub world_count: u16,
    pub country_count: u16,
    pub players: Vec<WhoPlayer>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WhoPlayer {
    pub name: String,
    pub title: String,
    pub class: CharacterClass,
    pub state: UserState,
    pub color: u8,
    pub is_master: bool,
    pub is_guildmate: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UserState {
    Awake,
    DoNotDisturb,
    Daydreaming,
    NeedGroup,
    Grouped,
    LoneHunter,
    GroupHunting,
    NeedHelp,
    Unknown(u8),
}

impl UserState {
    #[must_use]
    pub const fn from_raw(value: u8) -> Self {
        match value {
            0 => Self::Awake,
            1 => Self::DoNotDisturb,
            2 => Self::Daydreaming,
            3 => Self::NeedGroup,
            4 => Self::Grouped,
            5 => Self::LoneHunter,
            6 => Self::GroupHunting,
            7 => Self::NeedHelp,
            value => Self::Unknown(value),
        }
    }

    #[must_use]
    pub const fn raw(self) -> u8 {
        match self {
            Self::Awake => 0,
            Self::DoNotDisturb => 1,
            Self::Daydreaming => 2,
            Self::NeedGroup => 3,
            Self::Grouped => 4,
            Self::LoneHunter => 5,
            Self::GroupHunting => 6,
            Self::NeedHelp => 7,
            Self::Unknown(value) => value,
        }
    }
}
