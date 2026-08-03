use super::*;

#[derive(Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct SkillSlotOptions {
    slot: u8,
}

#[derive(Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct SkillNameOptions {
    name: String,
}

#[derive(Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(untagged)]
pub(crate) enum UseSkillOptions {
    Slot(SkillSlotOptions),
    Name(SkillNameOptions),
}

#[derive(Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(untagged)]
pub(crate) enum SpellTargetOptions {
    Name(String),
    Id(u32),
    Tile(Destination),
}

#[derive(Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct CastSpellBySlot {
    slot: u8,
    #[serde(default)]
    target: Option<SpellTargetOptions>,
    #[serde(default)]
    input: Option<String>,
}

#[derive(Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct CastSpellByName {
    name: String,
    #[serde(default)]
    target: Option<SpellTargetOptions>,
    #[serde(default)]
    input: Option<String>,
}

#[derive(Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(untagged)]
pub(crate) enum CastSpellOptions {
    Slot(CastSpellBySlot),
    Name(CastSpellByName),
}

#[utoipa::path(
    post,
    path = "/clients/{client}/skills/use",
    params(("client" = String, Path, description = "Process ID or current in-game character name")),
    request_body(
        content = UseSkillOptions,
        example = json!({"name": "Assail"})
    ),
    responses(
        (status = 200, description = "The normal client skill activation routine completed or reported a local rejection", body = CommandStatus),
        (status = 202, description = "The skill command was accepted and remains pending", body = CommandStatus),
        (status = 400, description = "The selector body, slot, or name was invalid", body = crate::api::ErrorState),
        (status = 404, description = "The client or selected learned skill was not found", body = crate::api::ErrorState),
        (status = 409, description = "The client is not in game or its skillbook is unavailable", body = crate::api::ErrorState),
        (status = 429, description = "A bounded command queue is full", body = crate::api::ErrorState),
        (status = 503, description = "The client command path is unavailable", body = crate::api::ErrorState),
        (status = 504, description = "The daemon command route timed out", body = crate::api::ErrorState)
    )
)]
pub(crate) async fn use_skill(
    State(state): State<ApiState>,
    Path(identifier): Path<String>,
    request: Result<Json<UseSkillOptions>, JsonRejection>,
) -> Result<(StatusCode, Json<CommandStatus>), ApiError> {
    let Json(request) = action_request(request)?;
    let (pid, identity, snapshot) = action_client(&state, &identifier)?;
    let slot = resolve_skill(pid, &snapshot, request)?;
    submit_action(&state, pid, identity, ProtocolKind::UseSkill(slot)).await
}

#[utoipa::path(
    post,
    path = "/clients/{client}/spells/cast",
    params(("client" = String, Path, description = "Process ID or current in-game character name")),
    request_body(
        content = CastSpellOptions,
        example = json!({"name": "Beag Ioc", "target": "Eidolon"})
    ),
    responses(
        (status = 200, description = "The native spell cast was started or submitted immediately", body = CommandStatus),
        (status = 202, description = "The spell command was accepted and remains pending", body = CommandStatus),
        (status = 400, description = "The spell selector, argument shape, input, or tile was invalid", body = crate::api::ErrorState),
        (status = 404, description = "The client, learned spell, or requested target was not found", body = crate::api::ErrorState),
        (status = 409, description = "The client is not in game or required state is unavailable", body = crate::api::ErrorState),
        (status = 429, description = "A bounded command queue is full", body = crate::api::ErrorState),
        (status = 503, description = "The client command path is unavailable", body = crate::api::ErrorState),
        (status = 504, description = "The daemon command route timed out", body = crate::api::ErrorState)
    )
)]
pub(crate) async fn cast_spell(
    State(state): State<ApiState>,
    Path(identifier): Path<String>,
    request: Result<Json<CastSpellOptions>, JsonRejection>,
) -> Result<(StatusCode, Json<CommandStatus>), ApiError> {
    let Json(request) = action_request(request)?;
    let (pid, identity, snapshot) = action_client(&state, &identifier)?;
    let cast = resolve_spell(pid, &snapshot, request)?;
    submit_action(&state, pid, identity, ProtocolKind::CastSpell(cast)).await
}
fn resolve_skill(
    pid: u32,
    snapshot: &GameSnapshot,
    request: UseSkillOptions,
) -> Result<SkillSlot, ApiError> {
    let skills = snapshot
        .character
        .as_ref()
        .and_then(|character| character.skillbook.as_ref())
        .ok_or_else(|| {
            ApiError::new(
                StatusCode::CONFLICT,
                "skillbook_unavailable",
                "the client's current skillbook is unavailable",
                Some(pid),
            )
        })?;
    match request {
        UseSkillOptions::Slot(options) => {
            let slot = SkillSlot::new(options.slot).ok_or_else(|| {
                ApiError::new(
                    StatusCode::BAD_REQUEST,
                    "invalid_skill_slot",
                    format!("slot must be from 1 through {MAX_SKILL_SLOT}"),
                    Some(pid),
                )
            })?;
            skills
                .iter()
                .any(|skill| skill.slot == slot.get())
                .then_some(slot)
                .ok_or_else(|| skill_not_found(pid))
        }
        UseSkillOptions::Name(options) => {
            if options.name.is_empty() || options.name.len() > MAX_SKILL_NAME_BYTES {
                return Err(ApiError::new(
                    StatusCode::BAD_REQUEST,
                    "invalid_skill_name",
                    format!("name must contain from 1 through {MAX_SKILL_NAME_BYTES} bytes"),
                    Some(pid),
                ));
            }
            let mut matches = skills.iter().filter(|skill| {
                skill
                    .name
                    .as_deref()
                    .is_some_and(|name| name.eq_ignore_ascii_case(&options.name))
            });
            let skill = matches.next().ok_or_else(|| skill_not_found(pid))?;
            if matches.next().is_some() {
                return Err(ApiError::new(
                    StatusCode::CONFLICT,
                    "ambiguous_skill_name",
                    "more than one learned skill has that case-insensitive name",
                    Some(pid),
                ));
            }
            SkillSlot::new(skill.slot).ok_or_else(|| {
                ApiError::new(
                    StatusCode::CONFLICT,
                    "invalid_skillbook",
                    "the retained skillbook contains an invalid slot",
                    Some(pid),
                )
            })
        }
    }
}

