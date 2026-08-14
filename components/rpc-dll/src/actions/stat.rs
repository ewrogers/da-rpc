use super::network;
use darpc_protocol::{CharacterStat, CommandFailure};

pub(super) fn add(stat: CharacterStat) -> Result<(), CommandFailure> {
    if crate::state::current_stat_points().unwrap_or(0) == 0 {
        return Err(CommandFailure::InvalidArguments);
    }
    network::submit(&[0x47, stat.flag()])
}
