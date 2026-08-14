use super::{CharacterGender, ObservationMetadata};
use darpc_model::{
    CreatureKind as ModelCreatureKind, Direction as ModelDirection,
    HumanVisual as ModelHumanVisual, PlayerVisual as ModelPlayerVisual,
    WorldObject as ModelWorldObject,
};
use serde::Serialize;
use std::str::FromStr;
use utoipa::ToSchema;

#[derive(Debug, Eq, PartialEq, Serialize, ToSchema)]
pub(crate) struct WorldObjects {
    observation: ObservationMetadata,
    objects: Option<Vec<WorldObject>>,
}

impl WorldObjects {
    pub(crate) fn from_model(
        pid: u32,
        snapshot: &darpc_model::ClientSnapshot,
        kinds: Option<&[WorldObjectKind]>,
    ) -> Self {
        Self {
            observation: ObservationMetadata::from_model(pid, snapshot),
            objects: snapshot.objects.as_ref().map(|objects| {
                objects
                    .iter()
                    .map(WorldObject::from)
                    .filter(|object| kinds.is_none_or(|kinds| kinds.contains(&object.kind())))
                    .collect()
            }),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum WorldObjectKind {
    Player,
    Monster,
    Mundane,
    Item,
}

impl FromStr for WorldObjectKind {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "player" => Ok(Self::Player),
            "monster" => Ok(Self::Monster),
            "mundane" | "npc" => Ok(Self::Mundane),
            "item" => Ok(Self::Item),
            _ => Err(()),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, ToSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum WorldObject {
    Player {
        id: u32,
        name: Option<String>,
        x: i32,
        y: i32,
        direction: Direction,
        is_hidden: bool,
        visual: Option<PlayerVisual>,
        profile: Option<PlayerProfile>,
    },
    Monster {
        id: u32,
        sprite: Option<u16>,
        x: i32,
        y: i32,
        direction: Direction,
    },
    Mundane {
        id: u32,
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
        /// Per-tile stack order. Zero is the bottom item.
        z_index: u16,
    },
}

impl WorldObject {
    fn kind(&self) -> WorldObjectKind {
        match self {
            Self::Player { .. } => WorldObjectKind::Player,
            Self::Monster { .. } => WorldObjectKind::Monster,
            Self::Mundane { .. } => WorldObjectKind::Mundane,
            Self::Item { .. } => WorldObjectKind::Item,
        }
    }
}

impl From<&ModelWorldObject> for WorldObject {
    fn from(object: &ModelWorldObject) -> Self {
        match object {
            ModelWorldObject::Player {
                id,
                name,
                x,
                y,
                direction,
                is_hidden,
                visual,
                profile,
            } => Self::Player {
                id: *id,
                name: name.clone(),
                x: *x,
                y: *y,
                direction: (*direction).into(),
                is_hidden: *is_hidden,
                visual: visual.as_ref().map(PlayerVisual::from),
                profile: profile.as_deref().map(PlayerProfile::from),
            },
            ModelWorldObject::Creature {
                id,
                kind: ModelCreatureKind::Monster,
                sprite,
                x,
                y,
                direction,
                ..
            } => Self::Monster {
                id: *id,
                sprite: *sprite,
                x: *x,
                y: *y,
                direction: (*direction).into(),
            },
            ModelWorldObject::Creature {
                id,
                kind: ModelCreatureKind::Npc,
                sprite,
                name,
                x,
                y,
                direction,
            } => Self::Mundane {
                id: *id,
                sprite: *sprite,
                name: name.clone(),
                x: *x,
                y: *y,
                direction: (*direction).into(),
            },
            ModelWorldObject::Item {
                id,
                sprite,
                x,
                y,
                z_index,
            } => Self::Item {
                id: *id,
                sprite: *sprite,
                x: *x,
                y: *y,
                z_index: *z_index,
            },
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, ToSchema)]
#[serde(tag = "form", rename_all = "snake_case")]
pub(crate) enum PlayerVisual {
    Human {
        gender: CharacterGender,
        head_sprite: u16,
        body_sprite: u16,
        arms_sprite: u16,
        boots_sprite: u16,
        pants_sprite: u16,
        armor_sprite: u16,
        weapon_sprite: u16,
        shield_sprite: u16,
        overcoat_sprite: u16,
        accessory1_sprite: u16,
        accessory2_sprite: u16,
        accessory3_sprite: u16,
        hair_color: u8,
        skin_color: u8,
        boots_color: u8,
        pants_color: u8,
        overcoat_color: u8,
        accessory1_color: u8,
        accessory2_color: u8,
        accessory3_color: u8,
        rest_position: u8,
        face_shape: u8,
        is_translucent: bool,
    },
    Creature {
        sprite: u16,
        color: u8,
        boots_color: u8,
        pants_color: u8,
    },
}

impl From<&ModelPlayerVisual> for PlayerVisual {
    fn from(visual: &ModelPlayerVisual) -> Self {
        match visual {
            ModelPlayerVisual::Human(ModelHumanVisual {
                gender,
                head_sprite,
                body_sprite,
                arms_sprite,
                boots_sprite,
                pants_sprite,
                armor_sprite,
                weapon_sprite,
                shield_sprite,
                overcoat_sprite,
                accessory1_sprite,
                accessory2_sprite,
                accessory3_sprite,
                hair_color,
                skin_color,
                boots_color,
                pants_color,
                overcoat_color,
                accessory1_color,
                accessory2_color,
                accessory3_color,
                rest_position,
                face_shape,
                is_translucent,
            }) => Self::Human {
                gender: (*gender).into(),
                head_sprite: *head_sprite,
                body_sprite: *body_sprite,
                arms_sprite: *arms_sprite,
                boots_sprite: *boots_sprite,
                pants_sprite: *pants_sprite,
                armor_sprite: *armor_sprite,
                weapon_sprite: *weapon_sprite,
                shield_sprite: *shield_sprite,
                overcoat_sprite: *overcoat_sprite,
                accessory1_sprite: *accessory1_sprite,
                accessory2_sprite: *accessory2_sprite,
                accessory3_sprite: *accessory3_sprite,
                hair_color: *hair_color,
                skin_color: *skin_color,
                boots_color: *boots_color,
                pants_color: *pants_color,
                overcoat_color: *overcoat_color,
                accessory1_color: *accessory1_color,
                accessory2_color: *accessory2_color,
                accessory3_color: *accessory3_color,
                rest_position: *rest_position,
                face_shape: *face_shape,
                is_translucent: *is_translucent,
            },
            ModelPlayerVisual::Creature {
                sprite,
                color,
                boots_color,
                pants_color,
            } => Self::Creature {
                sprite: *sprite,
                color: *color,
                boots_color: *boots_color,
                pants_color: *pants_color,
            },
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, ToSchema)]
pub(crate) struct PlayerProfile {
    identity: PlayerIdentity,
    user_state: UserState,
    is_group_open: bool,
    equipment: Vec<PlayerEquipmentItem>,
    legend: Vec<PlayerLegendMark>,
    inspected_tick_ms: u32,
}

impl From<&darpc_model::PlayerProfile> for PlayerProfile {
    fn from(profile: &darpc_model::PlayerProfile) -> Self {
        Self {
            identity: PlayerIdentity::from(&profile.identity),
            user_state: UserState::from(profile.user_state),
            is_group_open: profile.is_group_open,
            equipment: profile
                .equipment
                .iter()
                .map(PlayerEquipmentItem::from)
                .collect(),
            legend: profile.legend.iter().map(PlayerLegendMark::from).collect(),
            inspected_tick_ms: profile.inspected_tick_ms,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, ToSchema)]
pub(crate) struct PlayerIdentity {
    nation: Nation,
    title: String,
    guild_rank: String,
    display_class: String,
    guild: String,
}

impl From<&darpc_model::PlayerIdentity> for PlayerIdentity {
    fn from(identity: &darpc_model::PlayerIdentity) -> Self {
        Self {
            nation: Nation::from(identity.nation),
            title: identity.title.clone(),
            guild_rank: identity.guild_rank.clone(),
            display_class: identity.display_class.clone(),
            guild: identity.guild.clone(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Nation {
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

impl From<darpc_model::Nation> for Nation {
    fn from(nation: darpc_model::Nation) -> Self {
        match nation {
            darpc_model::Nation::None => Self::None,
            darpc_model::Nation::Suomi => Self::Suomi,
            darpc_model::Nation::Unknown1 => Self::Unknown1,
            darpc_model::Nation::Loures => Self::Loures,
            darpc_model::Nation::Mileth => Self::Mileth,
            darpc_model::Nation::Tagor => Self::Tagor,
            darpc_model::Nation::Rucesion => Self::Rucesion,
            darpc_model::Nation::Noes => Self::Noes,
            darpc_model::Nation::Unknown2 => Self::Unknown2,
            darpc_model::Nation::Piet => Self::Piet,
            darpc_model::Nation::Unknown3 => Self::Unknown3,
            darpc_model::Nation::Abel => Self::Abel,
            darpc_model::Nation::Undine => Self::Undine,
            darpc_model::Nation::Unknown4 => Self::Unknown4,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum UserState {
    Awake,
    DoNotDisturb,
    Daydreaming,
    NeedGroup,
    Grouped,
    LoneHunter,
    GroupHunting,
    NeedHelp,
    Unknown,
}

impl From<darpc_model::UserState> for UserState {
    fn from(state: darpc_model::UserState) -> Self {
        match state {
            darpc_model::UserState::Awake => Self::Awake,
            darpc_model::UserState::DoNotDisturb => Self::DoNotDisturb,
            darpc_model::UserState::Daydreaming => Self::Daydreaming,
            darpc_model::UserState::NeedGroup => Self::NeedGroup,
            darpc_model::UserState::Grouped => Self::Grouped,
            darpc_model::UserState::LoneHunter => Self::LoneHunter,
            darpc_model::UserState::GroupHunting => Self::GroupHunting,
            darpc_model::UserState::NeedHelp => Self::NeedHelp,
            darpc_model::UserState::Unknown(_) => Self::Unknown,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, ToSchema)]
pub(crate) struct PlayerEquipmentItem {
    slot: super::EquipmentSlot,
    sprite: u16,
    dye_color: u8,
}

impl From<&darpc_model::PlayerEquipmentItem> for PlayerEquipmentItem {
    fn from(item: &darpc_model::PlayerEquipmentItem) -> Self {
        Self {
            slot: super::EquipmentSlot::from(item.slot),
            sprite: item.sprite,
            dye_color: item.dye_color,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, ToSchema)]
pub(crate) struct PlayerLegendMark {
    text: String,
    tag: String,
    color: u8,
    icon: PlayerLegendIcon,
}

impl From<&darpc_model::LegendMark> for PlayerLegendMark {
    fn from(mark: &darpc_model::LegendMark) -> Self {
        Self {
            text: mark.text.clone(),
            tag: mark.tag.clone(),
            color: mark.color,
            icon: PlayerLegendIcon::from(mark.icon),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PlayerLegendIcon {
    Aisling,
    Warrior,
    Rogue,
    Wizard,
    Priest,
    Monk,
    Heart,
    Victory,
    None,
    Unknown,
}

impl From<darpc_model::LegendIcon> for PlayerLegendIcon {
    fn from(icon: darpc_model::LegendIcon) -> Self {
        match icon {
            darpc_model::LegendIcon::Aisling => Self::Aisling,
            darpc_model::LegendIcon::Warrior => Self::Warrior,
            darpc_model::LegendIcon::Rogue => Self::Rogue,
            darpc_model::LegendIcon::Wizard => Self::Wizard,
            darpc_model::LegendIcon::Priest => Self::Priest,
            darpc_model::LegendIcon::Monk => Self::Monk,
            darpc_model::LegendIcon::Heart => Self::Heart,
            darpc_model::LegendIcon::Victory => Self::Victory,
            darpc_model::LegendIcon::None => Self::None,
            darpc_model::LegendIcon::Unknown(_) => Self::Unknown,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Direction {
    North,
    East,
    South,
    West,
}

impl From<ModelDirection> for Direction {
    fn from(direction: ModelDirection) -> Self {
        match direction {
            ModelDirection::North => Self::North,
            ModelDirection::East => Self::East,
            ModelDirection::South => Self::South,
            ModelDirection::West => Self::West,
        }
    }
}
