#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FieldMapState {
    pub revision: u32,
    pub field_name: String,
    pub current_node_index: Option<u8>,
    pub destinations: Vec<FieldMapDestination>,
    pub selection: Option<FieldMapSelection>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FieldMapDestination {
    pub index: u8,
    pub screen_x: u16,
    pub screen_y: u16,
    pub name: String,
    pub checksum: u16,
    pub map_id: u16,
    pub map_x: u16,
    pub map_y: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FieldMapSelection {
    pub destination_index: u8,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FieldMapUpdate {
    Opened(FieldMapState),
    Changed(FieldMapState),
    SelectionSubmitted(FieldMapState),
    Closed { previous: FieldMapState },
}
