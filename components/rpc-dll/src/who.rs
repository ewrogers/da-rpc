use darpc_model::{CharacterClass, UserState, WhoList, WhoPlayer};
#[cfg(not(test))]
use darpc_protocol::CommandFailure;
use darpc_protocol::{MAX_WHO_NAME_LEN, MAX_WHO_PLAYERS, MAX_WHO_TITLE_LEN};
use std::{
    cell::UnsafeCell,
    sync::atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicUsize, Ordering},
};

#[cfg(not(test))]
const WHO_REQUEST_OPCODE: u8 = 0x18;
const WHO_RESPONSE_OPCODE: u8 = 0x36;
const RESPONSE_CAPACITY: usize = u16::MAX as usize;
const ORIGIN_CAPACITY: usize = 16;
const ORIGIN_TTL_MS: u32 = 3_000;
const ORIGIN_PLAYER: u8 = 1;
const ORIGIN_DARPC: u8 = 2;

static SUBMITTING: AtomicBool = AtomicBool::new(false);
static SUBMIT_COMMAND_ID: AtomicU32 = AtomicU32::new(0);
static SUBMIT_OBSERVED: AtomicBool = AtomicBool::new(false);
static ORIGIN_HEAD: AtomicUsize = AtomicUsize::new(0);
static ORIGIN_TAIL: AtomicUsize = AtomicUsize::new(0);
static ORIGIN_KINDS: [AtomicU8; ORIGIN_CAPACITY] = [const { AtomicU8::new(0) }; ORIGIN_CAPACITY];
static ORIGIN_COMMANDS: [AtomicU32; ORIGIN_CAPACITY] =
    [const { AtomicU32::new(0) }; ORIGIN_CAPACITY];
static ORIGIN_TICKS: [AtomicU32; ORIGIN_CAPACITY] = [const { AtomicU32::new(0) }; ORIGIN_CAPACITY];
static RESPONSE_SEQUENCE: AtomicU32 = AtomicU32::new(0);
static RESPONSE_COMMAND_ID: AtomicU32 = AtomicU32::new(0);
static RESPONSE_LENGTH: AtomicUsize = AtomicUsize::new(0);
static RESPONSE: ResponseCell = ResponseCell(UnsafeCell::new([0; RESPONSE_CAPACITY]));

struct ResponseCell(UnsafeCell<[u8; RESPONSE_CAPACITY]>);

// SAFETY: the game thread is the sole writer. Readers use RESPONSE_SEQUENCE as
// a seqlock and copy bytes only from a stable, fully published response.
unsafe impl Sync for ResponseCell {}

pub(crate) fn reset() {
    SUBMITTING.store(false, Ordering::Release);
    SUBMIT_COMMAND_ID.store(0, Ordering::Release);
    SUBMIT_OBSERVED.store(false, Ordering::Release);
    ORIGIN_HEAD.store(0, Ordering::Release);
    ORIGIN_TAIL.store(0, Ordering::Release);
    RESPONSE_COMMAND_ID.store(0, Ordering::Release);
    RESPONSE_LENGTH.store(0, Ordering::Release);
    RESPONSE_SEQUENCE.store(0, Ordering::Release);
    for kind in &ORIGIN_KINDS {
        kind.store(0, Ordering::Release);
    }
}

#[cfg(not(test))]
pub(crate) fn request(command_id: u32) -> Result<(), CommandFailure> {
    SUBMIT_COMMAND_ID.store(command_id, Ordering::Release);
    SUBMIT_OBSERVED.store(false, Ordering::Release);
    SUBMITTING.store(true, Ordering::Release);
    let result = crate::actions::network::submit(&[WHO_REQUEST_OPCODE]);
    SUBMITTING.store(false, Ordering::Release);
    SUBMIT_COMMAND_ID.store(0, Ordering::Release);
    result?;
    if !SUBMIT_OBSERVED.load(Ordering::Acquire) {
        return Err(CommandFailure::Internal);
    }
    Ok(())
}

