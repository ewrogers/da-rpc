#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Direction {
    North,
    East,
    South,
    West,
}

impl Direction {
    #[must_use]
    pub const fn from_raw(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::North),
            1 => Some(Self::East),
            2 => Some(Self::South),
            3 => Some(Self::West),
            _ => None,
        }
    }

    #[must_use]
    pub const fn raw(self) -> u8 {
        match self {
            Self::North => 0,
            Self::East => 1,
            Self::South => 2,
            Self::West => 3,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CreatureKind {
    Monster,
    Npc,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WorldObject {
    Player {
        id: u32,
        name: Option<String>,
        x: i32,
        y: i32,
        direction: Direction,
        profile: Option<Box<crate::PlayerProfile>>,
    },
    Creature {
        id: u32,
        kind: CreatureKind,
        sprite: Option<u16>,
        name: Option<String>,
        x: i32,
        y: i32,
        direction: Direction,
    },
    Item {
        id: u32,
        sprite: u16,
        x: i32,
        y: i32,
        /// Zero is the bottom item at a tile. Higher values are drawn above it.
        z_index: u16,
    },
}

impl WorldObject {
    #[must_use]
    pub const fn id(&self) -> u32 {
        match self {
            Self::Player { id, .. } | Self::Creature { id, .. } | Self::Item { id, .. } => *id,
        }
    }

    #[must_use]
    pub const fn position(&self) -> (i32, i32) {
        match self {
            Self::Player { x, y, .. } | Self::Creature { x, y, .. } | Self::Item { x, y, .. } => {
                (*x, *y)
            }
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ObjectUpdate {
    Appeared(WorldObject),
    Disappeared(WorldObject),
    Moved(WorldObject),
    DirectionChanged(WorldObject),
    Cleared,
}
