#![cfg_attr(test, allow(dead_code))]

use crate::{atomic_sequence::next_nonzero, client_text, transfer_slot::TransferSlot};
use darpc_model::{FieldMapDestination, FieldMapSelection, FieldMapState, FieldMapUpdate};
use darpc_protocol::{CommandFailure, FieldMapSelectionCommand};
use std::{
    cell::UnsafeCell,
    sync::atomic::{AtomicU32, Ordering},
};

pub(crate) const MAX_FIELD_MAP_PACKET_BYTES: usize = u16::MAX as usize;
const EVENT_SLOTS: usize = 4;
const NO_SELECTION: u16 = u16::MAX;

static CURRENT: CurrentFieldMap = CurrentFieldMap(UnsafeCell::new(RawFieldMap::empty()));
static EVENTS: FieldMapEvents = FieldMapEvents::new();
static REVISION: AtomicU32 = AtomicU32::new(0);

struct CurrentFieldMap(UnsafeCell<RawFieldMap>);

// SAFETY: CURRENT is read and written only by the client main thread. Copies
// cross to the IPC thread through snapshot publication or EVENTS.
unsafe impl Sync for CurrentFieldMap {}

#[derive(Clone, Copy)]
pub(crate) struct RawFieldMap {
    revision: u32,
    selection_index: u16,
    length: u16,
    bytes: [u8; MAX_FIELD_MAP_PACKET_BYTES],
}

impl RawFieldMap {
    pub(crate) const fn empty() -> Self {
        Self {
            revision: 0,
            selection_index: NO_SELECTION,
            length: 0,
            bytes: [0; MAX_FIELD_MAP_PACKET_BYTES],
        }
    }

    fn active(&self) -> bool {
        self.length != 0
    }

