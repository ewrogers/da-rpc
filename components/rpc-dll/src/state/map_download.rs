//! Bounded game-client map-download lifecycle tracking.
//!
//! `SMapSize` stages the only map identity available for the native transfer.
//! A matching outbound `CMapRequest` starts the download, and accepted
//! `SMapPart` rows complete it only after every prepared row has been observed.

use super::{QueuedStateUpdate, push_event};
use darpc_model::{MapDownload, MapDownloadUpdate};
use std::cell::UnsafeCell;

const MAP_REQUEST_OPCODE: u8 = 0x05;
const MAP_REQUEST_LENGTH: usize = 10;
const MAP_PART_HEADER_LENGTH: usize = 3;
const MAP_CELL_LENGTH: usize = 6;
const ROW_WORDS: usize = 4;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Candidate {
    download: MapDownload,
    requested: bool,
    received_rows: [u64; ROW_WORDS],
    received_count: u16,
}

impl Candidate {
    fn new(map_id: u32, width: i32, height: i32) -> Option<Self> {
        let width = u8::try_from(width).ok().filter(|width| *width != 0)?;
        let height = u8::try_from(height).ok().filter(|height| *height != 0)?;
        Some(Self {
            download: MapDownload {
                map_id,
                width,
                height,
            },
            requested: false,
            received_rows: [0; ROW_WORDS],
            received_count: 0,
        })
    }

    fn matches_request(&self, body: &[u8]) -> bool {
        body.len() == MAP_REQUEST_LENGTH
            && body[0] == MAP_REQUEST_OPCODE
            && body[1..5] == [0; 4]
            && body[5] == self.download.width
            && body[6] == self.download.height
    }

    fn begin(&mut self) -> MapDownloadUpdate {
        self.requested = true;
        self.received_rows = [0; ROW_WORDS];
        self.received_count = 0;
        MapDownloadUpdate::Requested(self.download)
    }

    fn observe_part(&mut self, row_index: u16, body_length: usize) -> Option<bool> {
        if !self.requested || row_index >= u16::from(self.download.height) {
            return None;
        }
        let expected_length = MAP_PART_HEADER_LENGTH
            .checked_add(usize::from(self.download.width).checked_mul(MAP_CELL_LENGTH)?)?;
        if body_length != expected_length {
            return None;
        }

        let word = usize::from(row_index / 64);
        let mask = 1_u64 << (row_index % 64);
        if self.received_rows[word] & mask == 0 {
            self.received_rows[word] |= mask;
            self.received_count += 1;
        }
        let is_final = row_index + 1 == u16::from(self.download.height);
        is_final.then_some(self.received_count == u16::from(self.download.height))
    }
}

#[derive(Default)]
struct Tracker {
    candidate: Option<Candidate>,
}

impl Tracker {
    fn stage(&mut self, map_id: u32, width: i32, height: i32) {
        self.candidate = Candidate::new(map_id, width, height);
    }

    fn observe_request(&mut self, body: &[u8]) -> Option<MapDownloadUpdate> {
        let candidate = self.candidate.as_mut()?;
        candidate.matches_request(body).then(|| candidate.begin())
    }

    fn finish_stage(&mut self) {
        if self
            .candidate
            .as_ref()
            .is_some_and(|candidate| !candidate.requested)
        {
            self.candidate = None;
        }
    }

    fn observe_part(&mut self, row_index: u16, body_length: usize) -> Option<MapDownloadUpdate> {
        let candidate = self.candidate.as_mut()?;
        let completed = candidate.observe_part(row_index, body_length)?;
        let download = candidate.download;
        self.candidate = None;
        completed.then_some(MapDownloadUpdate::Downloaded(download))
    }
}

struct MainThreadTracker(UnsafeCell<Tracker>);

// SAFETY: map-size, outgoing-packet, and server-event observations all run on
// the validated client main thread. Tests exercise `Tracker` directly.
unsafe impl Sync for MainThreadTracker {}