fn skill_not_found(pid: u32) -> ApiError {
    ApiError::new(
        StatusCode::NOT_FOUND,
        "skill_not_found",
        "the selected skill is not currently learned",
        Some(pid),
    )
}

fn resolve_spell(
    pid: u32,
    snapshot: &GameSnapshot,
    request: CastSpellOptions,
) -> Result<SpellCast, ApiError> {
    let spells = snapshot
        .character
        .as_ref()
        .and_then(|character| character.spellbook.as_ref())
        .ok_or_else(|| {
            ApiError::new(
                StatusCode::CONFLICT,
                "spellbook_unavailable",
                "the client's current spellbook is unavailable",
                Some(pid),
            )
        })?;
    let (slot, target, input) = match request {
        CastSpellOptions::Slot(options) => {
            let slot = SpellSlot::new(options.slot).ok_or_else(|| {
                ApiError::new(
                    StatusCode::BAD_REQUEST,
                    "invalid_spell_slot",
                    format!("slot must be from 1 through {MAX_SPELL_SLOT}"),
                    Some(pid),
                )
            })?;
            (slot, options.target, options.input)
        }
        CastSpellOptions::Name(options) => {
            validate_spell_name(pid, &options.name)?;
            let mut matches = spells.iter().filter(|spell| {
                spell
                    .name
                    .as_deref()
                    .is_some_and(|name| name.eq_ignore_ascii_case(&options.name))
            });
            let spell = matches.next().ok_or_else(|| spell_not_found(pid))?;
            if matches.next().is_some() {
                return Err(ApiError::new(
                    StatusCode::CONFLICT,
                    "ambiguous_spell_name",
                    "more than one learned spell has that case-insensitive name",
                    Some(pid),
                ));
            }
            let slot = SpellSlot::new(spell.slot).ok_or_else(|| {
                ApiError::new(
                    StatusCode::CONFLICT,
                    "invalid_spellbook",
                    "the retained spellbook contains an invalid slot",
                    Some(pid),
                )
            })?;
            (slot, options.target, options.input)
        }
    };
    let spell = spells
        .iter()
        .find(|spell| spell.slot == slot.get())
        .ok_or_else(|| spell_not_found(pid))?;
    let arguments = match spell.target_type {
        ModelSpellTargetType::None if target.is_none() && input.is_none() => SpellArguments::None,
        ModelSpellTargetType::TextInput if target.is_none() => {
            let input = input.ok_or_else(|| invalid_spell_arguments(pid, "input is required"))?;
            let input = SpellInput::new(&input).ok_or_else(|| {
                invalid_spell_arguments(
                    pid,
                    format!("input must contain from 1 through {MAX_SPELL_INPUT_LEN} ASCII bytes"),
                )
            })?;
            SpellArguments::Input(input)
        }
        ModelSpellTargetType::Target if input.is_none() => target
            .map_or(Ok(SpellArguments::None), |target| {
                resolve_spell_target(pid, snapshot, target).map(SpellArguments::Target)
            })?,
        ModelSpellTargetType::Unknown(_) => {
            return Err(invalid_spell_arguments(
                pid,
                "this spell uses a numeric or unsupported argument type",
            ));
        }
        _ => {
            return Err(invalid_spell_arguments(
                pid,
                "the supplied target or input does not match this spell's argument type",
            ));
        }
    };
    Ok(SpellCast { slot, arguments })
}