pub(crate) fn observe_request(tick_ms: u32) {
    let command_id = SUBMIT_COMMAND_ID.load(Ordering::Acquire);
    let is_darpc = SUBMITTING.load(Ordering::Acquire) && command_id != 0;
    push_origin(
        if is_darpc {
            ORIGIN_DARPC
        } else {
            ORIGIN_PLAYER
        },
        command_id,
        tick_ms,
    );
    if is_darpc {
        SUBMIT_OBSERVED.store(true, Ordering::Release);
    }
}

#[must_use]
pub(crate) fn intercept_response(body: &[u8], tick_ms: u32) -> bool {
    if body.first() != Some(&WHO_RESPONSE_OPCODE) {
        return false;
    }
    let Some((kind, command_id)) = pop_origin(tick_ms) else {
        return false;
    };
    if kind != ORIGIN_DARPC {
        return false;
    }
    if validate_body(body).is_err() {
        crate::commands::fail_who(command_id);
        return true;
    }
    publish(command_id, body);
    crate::commands::complete_who(command_id);
    true
}

pub(crate) fn result(command_id: u32) -> Option<WhoList> {
    loop {
        let before = RESPONSE_SEQUENCE.load(Ordering::Acquire);
        if before & 1 != 0 || RESPONSE_COMMAND_ID.load(Ordering::Relaxed) != command_id {
            return None;
        }
        let length = RESPONSE_LENGTH.load(Ordering::Relaxed);
        if length == 0 || length > RESPONSE_CAPACITY {
            return None;
        }
        let mut body = vec![0; length];
        // SAFETY: the seqlock value is even. We validate it again after this
        // bounded copy before parsing or returning the bytes.
        unsafe {
            body.copy_from_slice(&(&*RESPONSE.0.get())[..length]);
        }
        let after = RESPONSE_SEQUENCE.load(Ordering::Acquire);
        if before == after {
            return parse_body(&body).ok();
        }
    }
}

fn publish(command_id: u32, body: &[u8]) {
    RESPONSE_SEQUENCE.fetch_add(1, Ordering::AcqRel);
    // SAFETY: only the game thread publishes responses, and the odd seqlock
    // prevents readers from consuming this buffer during the copy.
    unsafe {
        (&mut *RESPONSE.0.get())[..body.len()].copy_from_slice(body);
    }
    RESPONSE_LENGTH.store(body.len(), Ordering::Relaxed);
    RESPONSE_COMMAND_ID.store(command_id, Ordering::Relaxed);
    RESPONSE_SEQUENCE.fetch_add(1, Ordering::Release);
}

fn push_origin(kind: u8, command_id: u32, tick_ms: u32) {
    let tail = ORIGIN_TAIL.load(Ordering::Relaxed);
    let head = ORIGIN_HEAD.load(Ordering::Acquire);
    if tail.wrapping_sub(head) >= ORIGIN_CAPACITY {
        ORIGIN_HEAD.store(head.wrapping_add(1), Ordering::Release);
    }
    let index = tail % ORIGIN_CAPACITY;
    ORIGIN_COMMANDS[index].store(command_id, Ordering::Relaxed);
    ORIGIN_TICKS[index].store(tick_ms, Ordering::Relaxed);
    ORIGIN_KINDS[index].store(kind, Ordering::Relaxed);
    ORIGIN_TAIL.store(tail.wrapping_add(1), Ordering::Release);
}

fn pop_origin(now: u32) -> Option<(u8, u32)> {
    loop {
        let head = ORIGIN_HEAD.load(Ordering::Relaxed);
        if head == ORIGIN_TAIL.load(Ordering::Acquire) {
            return None;
        }
        let index = head % ORIGIN_CAPACITY;
        let kind = ORIGIN_KINDS[index].load(Ordering::Relaxed);
        let command_id = ORIGIN_COMMANDS[index].load(Ordering::Relaxed);
        let tick_ms = ORIGIN_TICKS[index].load(Ordering::Relaxed);
        ORIGIN_HEAD.store(head.wrapping_add(1), Ordering::Release);
        if now.wrapping_sub(tick_ms) <= ORIGIN_TTL_MS {
            return Some((kind, command_id));
        }
    }
}