static TRACKER: MainThreadTracker = MainThreadTracker(UnsafeCell::new(Tracker { candidate: None }));

pub(super) fn stage(map_id: u32, width: i32, height: i32) {
    // SAFETY: the map-size hook runs on the client main thread.
    unsafe { (&mut *TRACKER.0.get()).stage(map_id, width, height) };
}

pub(super) fn observe_request(body: &[u8], tick_ms: u32) {
    // SAFETY: outgoing observation runs on the client main thread after the
    // native submit routine accepts the packet.
    let update = unsafe { (&mut *TRACKER.0.get()).observe_request(body) };
    if let Some(update) = update {
        push_event(QueuedStateUpdate::MapDownload(update), tick_ms);
    }
}

pub(super) fn finish_stage() {
    // SAFETY: this runs synchronously on the client main thread after the
    // native map-size handler returns. A nested request has already been seen.
    unsafe { (&mut *TRACKER.0.get()).finish_stage() };
}

pub(super) fn observe_part(row_index: u16, body_length: usize, tick_ms: u32) {
    // SAFETY: server-event observation runs on the client main thread after
    // the native map-part handler returns from its commit path.
    let update = unsafe { (&mut *TRACKER.0.get()).observe_part(row_index, body_length) };
    if let Some(update) = update {
        push_event(QueuedStateUpdate::MapDownload(update), tick_ms);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(width: u8, height: u8) -> [u8; MAP_REQUEST_LENGTH] {
        [0x05, 0, 0, 0, 0, width, height, 0, 0x12, 0x34]
    }

    fn part_length(width: u8) -> usize {
        MAP_PART_HEADER_LENGTH + usize::from(width) * MAP_CELL_LENGTH
    }

    #[test]
    fn request_requires_the_staged_dimensions() {
        let mut tracker = Tracker::default();
        tracker.stage(3001, 100, 80);

        assert_eq!(tracker.observe_request(&request(99, 80)), None);
        assert_eq!(
            tracker.observe_request(&request(100, 80)),
            Some(MapDownloadUpdate::Requested(MapDownload {
                map_id: 3001,
                width: 100,
                height: 80,
            }))
        );
    }

    #[test]
    fn cache_hits_clear_the_candidate_while_requests_keep_it() {
        let mut tracker = Tracker::default();
        tracker.stage(3001, 100, 80);
        tracker.finish_stage();
        assert!(tracker.candidate.is_none());

        tracker.stage(3001, 100, 80);
        tracker.observe_request(&request(100, 80)).unwrap();
        tracker.finish_stage();
        assert!(tracker.candidate.is_some());
    }

    #[test]
    fn final_row_completes_only_after_every_row() {
        let mut tracker = Tracker::default();
        tracker.stage(498, 20, 3);
        tracker.observe_request(&request(20, 3)).unwrap();

        assert_eq!(tracker.observe_part(0, part_length(20)), None);
        assert_eq!(tracker.observe_part(2, part_length(20)), None);
        assert!(tracker.candidate.is_none());

        tracker.stage(498, 20, 3);
        tracker.observe_request(&request(20, 3)).unwrap();
        assert_eq!(tracker.observe_part(1, part_length(20)), None);
        assert_eq!(tracker.observe_part(0, part_length(20)), None);
        assert_eq!(
            tracker.observe_part(2, part_length(20)),
            Some(MapDownloadUpdate::Downloaded(MapDownload {
                map_id: 498,
                width: 20,
                height: 3,
            }))
        );
    }

    #[test]
    fn malformed_rows_do_not_advance_the_download() {
        let mut tracker = Tracker::default();
        tracker.stage(498, 20, 2);
        tracker.observe_request(&request(20, 2)).unwrap();

        assert_eq!(tracker.observe_part(0, part_length(20) - 1), None);
        assert_eq!(tracker.observe_part(1, part_length(20)), None);
    }
}