    fn body(&self) -> &[u8] {
        &self.bytes[..usize::from(self.length)]
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct QueuedFieldMap(u8);

#[derive(Clone, Copy)]
enum EventKind {
    Opened,
    Changed,
    SelectionSubmitted,
    Closed,
}

#[derive(Clone, Copy)]
struct RawFieldMapEvent {
    kind: EventKind,
    field_map: RawFieldMap,
}

struct FieldMapEvents {
    slots: [TransferSlot<RawFieldMapEvent>; EVENT_SLOTS],
}

impl FieldMapEvents {
    const fn new() -> Self {
        Self {
            slots: [const { TransferSlot::new() }; EVENT_SLOTS],
        }
    }

    fn push(&self, event: RawFieldMapEvent) -> Option<QueuedFieldMap> {
        for (index, slot) in self.slots.iter().enumerate() {
            if slot.try_write(event) {
                return Some(QueuedFieldMap(index as u8));
            }
        }
        None
    }

    fn take(&self, queued: QueuedFieldMap) -> Option<RawFieldMapEvent> {
        self.slots.get(usize::from(queued.0))?.try_take()
    }

    fn release(&self, queued: QueuedFieldMap) {
        if let Some(slot) = self.slots.get(usize::from(queued.0)) {
            slot.discard();
        }
    }

    fn reset(&self) {
        for slot in &self.slots {
            slot.reset();
        }
    }
}

pub(crate) fn reset() {
    // SAFETY: reset runs outside active hook and snapshot publication access.
    unsafe { *CURRENT.0.get() = RawFieldMap::empty() };
    EVENTS.reset();
    REVISION.store(0, Ordering::Release);
}

pub(crate) fn observe_server(body: &[u8]) -> Option<QueuedFieldMap> {
    if body.first() != Some(&0x2E)
        || body.len() > MAX_FIELD_MAP_PACKET_BYTES
        || validate_server_packet(body).is_none()
        || !pane_is_open()
    {
        return None;
    }
    let current = current_mut();
    let opened = !current.active();
    let revision = next_nonzero(&REVISION);
    let length = u16::try_from(body.len()).ok()?;
    let mut next = RawFieldMap::empty();
    next.revision = revision;
    next.length = length;
    next.bytes[..body.len()].copy_from_slice(body);
    *current = next;
    EVENTS.push(RawFieldMapEvent {
        kind: if opened {
            EventKind::Opened
        } else {
            EventKind::Changed
        },
        field_map: next,
    })
}

pub(crate) fn observe_outgoing(body: &[u8]) -> Option<QueuedFieldMap> {
    if body.len() != 9 || body.first() != Some(&0x3F) {
        return None;
    }
    let current = current_mut();
    if !current.active() || current.selection_index != NO_SELECTION {
        return None;
    }
    let index = matching_destination(current.body(), body)?;
    current.selection_index = u16::from(index);
    EVENTS.push(RawFieldMapEvent {
        kind: EventKind::SelectionSubmitted,
        field_map: *current,
    })
}

pub(crate) fn observe_pane() -> Option<QueuedFieldMap> {
    if !current_mut().active() || pane_is_open() {
        return None;
    }
    close()
}

pub(crate) fn selection_packet(
    command: FieldMapSelectionCommand,
) -> Result<[u8; 9], CommandFailure> {
    let current = current_mut();
    if !current.active() || current.revision != command.revision || !pane_is_open() {
        return Err(CommandFailure::InvalidState);
    }
    if current.selection_index != NO_SELECTION {
        return Err(CommandFailure::Rejected);
    }
    let fields = destination_fields(current.body(), command.destination_index)
        .ok_or(CommandFailure::InvalidArguments)?;
    let mut packet = [0_u8; 9];
    packet[0] = 0x3F;
    packet[1..3].copy_from_slice(&fields.checksum.to_be_bytes());
    packet[3..5].copy_from_slice(&fields.map_id.to_be_bytes());
    packet[5..7].copy_from_slice(&fields.map_x.to_be_bytes());
    packet[7..9].copy_from_slice(&fields.map_y.to_be_bytes());
    Ok(packet)
}

pub(crate) fn copy_current(output: &mut RawFieldMap) {
    *output = *current_mut();
}

pub(crate) fn decode_current(raw: &RawFieldMap) -> Option<FieldMapState> {
    raw.active().then(|| decode(raw)).transpose().ok().flatten()
}

pub(crate) fn take(queued: QueuedFieldMap) -> Option<FieldMapUpdate> {
    let event = EVENTS.take(queued)?;
    let state = decode(&event.field_map).ok()?;
    Some(match event.kind {
        EventKind::Opened => FieldMapUpdate::Opened(state),
        EventKind::Changed => FieldMapUpdate::Changed(state),
        EventKind::SelectionSubmitted => FieldMapUpdate::SelectionSubmitted(state),
        EventKind::Closed => FieldMapUpdate::Closed { previous: state },
    })
}

pub(crate) fn release(queued: QueuedFieldMap) {
    EVENTS.release(queued);
}

fn close() -> Option<QueuedFieldMap> {
    let current = current_mut();
    if !current.active() {
        return None;
    }
    let previous = *current;
    *current = RawFieldMap::empty();
    EVENTS.push(RawFieldMapEvent {
        kind: EventKind::Closed,
        field_map: previous,
    })
}

fn current_mut() -> &'static mut RawFieldMap {
    // SAFETY: all callers run on the client main thread.
    unsafe { &mut *CURRENT.0.get() }
}

fn decode(raw: &RawFieldMap) -> Result<FieldMapState, ()> {
    let mut reader = Reader::new(raw.body());
    if reader.u8()? != 0x2E {
        return Err(());
    }
    let field_name = client_text::decode_or_empty(reader.string8()?).ok_or(())?;
    let count = reader.u8()?;
    let raw_current_node_index = reader.u8()?;
    let current_node_index = (raw_current_node_index < count).then_some(raw_current_node_index);
    let mut destinations = Vec::with_capacity(usize::from(count));
    for index in 0..count {
        destinations.push(FieldMapDestination {
            index,
            screen_x: reader.u16()?,
            screen_y: reader.u16()?,
            name: client_text::decode_or_empty(reader.string8()?).ok_or(())?,
            checksum: reader.u16()?,
            map_id: reader.u16()?,
            map_x: reader.u16()?,
            map_y: reader.u16()?,
        });
    }
    if !reader.empty() {
        return Err(());
    }
    let selection = u8::try_from(raw.selection_index)
        .ok()
        .map(|destination_index| FieldMapSelection { destination_index });
    Ok(FieldMapState {
        revision: raw.revision,
        field_name,
        current_node_index,
        destinations,
        selection,
    })
}

fn validate_server_packet(body: &[u8]) -> Option<()> {
    let mut reader = Reader::new(body);
    (reader.u8().ok()? == 0x2E).then_some(())?;
    reader.string8().ok()?;
    let count = reader.u8().ok()?;
    reader.u8().ok()?;
    for _ in 0..count {
        reader.u16().ok()?;
        reader.u16().ok()?;
        reader.string8().ok()?;
        for _ in 0..4 {
            reader.u16().ok()?;
        }
    }
    reader.empty().then_some(())
}

#[derive(Clone, Copy)]
struct DestinationFields {
    checksum: u16,
    map_id: u16,
    map_x: u16,
    map_y: u16,
}

fn destination_fields(body: &[u8], wanted: u8) -> Option<DestinationFields> {
    let mut reader = Reader::new(body);
    (reader.u8().ok()? == 0x2E).then_some(())?;
    reader.string8().ok()?;
    let count = reader.u8().ok()?;
    reader.u8().ok()?;
    if wanted >= count {
        return None;
    }
    for index in 0..count {
        reader.u16().ok()?;
        reader.u16().ok()?;
        reader.string8().ok()?;
        let fields = DestinationFields {
            checksum: reader.u16().ok()?,
            map_id: reader.u16().ok()?,
            map_x: reader.u16().ok()?,
            map_y: reader.u16().ok()?,
        };
        if index == wanted {
            return Some(fields);
        }
    }
    None
}

fn matching_destination(server: &[u8], outgoing: &[u8]) -> Option<u8> {
    let count = server
        .get(1)
        .and_then(|name_len| server.get(2 + usize::from(*name_len)))
        .copied()?;
    (0..count).find(|index| {
        destination_fields(server, *index).is_some_and(|fields| {
            outgoing[1..3] == fields.checksum.to_be_bytes()
                && outgoing[3..5] == fields.map_id.to_be_bytes()
                && outgoing[5..7] == fields.map_x.to_be_bytes()
                && outgoing[7..9] == fields.map_y.to_be_bytes()
        })
    })
}

#[cfg(all(windows, not(test)))]
fn pane_is_open() -> bool {
    crate::actions::field_map::is_open()
}

#[cfg(any(not(windows), test))]
fn pane_is_open() -> bool {
    true
}

struct Reader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn u8(&mut self) -> Result<u8, ()> {
        let value = *self.bytes.get(self.offset).ok_or(())?;
        self.offset += 1;
        Ok(value)
    }

