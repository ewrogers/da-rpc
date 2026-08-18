use crate::output::json_string;
use darpc_model::WorldObject;
use darpc_protocol::{CommandFailure, CommandKind, CommandResult, CommandState, CommandStatus};

pub(crate) fn render_human(
    action: &str,
    pid: u32,
    request_id: u32,
    round_trip_ms: u32,
    result: CommandResult,
) -> String {
    match result {
        CommandResult::Who { status: _, list } => {
            render_who_human(pid, request_id, round_trip_ms, &list)
        }
        CommandResult::Legend { status: _, marks } => {
            render_legend_human(pid, request_id, round_trip_ms, &marks)
        }
        CommandResult::Player { player, status: _ } => {
            let WorldObject::Player {
                id,
                name,
                x,
                y,
                visual,
                profile: Some(profile),
                ..
            } = player.as_ref()
            else {
                unreachable!("the protocol validates inspected player results")
            };
            format!(
                concat!(
                    "player inspected: pid={} request_id={} round_trip_ms={} id={} name={:?} x={} y={} ",
                    "nation={:?} title={:?} guild_rank={:?} display_class={:?} guild={:?} ",
                    "visual={:?} user_state={:?} is_group_open={} equipment={} legend={} inspected_tick_ms={}"
                ),
                pid,
                request_id,
                round_trip_ms,
                id,
                name,
                x,
                y,
                profile.identity.nation,
                profile.identity.title,
                profile.identity.guild_rank,
                profile.identity.display_class,
                profile.identity.guild,
                visual,
                profile.user_state,
                profile.is_group_open,
                profile.equipment.len(),
                profile.legend.len(),
                profile.inspected_tick_ms,
            )
        }
        CommandResult::Status(status) => format!(
            concat!(
                "{} succeeded: pid={} request_id={} round_trip_ms={} ",
                "command_id={} kind={} state={} enqueued_tick_ms={} deadline_tick_ms={} ",
                "started_tick_ms={} completed_tick_ms={} execution_us={} main_thread_id={} failure={}"
            ),
            action,
            pid,
            request_id,
            round_trip_ms,
            status.command_id,
            kind(status.kind),
            state(status.state),
            status.enqueued_tick_ms,
            status.deadline_tick_ms,
            optional(status.started_tick_ms),
            optional(status.completed_tick_ms),
            optional(status.execution_us),
            optional(status.main_thread_id),
            failure(status.failure),
        ),
        result => format!(
            "{action} completed: pid={pid} request_id={request_id} round_trip_ms={round_trip_ms} result={}",
            result_name(result)
        ),
    }
}

pub(crate) fn render_json(
    action: &str,
    pid: u32,
    request_id: u32,
    round_trip_ms: u32,
    result: CommandResult,
) -> String {
    match result {
        CommandResult::Who { status, list } => {
            render_who_json(pid, request_id, round_trip_ms, status, &list)
        }
        CommandResult::Legend { status, marks } => {
            render_legend_json(pid, request_id, round_trip_ms, status, &marks)
        }
        CommandResult::Player { status, player } => {
            let WorldObject::Player {
                id,
                profile: Some(profile),
                ..
            } = player.as_ref()
            else {
                unreachable!("the protocol validates inspected player results")
            };
            render_player_json(
                pid,
                request_id,
                round_trip_ms,
                status,
                player.as_ref(),
                *id,
                profile,
            )
        }
        CommandResult::Status(status) => {
            render_status_json(action, pid, request_id, round_trip_ms, status)
        }
        result => format!(
            concat!(
                "{{\"ok\":true,\"command\":{},\"pid\":{},\"request_id\":{},",
                "\"round_trip_ms\":{},\"result\":{}}}"
            ),
            json_string(action),
            pid,
            request_id,
            round_trip_ms,
            json_string(result_name(result)),
        ),
    }
}

