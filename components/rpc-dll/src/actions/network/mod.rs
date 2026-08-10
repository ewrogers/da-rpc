use super::{module_base, read};
use darpc_game_client::{
    CLIENT_PACKET_SUBMIT_RVA, CLIENT_SOCKET_POINTER_RVA, EVENT_DISPATCH_RVA,
    EVENT_DISPATCHER_POINTER_RVA,
};
use darpc_protocol::{CommandFailure, MAX_RAW_PACKET_PAYLOAD_LEN, RawPacket, RawPacketDirection};
use std::ffi::c_void;

type SubmitFn = unsafe extern "thiscall" fn(*mut c_void, *const u8, i16) -> u32;
type DispatchFn = unsafe extern "thiscall" fn(*mut c_void, *const ServerEvent) -> bool;

const SERVER_EVENT_TYPE: u8 = 0x13;

#[repr(C)]
struct ServerEvent {
    prefix: [u8; 0x0c],
    event_type: u8,
    padding: [u8; 7],
    body: *const u8,
    body_length: u32,
}

const _: () = {
    assert!(std::mem::size_of::<ServerEvent>() == 0x1c);
    assert!(std::mem::offset_of!(ServerEvent, event_type) == 0x0c);
    assert!(std::mem::offset_of!(ServerEvent, body) == 0x14);
    assert!(std::mem::offset_of!(ServerEvent, body_length) == 0x18);
};

pub(crate) fn raw(packet: RawPacket) -> Result<(), CommandFailure> {
    let mut body = [0; MAX_RAW_PACKET_PAYLOAD_LEN + 1];
    body[0] = packet.command();
    body[1..1 + packet.payload().len()].copy_from_slice(packet.payload());
    let body = &body[..packet.payload().len() + 1];
    match packet.direction() {
        RawPacketDirection::Client => submit(body),
        RawPacketDirection::Server => dispatch(body),
    }
}

pub(crate) fn submit(body: &[u8]) -> Result<(), CommandFailure> {
    let module_base = module_base()?;
    let socket = read::<u32>(
        module_base
            .checked_add(CLIENT_SOCKET_POINTER_RVA)
            .ok_or(CommandFailure::Internal)?,
    )
    .filter(|pointer| *pointer != 0)
    .ok_or(CommandFailure::InvalidState)?;
    let length = i16::try_from(body.len()).map_err(|_| CommandFailure::InvalidArguments)?;
    let address = module_base
        .checked_add(CLIENT_PACKET_SUBMIT_RVA)
        .ok_or(CommandFailure::Internal)?;
    // SAFETY: client validation fixes the RVA and ABI. The socket is resolved
    // from the live client immediately before this main-thread call. The
    // native function copies the complete body before returning.
    unsafe {
        let submit: SubmitFn = std::mem::transmute(address);
        submit(socket as *mut c_void, body.as_ptr(), length);
    }
    Ok(())
}

fn dispatch(body: &[u8]) -> Result<(), CommandFailure> {
    let module_base = module_base()?;
    let dispatcher = read::<u32>(
        module_base
            .checked_add(EVENT_DISPATCHER_POINTER_RVA)
            .ok_or(CommandFailure::Internal)?,
    )
    .filter(|pointer| *pointer != 0)
    .ok_or(CommandFailure::InvalidState)?;
    let address = module_base
        .checked_add(EVENT_DISPATCH_RVA)
        .ok_or(CommandFailure::Internal)?;
    let body_length = u32::try_from(body.len()).map_err(|_| CommandFailure::InvalidArguments)?;
    let event = ServerEvent {
        prefix: [0; 0x0c],
        event_type: SERVER_EVENT_TYPE,
        padding: [0; 7],
        body: body.as_ptr(),
        body_length,
    };
    // SAFETY: client validation fixes the RVA, ABI, and server-event layout.
    // The dispatcher and body are resolved immediately before this main-thread
    // call, and both stack-backed values remain alive until it returns.
    unsafe {
        let dispatch: DispatchFn = std::mem::transmute(address);
        dispatch(dispatcher as *mut c_void, &event);
    }
    Ok(())
}
