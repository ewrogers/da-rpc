use crate::output::json_string;
use darpc_protocol::{CommandFailure, CommandKind, CommandResult, CommandState, CommandStatus};

pub(crate) fn render_human(
    action: &str,
    pid: u32,
    request_id: u32,
    round_trip_ms: u32,
    result: CommandResult,
) -> String {
    match result {
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
        CommandResult::Busy => "busy",
        CommandResult::NotFound => "not_found",
        CommandResult::Unavailable => "unavailable",
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
    use darpc_protocol::{CommandKind, CommandResult, CommandState, CommandStatus};

    #[test]
    fn zero_duration_is_present_in_command_json() {
        let output = render_json(
            "diagnostic",
            42,
            1,
            3,
            CommandResult::Status(CommandStatus {
                command_id: 9,
                kind: CommandKind::Diagnostic,
                state: CommandState::Executed,
                enqueued_tick_ms: 100,
                deadline_tick_ms: 1_100,
                started_tick_ms: Some(101),
                completed_tick_ms: Some(101),
                execution_us: Some(0),
                main_thread_id: Some(77),
                failure: None,
            }),
        );
        assert!(output.contains("\"execution_us\":0"));
        assert!(output.contains("\"state\":\"executed\""));
    }
}
