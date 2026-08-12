use super::*;

#[derive(Clone, Debug, Serialize, ToSchema)]
/// An ability entered or restarted cooldown.
pub(crate) struct CooldownStarted {
    pub(super) observation: EventObservation,
    pub(super) slot: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) remaining_ms: Option<u32>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
/// An ability left cooldown and is ready to use.
pub(crate) struct AbilityReady {
    pub(super) observation: EventObservation,
    pub(super) slot: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) name: Option<String>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
/// A skill packet was submitted by the client.
pub(crate) struct SkillUsed {
    pub(super) observation: EventObservation,
    pub(super) slot: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) name: Option<String>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
/// A positive-line spell entered the native delayed-cast sequence.
pub(crate) struct SpellBegin {
    pub(super) observation: EventObservation,
    pub(super) slot: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) name: Option<String>,
    pub(super) total_lines: u8,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
/// One visible chant line was submitted during a delayed spell cast.
pub(crate) struct SpellChant {
    pub(super) observation: EventObservation,
    pub(super) slot: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) name: Option<String>,
    pub(super) line: u8,
    pub(super) total_lines: u8,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
/// The final spell packet was submitted by the client.
pub(crate) struct SpellCast {
    pub(super) observation: EventObservation,
    pub(super) slot: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) arguments: Option<SpellCastArguments>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum SpellCastArguments {
    Unknown,
    Target {
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<u32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        x: i32,
        y: i32,
    },
    Input {
        value: String,
    },
    Values {
        values: Vec<u16>,
    },
}

#[derive(Clone, Debug, Serialize, ToSchema)]
/// A positive-line cast ended before its final spell packet was submitted.
pub(crate) struct SpellCancelled {
    pub(super) observation: EventObservation,
    pub(super) slot: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) name: Option<String>,
    pub(super) source: SpellCancellationSource,
}

#[derive(Clone, Copy, Debug, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SpellCancellationSource {
    Client,
    Server,
    Replaced,
}

pub(super) fn spell_arguments(
    arguments: darpc_model::SpellCastArguments,
    target_name: Option<String>,
) -> Option<SpellCastArguments> {
    match arguments {
        darpc_model::SpellCastArguments::Unknown => Some(SpellCastArguments::Unknown),
        darpc_model::SpellCastArguments::None => None,
        darpc_model::SpellCastArguments::Target { id, x, y } => Some(SpellCastArguments::Target {
            id,
            name: target_name,
            x,
            y,
        }),
        darpc_model::SpellCastArguments::Input(value) => Some(SpellCastArguments::Input { value }),
        darpc_model::SpellCastArguments::Values(values) => {
            Some(SpellCastArguments::Values { values })
        }
    }
}
