use super::{module_base, read};
use darpc_game_client::{CLIENT_PACKET_SUBMIT_RVA, CLIENT_SOCKET_POINTER_RVA};
use darpc_protocol::CommandFailure;
use std::ffi::c_void;

type SubmitFn = unsafe extern "thiscall" fn(*mut c_void, *const u8, i16) -> u32;

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
