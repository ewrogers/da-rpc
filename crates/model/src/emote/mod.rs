/// A confirmed client emote name and its outgoing request code.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NamedEmote {
    pub name: &'static str,
    pub code: u8,
}

/// Emotes whose player-facing names have been confirmed.
pub const NAMED_EMOTES: &[NamedEmote] = &[
    NamedEmote {
        name: "smile",
        code: 0,
    },
    NamedEmote {
        name: "cry",
        code: 1,
    },
    NamedEmote {
        name: "sad",
        code: 2,
    },
    NamedEmote {
        name: "wink",
        code: 3,
    },
    NamedEmote {
        name: "stunned",
        code: 4,
    },
    NamedEmote {
        name: "raz",
        code: 5,
    },
    NamedEmote {
        name: "surprise",
        code: 6,
    },
    NamedEmote {
        name: "sleepy",
        code: 7,
    },
    NamedEmote {
        name: "yawn",
        code: 8,
    },
    NamedEmote {
        name: "kiss",
        code: 12,
    },
    NamedEmote {
        name: "wave",
        code: 13,
    },
    NamedEmote {
        name: "rock",
        code: 25,
    },
    NamedEmote {
        name: "scissors",
        code: 26,
    },
    NamedEmote {
        name: "paper",
        code: 27,
    },
    NamedEmote {
        name: "oof",
        code: 28,
    },
    NamedEmote {
        name: "speechless",
        code: 29,
    },
    NamedEmote {
        name: "blue",
        code: 30,
    },
    NamedEmote {
        name: "blush",
        code: 31,
    },
    NamedEmote {
        name: "heart",
        code: 32,
    },
    NamedEmote {
        name: "sweat",
        code: 33,
    },
    NamedEmote {
        name: "sing",
        code: 34,
    },
    NamedEmote {
        name: "ack",
        code: 35,
    },
];

pub fn emote_code(name: &str) -> Option<u8> {
    NAMED_EMOTES
        .iter()
        .find(|emote| emote.name.eq_ignore_ascii_case(name))
        .map(|emote| emote.code)
}

pub fn is_client_emote_code(code: u8) -> bool {
    (0..=8).contains(&code) || (12..=35).contains(&code)
}

#[cfg(test)]
mod tests {
    use super::{NAMED_EMOTES, emote_code, is_client_emote_code};

    #[test]
    fn resolves_confirmed_names_without_case_sensitivity() {
        for emote in NAMED_EMOTES {
            assert_eq!(emote_code(emote.name), Some(emote.code));
            assert_eq!(
                emote_code(&emote.name.to_ascii_uppercase()),
                Some(emote.code)
            );
            assert!(is_client_emote_code(emote.code));
        }
        assert_eq!(emote_code("unknown"), None);
    }

    #[test]
    fn accepts_the_complete_client_ui_code_set() {
        for code in 0..=35 {
            assert_eq!(is_client_emote_code(code), !matches!(code, 9..=11));
        }
    }
}
