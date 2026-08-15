use crate::{state::ObservationMetadata, stream::EventObservation};
use darpc_model as model;
use serde::Serialize;
use utoipa::ToSchema;

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct FieldMapSnapshot {
    /// Metadata for the retained client snapshot.
    observation: ObservationMetadata,
    /// Current field map, or null when the native panel is not active.
    field_map: Option<FieldMapState>,
}

impl FieldMapSnapshot {
    pub(crate) fn from_model(pid: u32, snapshot: &model::ClientSnapshot) -> Self {
        Self {
            observation: ObservationMetadata::from_model(pid, snapshot),
            field_map: snapshot.active_field_map.as_ref().map(FieldMapState::from),
        }
    }
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct FieldMapState {
    /// Wrapping nonzero instance revision required by selection commands.
    revision: u32,
    /// Local field-map asset stem, such as `field001`.
    field_name: String,
    /// Zero-based current-node index, or null when the packet index is invalid.
    current_node_index: Option<u8>,
    destinations: Vec<FieldMapDestination>,
    /// Destination whose CFieldMap packet was actually submitted, if any.
    selection: Option<FieldMapSelection>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct FieldMapDestination {
    index: u8,
    /// Server fallback screen coordinate; the local field asset may override it.
    screen_x: u16,
    /// Server fallback screen coordinate; the local field asset may override it.
    screen_y: u16,
    name: String,
    /// Server-issued destination token. Read-only; select by revision and index.
    checksum: u16,
    map_id: u16,
    map_x: u16,
    map_y: u16,
}

#[derive(Clone, Copy, Debug, Serialize, ToSchema)]
pub(crate) struct FieldMapSelection {
    destination_index: u8,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct FieldMapOpened {
    pub(crate) observation: EventObservation,
    field_map: FieldMapState,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct FieldMapChanged {
    pub(crate) observation: EventObservation,
    field_map: FieldMapState,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct FieldMapSelectionSubmitted {
    pub(crate) observation: EventObservation,
    field_map: FieldMapState,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct FieldMapClosed {
    pub(crate) observation: EventObservation,
    previous: FieldMapState,
}

impl FieldMapOpened {
    pub(crate) fn new(observation: EventObservation, field_map: model::FieldMapState) -> Self {
        Self {
            observation,
            field_map: FieldMapState::from(&field_map),
        }
    }
}

impl FieldMapChanged {
    pub(crate) fn new(observation: EventObservation, field_map: model::FieldMapState) -> Self {
        Self {
            observation,
            field_map: FieldMapState::from(&field_map),
        }
    }
}

impl FieldMapSelectionSubmitted {
    pub(crate) fn new(observation: EventObservation, field_map: model::FieldMapState) -> Self {
        Self {
            observation,
            field_map: FieldMapState::from(&field_map),
        }
    }
}

impl FieldMapClosed {
    pub(crate) fn new(observation: EventObservation, previous: model::FieldMapState) -> Self {
        Self {
            observation,
            previous: FieldMapState::from(&previous),
        }
    }
}

impl From<&model::FieldMapState> for FieldMapState {
    fn from(value: &model::FieldMapState) -> Self {
        Self {
            revision: value.revision,
            field_name: value.field_name.clone(),
            current_node_index: value.current_node_index,
            destinations: value
                .destinations
                .iter()
                .map(FieldMapDestination::from)
                .collect(),
            selection: value.selection.map(FieldMapSelection::from),
        }
    }
}

impl From<&model::FieldMapDestination> for FieldMapDestination {
    fn from(value: &model::FieldMapDestination) -> Self {
        Self {
            index: value.index,
            screen_x: value.screen_x,
            screen_y: value.screen_y,
            name: value.name.clone(),
            checksum: value.checksum,
            map_id: value.map_id,
            map_x: value.map_x,
            map_y: value.map_y,
        }
    }
}

impl From<model::FieldMapSelection> for FieldMapSelection {
    fn from(value: model::FieldMapSelection) -> Self {
        Self {
            destination_index: value.destination_index,
        }
    }
}
