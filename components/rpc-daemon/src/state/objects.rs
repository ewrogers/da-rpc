use super::ObservationMetadata;
use darpc_model::{
    CreatureKind as ModelCreatureKind, Direction as ModelDirection, WorldObject as ModelWorldObject,
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
    Npc,
    Item,
}

impl FromStr for WorldObjectKind {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "player" => Ok(Self::Player),
            "monster" => Ok(Self::Monster),
            "npc" => Ok(Self::Npc),
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
    },
    Monster {
        id: u32,
        sprite: Option<u16>,
        x: i32,
        y: i32,
        direction: Direction,
    },
    Npc {
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
            Self::Npc { .. } => WorldObjectKind::Npc,
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
            } => Self::Player {
                id: *id,
                name: name.clone(),
                x: *x,
                y: *y,
                direction: (*direction).into(),
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
            } => Self::Npc {
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
