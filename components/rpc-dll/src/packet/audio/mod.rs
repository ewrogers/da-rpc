use crate::packet::ParseError;

const AUDIO_OPCODE: u8 = 0x19;
const MUSIC_MARKER: u8 = 0xFF;

pub(crate) use darpc_model::AudioUpdate;

pub(crate) fn update(body: &[u8]) -> Result<Option<AudioUpdate>, ParseError> {
    if body.first() != Some(&AUDIO_OPCODE) {
        return Ok(None);
    }
    let sound = *body.get(1).ok_or_else(|| ParseError::truncated(1, 1, 0))?;
    if sound != MUSIC_MARKER {
        return Ok(Some(AudioUpdate::SoundPlayed { effect: sound }));
    }
    let track = *body.get(2).ok_or_else(|| ParseError::truncated(2, 1, 0))?;
    Ok(Some(if track == 0 {
        AudioUpdate::MusicStopped
    } else {
        AudioUpdate::MusicStarted { track }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_sound_and_music_updates() {
        assert_eq!(
            update(&[AUDIO_OPCODE, 12]).unwrap(),
            Some(AudioUpdate::SoundPlayed { effect: 12 })
        );
        assert_eq!(
            update(&[AUDIO_OPCODE, MUSIC_MARKER, 4]).unwrap(),
            Some(AudioUpdate::MusicStarted { track: 4 })
        );
        assert_eq!(
            update(&[AUDIO_OPCODE, MUSIC_MARKER, 0]).unwrap(),
            Some(AudioUpdate::MusicStopped)
        );
    }

    #[test]
    fn rejects_truncated_audio_updates() {
        assert_eq!(update(&[AUDIO_OPCODE]).unwrap_err().offset(), 1);
        assert_eq!(
            update(&[AUDIO_OPCODE, MUSIC_MARKER]).unwrap_err().offset(),
            2
        );
    }
}