fn resolve_spell_target(
    pid: u32,
    snapshot: &GameSnapshot,
    target: SpellTargetOptions,
) -> Result<SpellTarget, ApiError> {
    match target {
        SpellTargetOptions::Tile(tile) => {
            validate_destination(pid, snapshot, tile)?;
            Ok(SpellTarget::Tile {
                x: tile.x,
                y: tile.y,
            })
        }
        SpellTargetOptions::Id(id) => {
            let id = NonZeroU32::new(id).ok_or_else(|| {
                ApiError::new(
                    StatusCode::BAD_REQUEST,
                    "invalid_spell_target",
                    "target object ID must be greater than zero",
                    Some(pid),
                )
            })?;
            object_target(snapshot, id.get())
                .filter(|(_, distance)| *distance <= SPELL_TARGET_DISTANCE)
                .map(|_| SpellTarget::Object(id))
                .ok_or_else(|| target_not_found(pid))
        }
        SpellTargetOptions::Name(name) => {
            if name.is_empty() || name.len() > MAX_SPELL_NAME_BYTES {
                return Err(ApiError::new(
                    StatusCode::BAD_REQUEST,
                    "invalid_spell_target_name",
                    format!("target name must contain from 1 through {MAX_SPELL_NAME_BYTES} bytes"),
                    Some(pid),
                ));
            }
            let id = named_target(snapshot, &name).ok_or_else(|| target_not_found(pid))?;
            NonZeroU32::new(id)
                .map(SpellTarget::Object)
                .ok_or_else(|| target_not_found(pid))
        }
    }
}

fn named_target(snapshot: &GameSnapshot, requested: &str) -> Option<u32> {
    let character = snapshot.character.as_ref()?;
    let (self_x, self_y) = character
        .location
        .as_ref()?
        .x
        .zip(character.location.as_ref()?.y)?;
    let objects = snapshot.objects.as_deref().unwrap_or_default();
    let player = objects
        .iter()
        .filter_map(|object| match object {
            WorldObject::Player { id, name, x, y, .. }
                if name
                    .as_deref()
                    .is_some_and(|name| name.eq_ignore_ascii_case(requested)) =>
            {
                Some((*id, tile_distance(self_x, self_y, *x, *y)))
            }
            _ => None,
        })
        .chain(
            character
                .id
                .zip(character.name.as_deref())
                .and_then(|(id, name)| name.eq_ignore_ascii_case(requested).then_some((id, 0))),
        )
        .filter(|(_, distance)| *distance <= SPELL_TARGET_DISTANCE)
        .min_by_key(|(id, distance)| (*distance, *id));
    if let Some((id, _)) = player {
        return Some(id);
    }
    objects
        .iter()
        .filter_map(|object| match object {
            WorldObject::Creature {
                id,
                kind: CreatureKind::Npc,
                name,
                x,
                y,
                ..
            } if name
                .as_deref()
                .is_some_and(|name| name.eq_ignore_ascii_case(requested)) =>
            {
                Some((*id, tile_distance(self_x, self_y, *x, *y)))
            }
            _ => None,
        })
        .filter(|(_, distance)| *distance <= SPELL_TARGET_DISTANCE)
        .min_by_key(|(id, distance)| (*distance, *id))
        .map(|(id, _)| id)
}

fn object_target(snapshot: &GameSnapshot, requested_id: u32) -> Option<((i32, i32), u32)> {
    let character = snapshot.character.as_ref()?;
    let location = character.location.as_ref()?;
    let (self_x, self_y) = location.x.zip(location.y)?;
    if character.id == Some(requested_id) {
        return Some(((self_x, self_y), 0));
    }
    snapshot.objects.as_ref()?.iter().find_map(|object| {
        (object.id() == requested_id).then(|| {
            let (x, y) = object.position();
            ((x, y), tile_distance(self_x, self_y, x, y))
        })
    })
}

const fn tile_distance(left_x: i32, left_y: i32, right_x: i32, right_y: i32) -> u32 {
    left_x
        .abs_diff(right_x)
        .saturating_add(left_y.abs_diff(right_y))
}

fn validate_spell_name(pid: u32, name: &str) -> Result<(), ApiError> {
    if name.is_empty() || name.len() > MAX_SPELL_NAME_BYTES {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_spell_name",
            format!("name must contain from 1 through {MAX_SPELL_NAME_BYTES} bytes"),
            Some(pid),
        ));
    }
    Ok(())
}

fn spell_not_found(pid: u32) -> ApiError {
    ApiError::new(
        StatusCode::NOT_FOUND,
        "spell_not_found",
        "the selected spell is not currently learned",
        Some(pid),
    )
}

fn target_not_found(pid: u32) -> ApiError {
    ApiError::new(
        StatusCode::NOT_FOUND,
        "spell_target_not_found",
        "the selected player or NPC is not currently visible within 14 tiles",
        Some(pid),
    )
}

fn invalid_spell_arguments(pid: u32, message: impl Into<String>) -> ApiError {
    ApiError::new(
        StatusCode::BAD_REQUEST,
        "invalid_spell_arguments",
        message.into(),
        Some(pid),
    )
}
