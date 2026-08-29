/// Maximum number of nearby world objects retained in one client snapshot.
pub const MAX_WORLD_OBJECTS: usize = 1_024;

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
pub struct HumanVisual {
    pub gender: crate::snapshot::Gender,
    pub head_sprite: u16,
    pub body_sprite: u16,
    pub arms_sprite: u16,
    pub boots_sprite: u16,
    pub pants_sprite: u16,
    pub armor_sprite: u16,
    pub weapon_sprite: u16,
    pub shield_sprite: u16,
    pub overcoat_sprite: u16,
    pub accessory1_sprite: u16,
    pub accessory2_sprite: u16,
    pub accessory3_sprite: u16,
    pub hair_color: u8,
    pub skin_color: u8,
    pub boots_color: u8,
    pub pants_color: u8,
    pub overcoat_color: u8,
    pub accessory1_color: u8,
    pub accessory2_color: u8,
    pub accessory3_color: u8,
    pub rest_position: u8,
    pub face_shape: u8,
    pub is_translucent: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PlayerVisual {
    Human(HumanVisual),
    Creature {
        sprite: u16,
        color: u8,
        boots_color: u8,
        pants_color: u8,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WorldObject {
    Player {
        id: u32,
        name: Option<String>,
        x: i32,
        y: i32,
        direction: Direction,
        is_hidden: bool,
        visual: Option<PlayerVisual>,
        profile: Option<Box<crate::PlayerProfile>>,
    },
    Creature {
        id: u32,
        kind: CreatureKind,
        is_solid: bool,
        sprite: Option<u16>,
        name: Option<String>,
        x: i32,
        y: i32,
        direction: Direction,
    },
    Item {
        id: u32,
        sprite: u16,
        dye_color: u8,
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

    #[must_use]
    pub const fn is_solid(&self) -> bool {
        match self {
            Self::Player { .. } => true,
            Self::Creature { is_solid, .. } => *is_solid,
            Self::Item { .. } => false,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ObjectUpdate {
    Appeared(WorldObject),
    Disappeared(WorldObject),
    Moved(WorldObject),
    DirectionChanged(WorldObject),
}
