#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EffectDuration {
    Blue,
    Green,
    Yellow,
    Orange,
    Red,
    White,
}

impl EffectDuration {
    #[must_use]
    pub const fn from_raw(value: u8) -> Option<Self> {
        Some(match value {
            1 => Self::Blue,
            2 => Self::Green,
            3 => Self::Yellow,
            4 => Self::Orange,
            5 => Self::Red,
            6 => Self::White,
            _ => return None,
        })
    }

    #[must_use]
    pub const fn raw(self) -> u8 {
        match self {
            Self::Blue => 1,
            Self::Green => 2,
            Self::Yellow => 3,
            Self::Orange => 4,
            Self::Red => 5,
            Self::White => 6,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Effect {
    pub icon: u16,
    /// Relative remaining-duration band. This is not an exact time value.
    pub duration: EffectDuration,
}