    fn u16(&mut self) -> Result<u16, ()> {
        let bytes = self.take(2)?;
        Ok(u16::from_be_bytes([bytes[0], bytes[1]]))
    }

    fn string8(&mut self) -> Result<&'a [u8], ()> {
        let length = usize::from(self.u8()?);
        self.take(length)
    }

    fn take(&mut self, length: usize) -> Result<&'a [u8], ()> {
        let end = self.offset.checked_add(length).ok_or(())?;
        let value = self.bytes.get(self.offset..end).ok_or(())?;
        self.offset = end;
        Ok(value)
    }

    fn empty(&self) -> bool {
        self.offset == self.bytes.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static LOCK: Mutex<()> = Mutex::new(());

    fn packet() -> Vec<u8> {
        vec![
            0x2E, 8, b'f', b'i', b'e', b'l', b'd', b'0', b'0', b'1', 2, 1, 0, 10, 0, 20, 4, b'H',
            b'o', b'm', b'e', 0x12, 0x34, 0, 7, 0, 3, 0, 4, 0, 30, 0, 40, 4, b'M', b'i', b'n',
            b'e', 0x56, 0x78, 0, 9, 0, 5, 0, 6,
        ]
    }

    #[test]
    fn decodes_field_map_and_builds_canonical_selection() {
        let _guard = LOCK.lock().unwrap();
        reset();
        let queued = observe_server(&packet()).unwrap();
        let update = take(queued).unwrap();
        let FieldMapUpdate::Opened(state) = update else {
            panic!("expected opened field map");
        };
        assert_eq!(state.field_name, "field001");
        assert_eq!(state.current_node_index, Some(1));
        assert_eq!(state.destinations[0].name, "Home");
        assert_eq!(state.destinations[1].map_id, 9);
        let packet = selection_packet(FieldMapSelectionCommand {
            revision: state.revision,
            destination_index: 1,
        })
        .unwrap();
        assert_eq!(packet, [0x3F, 0x56, 0x78, 0, 9, 0, 5, 0, 6]);
    }

    #[test]
    fn submitted_selection_matches_the_full_destination_tuple() {
        let _guard = LOCK.lock().unwrap();
        reset();
        release(observe_server(&packet()).unwrap());
        assert!(observe_outgoing(&[0x3F, 0x56, 0x78, 0, 9, 0, 5, 0, 6]).is_some());
        assert!(observe_outgoing(&[0x3F, 0x56, 0x78, 0, 9, 0, 5, 0, 7]).is_none());
    }

    #[test]
    fn malformed_packets_do_not_become_active() {
        let _guard = LOCK.lock().unwrap();
        reset();
        assert!(observe_server(&[0x2E, 4, b'a']).is_none());
        assert!(decode_current(&RawFieldMap::empty()).is_none());
    }
}