fn render_player_json(
    pid: u32,
    request_id: u32,
    round_trip_ms: u32,
    status: CommandStatus,
    player: &WorldObject,
    id: u32,
    profile: &darpc_model::PlayerProfile,
) -> String {
    format!(
        concat!(
            "{{\"ok\":true,\"command\":\"inspect\",\"pid\":{},\"request_id\":{},",
            "\"round_trip_ms\":{},\"command_id\":{},\"player\":{{\"id\":{},",
            "\"identity\":{{\"nation\":{},\"title\":{},\"guild_rank\":{},",
            "\"display_class\":{},\"guild\":{}}},\"visual\":{},\"user_state\":{},",
            "\"is_group_open\":{},\"equipment_count\":{},\"legend_count\":{},",
            "\"inspected_tick_ms\":{}}}}}"
        ),
        pid,
        request_id,
        round_trip_ms,
        status.command_id,
        id,
        json_string(&format!("{:?}", profile.identity.nation).to_ascii_lowercase()),
        json_string(&profile.identity.title),
        json_string(&profile.identity.guild_rank),
        json_string(&profile.identity.display_class),
        json_string(&profile.identity.guild),
        crate::object_output::json_value(player)["visual"],
        json_string(&format!("{:?}", profile.user_state).to_ascii_lowercase()),
        profile.is_group_open,
        profile.equipment.len(),
        profile.legend.len(),
        profile.inspected_tick_ms,
    )
}

fn render_status_json(
    action: &str,
    pid: u32,
    request_id: u32,
    round_trip_ms: u32,
    status: CommandStatus,
) -> String {
    format!(
        concat!(
            "{{\"ok\":true,\"command\":{},\"pid\":{},\"request_id\":{},",
            "\"round_trip_ms\":{},\"result\":\"status\",\"status\":{{",
            "\"command_id\":{},\"kind\":{},\"state\":{},\"enqueued_tick_ms\":{},",
            "\"deadline_tick_ms\":{},\"started_tick_ms\":{},\"completed_tick_ms\":{},",
            "\"execution_us\":{},\"main_thread_id\":{},\"failure\":{}}}}}"
        ),
        json_string(action),
        pid,
        request_id,
        round_trip_ms,
        status.command_id,
        json_string(kind(status.kind)),
        json_string(state(status.state)),
        status.enqueued_tick_ms,
        status.deadline_tick_ms,
        optional_json(status.started_tick_ms),
        optional_json(status.completed_tick_ms),
        optional_json(status.execution_us),
        optional_json(status.main_thread_id),
        status
            .failure
            .map_or_else(|| "null".into(), |value| json_string(failure_name(value))),
    )
}

fn kind(kind: CommandKind) -> &'static str {
    match kind {
        CommandKind::Diagnostic => "diagnostic",
        CommandKind::Turn(_) => "turn",
        CommandKind::Walk(_) => "walk",
        CommandKind::UseSkill(_) => "use_skill",
        CommandKind::CastSpell(_) => "cast_spell",
        CommandKind::UseItem(_) => "use_item",
        CommandKind::DropItem(_) => "drop_item",
        CommandKind::DropGold(_) => "drop_gold",
        CommandKind::PickupItem(_) => "pickup_item",
        CommandKind::Unequip(_) => "unequip",
        CommandKind::Emote(_) => "emote",
        CommandKind::GiveItem(_) => "give_item",
        CommandKind::GiveGold(_) => "give_gold",
        CommandKind::SwapSlots(_) => "swap_slots",
        CommandKind::Interact(_) => "interact",
        CommandKind::Dialog(_) => "dialog",
        CommandKind::Group(_) => "group",
        CommandKind::Who => "who",
        CommandKind::Exchange(_) => "exchange",
        CommandKind::Chant(_) => "chant",
        CommandKind::Legend => "legend",
        CommandKind::Raw(_) => "raw",
        CommandKind::Assail => "assail",
        CommandKind::InspectPlayer(_) => "inspect_player",
        CommandKind::Resync => "resync",
        CommandKind::Message(_) => "message",
        CommandKind::AddStat(_) => "add_stat",
        CommandKind::SelectFieldMapDestination(_) => "select_field_map_destination",
    }
}

fn state(state: CommandState) -> &'static str {
    match state {
        CommandState::Accepted => "accepted",
        CommandState::Executed => "executed",
        CommandState::Failed => "failed",
        CommandState::Cancelled => "cancelled",
        CommandState::TimedOut => "timed_out",
    }
}

fn failure(failure: Option<CommandFailure>) -> &'static str {
    failure.map_or("none", failure_name)
}

fn failure_name(failure: CommandFailure) -> &'static str {
    match failure {
        CommandFailure::Internal => "internal",
        CommandFailure::InvalidState => "invalid_state",
        CommandFailure::InvalidDestination => "invalid_destination",
        CommandFailure::Rejected => "rejected",
        CommandFailure::NoPath => "no_path",
        CommandFailure::InvalidSkill => "invalid_skill",
        CommandFailure::InvalidSpell => "invalid_spell",
        CommandFailure::InvalidArguments => "invalid_arguments",
        CommandFailure::InvalidTarget => "invalid_target",
    }
}

