use crate::registry::ClientIdentity;
use darpc_protocol::{CommandOperation, CommandResult};
use tokio::sync::oneshot;

pub(crate) const ROUTER_CAPACITY: usize = 64;
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) const WORKER_CAPACITY: usize = 16;

pub(crate) struct CommandCall {
    pub(crate) pid: u32,
    pub(crate) identity: ClientIdentity,
    pub(crate) operation: CommandOperation,
    pub(crate) reply: oneshot::Sender<CommandReply>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) enum CommandReply {
    Result(CommandResult),
    Busy,
    Unavailable,
}
