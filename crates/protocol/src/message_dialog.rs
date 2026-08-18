use darpc_model::{MessageDialog, MessageDialogsState};

use crate::{
    DecodeError, EncodeError,
    message::{PayloadReader, push_bool, push_u32},
    snapshot::{decode_optional_string, encode_optional_string},
};

pub const MAX_MESSAGE_DIALOGS: usize = 8;
pub const MAX_MESSAGE_DIALOG_TEXT_LEN: usize = 4 * 1024;

pub(crate) fn encode_state(
    output: &mut Vec<u8>,
    state: &MessageDialogsState,
) -> Result<(), EncodeError> {
    if state.dialogs.len() > MAX_MESSAGE_DIALOGS {
        return Err(EncodeError::SnapshotCollectionTooLong {
            length: state.dialogs.len(),
            max: MAX_MESSAGE_DIALOGS,
        });
    }

    push_u32(output, state.revision);
    output.push(state.dialogs.len() as u8);
    for dialog in &state.dialogs {
        push_u32(output, dialog.id);
        encode_optional_string(output, dialog.text.as_deref(), MAX_MESSAGE_DIALOG_TEXT_LEN)?;
        push_bool(output, dialog.truncated);
    }
    Ok(())
}

pub(crate) fn decode_state(
    reader: &mut PayloadReader<'_>,
) -> Result<MessageDialogsState, DecodeError> {
    let revision = reader.read_u32()?;
    let count = usize::from(reader.read_u8()?);
    if count > MAX_MESSAGE_DIALOGS {
        return Err(DecodeError::SnapshotCollectionTooLong {
            length: count,
            max: MAX_MESSAGE_DIALOGS,
        });
    }

    let mut dialogs = Vec::with_capacity(count);
    for _ in 0..count {
        dialogs.push(MessageDialog {
            id: reader.read_u32()?,
            text: decode_optional_string(reader, MAX_MESSAGE_DIALOG_TEXT_LEN)?,
            truncated: reader.read_bool()?,
        });
    }
    Ok(MessageDialogsState { revision, dialogs })
}