fn validate_body(body: &[u8]) -> Result<(), ()> {
    parse_rows(body, |_, _, _, _, _, _| Ok(()))?;
    Ok(())
}

fn parse_body(body: &[u8]) -> Result<WhoList, ()> {
    let world_count = read_u16(body, 1)?;
    let country_count = read_u16(body, 3)?;
    let mut players = Vec::with_capacity(usize::from(country_count).min(MAX_WHO_PLAYERS));
    parse_rows(
        body,
        |class_and_flags, color, state, title, is_master, name| {
            let title = decode_text(title)?;
            let name = decode_text(name)?;
            players.push(WhoPlayer {
                name,
                title,
                class: CharacterClass::from_raw(class_and_flags & 0x07),
                state: UserState::from_raw(state),
                color,
                is_master: is_master != 0,
                is_guildmate: class_and_flags & 0x08 != 0,
            });
            Ok(())
        },
    )?;
    Ok(WhoList {
        world_count,
        country_count,
        players,
    })
}

fn parse_rows(
    body: &[u8],
    mut row: impl FnMut(u8, u8, u8, &[u8], u8, &[u8]) -> Result<(), ()>,
) -> Result<(), ()> {
    if body.first() != Some(&WHO_RESPONSE_OPCODE) || body.len() < 5 {
        return Err(());
    }
    let count = usize::from(read_u16(body, 3)?);
    if count > MAX_WHO_PLAYERS {
        return Err(());
    }
    let mut offset = 5;
    for _ in 0..count {
        let class_and_flags = take_u8(body, &mut offset)?;
        let color = take_u8(body, &mut offset)?;
        let state = take_u8(body, &mut offset)?;
        let title = take_string(body, &mut offset, MAX_WHO_TITLE_LEN)?;
        let is_master = take_u8(body, &mut offset)?;
        let name = take_string(body, &mut offset, MAX_WHO_NAME_LEN)?;
        row(class_and_flags, color, state, title, is_master, name)?;
    }
    (offset == body.len()).then_some(()).ok_or(())
}

fn decode_text(bytes: &[u8]) -> Result<String, ()> {
    if bytes.is_empty() {
        Ok(String::new())
    } else {
        crate::client_text::decode(bytes).ok_or(())
    }
}

fn read_u16(body: &[u8], offset: usize) -> Result<u16, ()> {
    let bytes: [u8; 2] = body
        .get(offset..offset + 2)
        .ok_or(())?
        .try_into()
        .map_err(|_| ())?;
    Ok(u16::from_be_bytes(bytes))
}

fn take_u8(body: &[u8], offset: &mut usize) -> Result<u8, ()> {
    let value = *body.get(*offset).ok_or(())?;
    *offset += 1;
    Ok(value)
}

fn take_string<'a>(body: &'a [u8], offset: &mut usize, max: usize) -> Result<&'a [u8], ()> {
    let length = usize::from(take_u8(body, offset)?);
    if length > max {
        return Err(());
    }
    let value = body
        .get(*offset..offset.checked_add(length).ok_or(())?)
        .ok_or(())?;
    *offset += length;
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    const RESPONSE: [u8; 35] = [
        0x36, 0x00, 0x64, 0x00, 0x02, 0x0c, 0x03, 0x03, 0x07, b'A', b'i', b's', b'l', b'i', b'n',
        b'g', 0x01, 0x04, b'Z', b'i', b'L', b'o', 0x02, 0x00, 0x00, 0x00, 0x00, 0x07, b'E', b'i',
        b'd', b'o', b'l', b'o', b'n',
    ];

    #[test]
    fn parses_server_order_and_flags() {
        let list = parse_body(&RESPONSE).unwrap();
        assert_eq!(list.world_count, 100);
        assert_eq!(list.country_count, 2);
        assert_eq!(list.players[0].name, "ZiLo");
        assert_eq!(list.players[0].class, CharacterClass::Priest);
        assert!(list.players[0].is_guildmate);
        assert!(list.players[0].is_master);
        assert_eq!(list.players[1].name, "Eidolon");
        assert_eq!(list.players[1].class, CharacterClass::Rogue);
    }
}
