use crate::{
    DecodeError, EncodeError,
    message::{PayloadReader, push_u32},
};
use darpc_model::ActionSource;

pub(crate) fn encode(output: &mut Vec<u8>, source: ActionSource) -> Result<(), EncodeError> {
    match source {
        ActionSource::Unknown => output.push(0),
        ActionSource::Client => output.push(1),
        ActionSource::Command { command_id } => {
            if command_id == 0 {
                return Err(EncodeError::InvalidCommandId);
            }
            output.push(2);
            push_u32(output, command_id);
        }
    }
    Ok(())
}

pub(crate) fn decode(reader: &mut PayloadReader<'_>) -> Result<ActionSource, DecodeError> {
    match reader.read_u8()? {
        0 => Ok(ActionSource::Unknown),
        1 => Ok(ActionSource::Client),
        2 => {
            let command_id = reader.read_u32()?;
            if command_id == 0 {
                return Err(DecodeError::InvalidCommandId);
            }
            Ok(ActionSource::Command { command_id })
        }
        actual => Err(DecodeError::InvalidActionSource { actual }),
    }
}