fn result_name(result: CommandResult) -> &'static str {
    match result {
        CommandResult::Status(_) => "status",
        CommandResult::Who { .. } => "who",
        CommandResult::Legend { .. } => "legend",
        CommandResult::Busy => "busy",
        CommandResult::NotFound => "not_found",
        CommandResult::Unavailable => "unavailable",
        CommandResult::Player { .. } => "player",
    }
}

fn render_who_human(
    pid: u32,
    request_id: u32,
    round_trip_ms: u32,
    list: &darpc_model::WhoList,
) -> String {
    let mut output = format!(
        "who succeeded: pid={pid} request_id={request_id} round_trip_ms={round_trip_ms} world={} country={}\nNAME\tCLASS\tSTATE\tTITLE\tGUILDMATE\tMASTER",
        list.world_count, list.country_count
    );
    for player in &list.players {
        use std::fmt::Write as _;
        let _ = write!(
            output,
            "\n{}\t{:?}\t{:?}\t{}\t{}\t{}",
            player.name,
            player.class,
            player.state,
            player.title,
            player.is_guildmate,
            player.is_master
        );
    }
    output
}

fn render_who_json(
    pid: u32,
    request_id: u32,
    round_trip_ms: u32,
    status: CommandStatus,
    list: &darpc_model::WhoList,
) -> String {
    let players = list
        .players
        .iter()
        .map(|player| {
            format!(
                concat!(
                    "{{\"name\":{},\"title\":{},\"class\":{},\"state\":{},",
                    "\"color\":{},\"is_master\":{},\"is_guildmate\":{}}}"
                ),
                json_string(&player.name),
                json_string(&player.title),
                json_string(who_class(player.class)),
                json_string(who_state(player.state)),
                player.color,
                player.is_master,
                player.is_guildmate,
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!(
        concat!(
            "{{\"ok\":true,\"command\":\"who\",\"pid\":{},\"request_id\":{},",
            "\"round_trip_ms\":{},\"result\":\"who\",\"command_id\":{},",
            "\"world_count\":{},\"country_count\":{},\"players\":[{}]}}"
        ),
        pid,
        request_id,
        round_trip_ms,
        status.command_id,
        list.world_count,
        list.country_count,
        players,
    )
}

fn who_state(state: darpc_model::UserState) -> &'static str {
    match state {
        darpc_model::UserState::Awake => "awake",
        darpc_model::UserState::DoNotDisturb => "do_not_disturb",
        darpc_model::UserState::Daydreaming => "daydreaming",
        darpc_model::UserState::NeedGroup => "need_group",
        darpc_model::UserState::Grouped => "grouped",
        darpc_model::UserState::LoneHunter => "lone_hunter",
        darpc_model::UserState::GroupHunting => "group_hunting",
        darpc_model::UserState::NeedHelp => "need_help",
        darpc_model::UserState::Unknown(_) => "unknown",
    }
}

fn who_class(class: darpc_model::CharacterClass) -> &'static str {
    match class {
        darpc_model::CharacterClass::Peasant => "peasant",
        darpc_model::CharacterClass::Warrior => "warrior",
        darpc_model::CharacterClass::Rogue => "rogue",
        darpc_model::CharacterClass::Wizard => "wizard",
        darpc_model::CharacterClass::Priest => "priest",
        darpc_model::CharacterClass::Monk => "monk",
        darpc_model::CharacterClass::Unknown(_) => "unknown",
    }
}

fn render_legend_human(
    pid: u32,
    request_id: u32,
    round_trip_ms: u32,
    marks: &[darpc_model::LegendMark],
) -> String {
    let mut output = format!(
        "legend succeeded: pid={pid} request_id={request_id} round_trip_ms={round_trip_ms} count={}\nICON\tCOLOR\tTAG\tTEXT",
        marks.len()
    );
    for mark in marks {
        use std::fmt::Write as _;
        let _ = write!(
            output,
            "\n{}\t{}\t{}\t{}",
            legend_icon(mark.icon),
            mark.color,
            mark.tag,
            mark.text
        );
    }
    output
}

fn render_legend_json(
    pid: u32,
    request_id: u32,
    round_trip_ms: u32,
    status: CommandStatus,
    marks: &[darpc_model::LegendMark],
) -> String {
    let marks = marks
        .iter()
        .map(|mark| {
            format!(
                "{{\"text\":{},\"tag\":{},\"color\":{},\"icon\":{}}}",
                json_string(&mark.text),
                json_string(&mark.tag),
                mark.color,
                json_string(legend_icon(mark.icon)),
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!(
        concat!(
            "{{\"ok\":true,\"command\":\"legend\",\"pid\":{},\"request_id\":{},",
            "\"round_trip_ms\":{},\"result\":\"legend\",\"command_id\":{},",
            "\"marks\":[{}]}}"
        ),
        pid, request_id, round_trip_ms, status.command_id, marks,
    )
}

fn legend_icon(icon: darpc_model::LegendIcon) -> &'static str {
    match icon {
        darpc_model::LegendIcon::Aisling => "aisling",
        darpc_model::LegendIcon::Warrior => "warrior",
        darpc_model::LegendIcon::Rogue => "rogue",
        darpc_model::LegendIcon::Wizard => "wizard",
        darpc_model::LegendIcon::Priest => "priest",
        darpc_model::LegendIcon::Monk => "monk",
        darpc_model::LegendIcon::Heart => "heart",
        darpc_model::LegendIcon::Victory => "victory",
        darpc_model::LegendIcon::None => "none",
        darpc_model::LegendIcon::Unknown(_) => "unknown",
    }
}

fn optional(value: Option<u32>) -> String {
    value.map_or_else(|| "none".into(), |value| value.to_string())
}

fn optional_json(value: Option<u32>) -> String {
    value.map_or_else(|| "null".into(), |value| value.to_string())
}

#[cfg(test)]
mod tests {
    use super::render_json;
    use darpc_model::{CharacterClass, LegendIcon, LegendMark, UserState, WhoList, WhoPlayer};
    use darpc_protocol::{CommandKind, CommandResult, CommandState, CommandStatus};

    fn status(kind: CommandKind) -> CommandStatus {
        CommandStatus {
            command_id: 9,
            kind,
            state: CommandState::Executed,
            enqueued_tick_ms: 100,
            deadline_tick_ms: 1_100,
            started_tick_ms: Some(101),
            completed_tick_ms: Some(101),
            execution_us: Some(0),
            main_thread_id: Some(77),
            failure: None,
        }
    }

    #[test]
    fn zero_duration_is_present_in_command_json() {
        let output = render_json(
            "diagnostic",
            42,
            1,
            3,
            CommandResult::Status(status(CommandKind::Diagnostic)),
        );
        assert!(output.contains("\"execution_us\":0"));
        assert!(output.contains("\"state\":\"executed\""));
    }

    #[test]
    fn who_json_has_a_stable_shape_and_escaping() {
        let output = render_json(
            "who",
            42,
            1,
            3,
            CommandResult::Who {
                status: status(CommandKind::Who),
                list: WhoList {
                    world_count: 2,
                    country_count: 1,
                    players: vec![WhoPlayer {
                        name: "Zi\"Lo".into(),
                        title: "Guide".into(),
                        class: CharacterClass::Wizard,
                        state: UserState::NeedGroup,
                        color: 4,
                        is_master: true,
                        is_guildmate: false,
                    }],
                },
            },
        );
        let value: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert_eq!(
            value,
            serde_json::json!({
                "ok": true,
                "command": "who",
                "pid": 42,
                "request_id": 1,
                "round_trip_ms": 3,
                "result": "who",
                "command_id": 9,
                "world_count": 2,
                "country_count": 1,
                "players": [{
                    "name": "Zi\"Lo",
                    "title": "Guide",
                    "class": "wizard",
                    "state": "need_group",
                    "color": 4,
                    "is_master": true,
                    "is_guildmate": false,
                }],
            })
        );
    }

    #[test]
    fn legend_json_has_a_stable_shape_and_escaping() {
        let output = render_json(
            "legend",
            42,
            1,
            3,
            CommandResult::Legend {
                status: status(CommandKind::Legend),
                marks: vec![LegendMark {
                    text: "Sgrios's \"scar\"".into(),
                    tag: "D".into(),
                    color: 53,
                    icon: LegendIcon::Aisling,
                }],
            },
        );
        let value: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert_eq!(
            value,
            serde_json::json!({
                "ok": true,
                "command": "legend",
                "pid": 42,
                "request_id": 1,
                "round_trip_ms": 3,
                "result": "legend",
                "command_id": 9,
                "marks": [{
                    "text": "Sgrios's \"scar\"",
                    "tag": "D",
                    "color": 53,
                    "icon": "aisling",
                }],
            })
        );
    }
}
