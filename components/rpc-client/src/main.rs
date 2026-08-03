//! daRPC command-line client.

mod command_output;
mod error;
#[cfg(windows)]
mod ipc;
mod object_output;
mod output;
mod snapshot_output;

use darpc_model::Direction;
use darpc_protocol::{
    MAX_ECHO_TEXT_LEN, MAX_SKILL_SLOT, MAX_SPELL_INPUT_LEN, MAX_SPELL_SLOT, SkillSlot,
    SpellArguments, SpellCast, SpellInput, SpellSlot, SpellTarget, WalkTarget,
};
use error::{ClientError, ErrorKind, Result};
use output::{CommandResult, OutputFormat, render_error};
use std::{env, ffi::OsString, process::ExitCode};

const USAGE: &str = "\
usage:
    darpc [--output <table|json>] hello --pid <pid>
    darpc [--output <table|json>] ping --pid <pid>
    darpc [--output <table|json>] tick-health --pid <pid>
    darpc [--output <table|json>] snapshot --pid <pid>
    darpc [--output <table|json>] echo --pid <pid> <text>
    darpc [--output <table|json>] diagnostic --pid <pid>
    darpc [--output <table|json>] turn --pid <pid> <north|east|south|west>
    darpc [--output <table|json>] walk --pid <pid> <north|east|south|west>
    darpc [--output <table|json>] walk --pid <pid> <x> <y>
    darpc [--output <table|json>] skill-use --pid <pid> <slot>
    darpc [--output <table|json>] spell-cast --pid <pid> <slot>
    darpc [--output <table|json>] spell-cast --pid <pid> <slot> --target-id <id>
    darpc [--output <table|json>] spell-cast --pid <pid> <slot> --target <x> <y>
    darpc [--output <table|json>] spell-cast --pid <pid> <slot> --input <text>
    darpc [--output <table|json>] command-status --pid <pid> <command-id>
    darpc [--output <table|json>] command-cancel --pid <pid> <command-id>";

#[derive(Clone, Debug, Eq, PartialEq)]
struct Command {
    pid: u32,
    operation: Operation,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum Operation {
    Hello,
    Ping,
    TickHealth,
    Snapshot,
    Echo(String),
    Diagnostic,
    Turn(Direction),
    Walk(WalkTarget),
    UseSkill(SkillSlot),
    CastSpell(SpellCast),
    CommandStatus(u32),
    CommandCancel(u32),
}

impl Command {
    const fn name(&self) -> &'static str {
        match &self.operation {
            Operation::Hello => "hello",
            Operation::Ping => "ping",
            Operation::TickHealth => "tick-health",
            Operation::Snapshot => "snapshot",
            Operation::Echo(_) => "echo",
            Operation::Diagnostic => "diagnostic",
            Operation::Turn(_) => "turn",
            Operation::Walk(_) => "walk",
            Operation::UseSkill(_) => "skill-use",
            Operation::CastSpell(_) => "spell-cast",
            Operation::CommandStatus(_) => "command-status",
            Operation::CommandCancel(_) => "command-cancel",
        }
    }
}

fn main() -> ExitCode {
    let mut arguments: Vec<OsString> = env::args_os().skip(1).collect();
    let format = match parse_output_format(&mut arguments) {
        Ok(format) => format,
        Err(error) => {
            eprintln!("{}", render_error(OutputFormat::Table, None, &error));
            return ExitCode::from(error.kind().exit_code());
        }
    };
    let command = match parse_command(arguments) {
        Ok(command) => command,
        Err(error) => {
            let rendered = render_error(format, None, &error);
            match format {
                OutputFormat::Table => eprintln!("{rendered}"),
                OutputFormat::Json => println!("{rendered}"),
            }
            return ExitCode::from(error.kind().exit_code());
        }
    };
    let command_name = command.name();

    match execute(command) {
        Ok(result) => {
            println!("{}", result.render(format));
            ExitCode::SUCCESS
        }
        Err(error) => {
            let rendered = render_error(format, Some(command_name), &error);
            match format {
                OutputFormat::Table => eprintln!("{rendered}"),
                OutputFormat::Json => println!("{rendered}"),
            }
            ExitCode::from(error.kind().exit_code())
        }
    }
}

#[cfg(test)]
fn parse(mut arguments: Vec<OsString>) -> Result<(OutputFormat, Command)> {
    let format = parse_output_format(&mut arguments)?;
    let command = parse_command(arguments)?;
    Ok((format, command))
}

fn parse_output_format(arguments: &mut Vec<OsString>) -> Result<OutputFormat> {
    let format = if arguments.first().and_then(|value| value.to_str()) == Some("--output") {
        if arguments.len() < 2 {
            return Err(invalid_arguments("--output requires `table` or `json`"));
        }
        let value = arguments.remove(1);
        arguments.remove(0);
        match value.to_str() {
            Some("table") => OutputFormat::Table,
            Some("json") => OutputFormat::Json,
            _ => return Err(invalid_arguments("--output must be `table` or `json`")),
        }
    } else {
        OutputFormat::Table
    };
    Ok(format)
}

fn parse_command(arguments: Vec<OsString>) -> Result<Command> {
    let mut arguments = arguments.into_iter();
    let action = arguments
        .next()
        .and_then(|value| value.to_str().map(str::to_owned))
        .ok_or_else(|| ClientError::new(ErrorKind::InvalidArguments, USAGE))?;
    let pid_option = arguments
        .next()
        .and_then(|value| value.to_str().map(str::to_owned));
    if pid_option.as_deref() != Some("--pid") {
        return Err(invalid_arguments("commands require `--pid <pid>`"));
    }
    let pid = parse_pid(arguments.next())?;

    let operation = match action.as_str() {
        "hello" => Operation::Hello,
        "ping" => Operation::Ping,
        "tick-health" => Operation::TickHealth,
        "snapshot" => Operation::Snapshot,
        "diagnostic" => Operation::Diagnostic,
        "turn" => Operation::Turn(parse_direction(arguments.next())?),
        "walk" => Operation::Walk(parse_walk_target(&mut arguments)?),
        "skill-use" => Operation::UseSkill(parse_skill_slot(arguments.next())?),
        "spell-cast" => Operation::CastSpell(parse_spell_cast(&mut arguments)?),
        "command-status" => Operation::CommandStatus(parse_command_id(arguments.next())?),
        "command-cancel" => Operation::CommandCancel(parse_command_id(arguments.next())?),
        "echo" => {
            let text = arguments
                .next()
                .and_then(|value| value.into_string().ok())
                .ok_or_else(|| invalid_arguments("echo requires UTF-8 text"))?;
            if text.len() > MAX_ECHO_TEXT_LEN {
                return Err(invalid_arguments(format!(
                    "echo text is {} bytes; maximum is {MAX_ECHO_TEXT_LEN}",
                    text.len()
                )));
            }
            Operation::Echo(text)
        }
        _ => return Err(invalid_arguments(format!("unknown command `{action}`"))),
    };

    if arguments.next().is_some() {
        return Err(invalid_arguments("too many arguments"));
    }

    Ok(Command { pid, operation })
}

fn parse_direction(argument: Option<OsString>) -> Result<Direction> {
    let argument = argument.ok_or_else(|| invalid_arguments("direction is required"))?;
    match argument.to_str() {
        Some("north") => Ok(Direction::North),
        Some("east") => Ok(Direction::East),
        Some("south") => Ok(Direction::South),
        Some("west") => Ok(Direction::West),
        _ => Err(invalid_arguments(
            "direction must be north, east, south, or west",
        )),
    }
}

fn parse_walk_target(arguments: &mut impl Iterator<Item = OsString>) -> Result<WalkTarget> {
    let first = arguments
        .next()
        .ok_or_else(|| invalid_arguments("walk requires a direction or x and y coordinates"))?;
    if matches!(first.to_str(), Some("north" | "east" | "south" | "west")) {
        return Ok(WalkTarget::Direction(parse_direction(Some(first))?));
    }
    let x = parse_coordinate(Some(first), "x")?;
    let y = parse_coordinate(arguments.next(), "y")?;
    Ok(WalkTarget::Destination { x, y })
}

fn parse_coordinate(argument: Option<OsString>, name: &str) -> Result<i32> {
    let argument =
        argument.ok_or_else(|| invalid_arguments(format!("{name} coordinate is required")))?;
    let argument = argument
        .to_str()
        .ok_or_else(|| invalid_arguments(format!("{name} coordinate must be valid Unicode")))?;
    argument.parse().map_err(|_| {
        invalid_arguments(format!("{name} coordinate must be a signed 32-bit integer"))
    })
}

fn parse_command_id(argument: Option<OsString>) -> Result<u32> {
    let argument = argument.ok_or_else(|| invalid_arguments("command ID is required"))?;
    let argument = argument
        .to_str()
        .ok_or_else(|| invalid_arguments("command ID must be valid Unicode"))?;
    let command_id = argument
        .parse()
        .map_err(|_| invalid_arguments("command ID must be an unsigned 32-bit integer"))?;
    if command_id == 0 {
        return Err(invalid_arguments("command ID must be greater than zero"));
    }
    Ok(command_id)
}

fn parse_skill_slot(argument: Option<OsString>) -> Result<SkillSlot> {
    let argument = argument.ok_or_else(|| invalid_arguments("skill slot is required"))?;
    let argument = argument
        .to_str()
        .ok_or_else(|| invalid_arguments("skill slot must be valid Unicode"))?;
    let slot: u32 = argument
        .parse()
        .map_err(|_| invalid_arguments("skill slot must be an unsigned integer"))?;
    if !(1..=u32::from(MAX_SKILL_SLOT)).contains(&slot) {
        return Err(invalid_arguments(format!(
            "skill slot must be between 1 and {MAX_SKILL_SLOT}"
        )));
    }
    SkillSlot::new(slot as u8).ok_or_else(|| {
        invalid_arguments(format!("skill slot must be between 1 and {MAX_SKILL_SLOT}"))
    })
}

fn parse_spell_cast(arguments: &mut impl Iterator<Item = OsString>) -> Result<SpellCast> {
    let slot = parse_spell_slot(arguments.next())?;
    let arguments = match arguments.next() {
        None => SpellArguments::None,
        Some(option) if option == "--target-id" => {
            let id = parse_nonzero_u32(arguments.next(), "target ID")?;
            SpellArguments::Target(SpellTarget::Object(id))
        }
        Some(option) if option == "--target" => {
            let x = parse_coordinate(arguments.next(), "x")?;
            let y = parse_coordinate(arguments.next(), "y")?;
            SpellArguments::Target(SpellTarget::Tile { x, y })
        }
        Some(option) if option == "--input" => {
            let input = arguments
                .next()
                .and_then(|value| value.into_string().ok())
                .ok_or_else(|| invalid_arguments("spell input must be valid Unicode"))?;
            let input = SpellInput::new(&input).ok_or_else(|| {
                invalid_arguments(format!(
                    "spell input must contain between 1 and {MAX_SPELL_INPUT_LEN} ASCII bytes"
                ))
            })?;
            SpellArguments::Input(input)
        }
        Some(option) => {
            return Err(invalid_arguments(format!(
                "unknown spell argument `{}`",
                option.to_string_lossy()
            )));
        }
    };
    Ok(SpellCast { slot, arguments })
}

fn parse_spell_slot(argument: Option<OsString>) -> Result<SpellSlot> {
    let argument = argument.ok_or_else(|| invalid_arguments("spell slot is required"))?;
    let argument = argument
        .to_str()
        .ok_or_else(|| invalid_arguments("spell slot must be valid Unicode"))?;
    let slot: u32 = argument
        .parse()
        .map_err(|_| invalid_arguments("spell slot must be an unsigned integer"))?;
    if !(1..=u32::from(MAX_SPELL_SLOT)).contains(&slot) {
        return Err(invalid_arguments(format!(
            "spell slot must be between 1 and {MAX_SPELL_SLOT}"
        )));
    }
    SpellSlot::new(slot as u8).ok_or_else(|| {
        invalid_arguments(format!("spell slot must be between 1 and {MAX_SPELL_SLOT}"))
    })
}

fn parse_nonzero_u32(argument: Option<OsString>, name: &str) -> Result<std::num::NonZeroU32> {
    let argument = argument.ok_or_else(|| invalid_arguments(format!("{name} is required")))?;
    let argument = argument
        .to_str()
        .ok_or_else(|| invalid_arguments(format!("{name} must be valid Unicode")))?;
    let value = argument
        .parse()
        .map_err(|_| invalid_arguments(format!("{name} must be an unsigned 32-bit integer")))?;
    std::num::NonZeroU32::new(value)
        .ok_or_else(|| invalid_arguments(format!("{name} must be greater than zero")))
}

fn parse_pid(argument: Option<OsString>) -> Result<u32> {
    let argument = argument.ok_or_else(|| invalid_arguments("--pid requires a value"))?;
    let argument = argument
        .to_str()
        .ok_or_else(|| invalid_arguments("PID must be valid Unicode"))?;
    let pid = argument
        .parse()
        .map_err(|_| invalid_arguments("PID must be an unsigned 32-bit integer"))?;
    if pid == 0 {
        return Err(invalid_arguments("PID must be greater than zero"));
    }
    Ok(pid)
}

fn invalid_arguments(message: impl Into<String>) -> ClientError {
    ClientError::new(
        ErrorKind::InvalidArguments,
        format!("{}\n{USAGE}", message.into()),
    )
}

#[cfg(windows)]
fn execute(command: Command) -> Result<CommandResult> {
    ipc::execute(command.pid, command.operation)
}

#[cfg(not(windows))]
fn execute(_command: Command) -> Result<CommandResult> {
    Err(ClientError::new(
        ErrorKind::UnsupportedPlatform,
        "direct IPC diagnostics require Windows",
    ))
}

#[cfg(test)]
mod tests {
    use super::{Command, Operation, OutputFormat, parse};
    use darpc_model::Direction;
    use darpc_protocol::{
        SkillSlot, SpellArguments, SpellCast, SpellInput, SpellSlot, SpellTarget, WalkTarget,
    };
    use std::ffi::OsString;

    fn arguments(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }

    #[test]
    fn parses_direct_commands() {
        assert_eq!(
            parse(arguments(&["hello", "--pid", "42"])).unwrap(),
            (
                OutputFormat::Table,
                Command {
                    pid: 42,
                    operation: Operation::Hello,
                }
            )
        );
        assert_eq!(
            parse(arguments(&[
                "--output", "json", "echo", "--pid", "7", "hello"
            ]))
            .unwrap(),
            (
                OutputFormat::Json,
                Command {
                    pid: 7,
                    operation: Operation::Echo("hello".into()),
                }
            )
        );
        assert_eq!(
            parse(arguments(&["tick-health", "--pid", "9"])).unwrap(),
            (
                OutputFormat::Table,
                Command {
                    pid: 9,
                    operation: Operation::TickHealth,
                }
            )
        );
        assert_eq!(
            parse(arguments(&["diagnostic", "--pid", "9"])).unwrap(),
            (
                OutputFormat::Table,
                Command {
                    pid: 9,
                    operation: Operation::Diagnostic,
                }
            )
        );
        assert_eq!(
            parse(arguments(&["command-status", "--pid", "9", "17"])).unwrap(),
            (
                OutputFormat::Table,
                Command {
                    pid: 9,
                    operation: Operation::CommandStatus(17),
                }
            )
        );
        assert_eq!(
            parse(arguments(&["snapshot", "--pid", "10"])).unwrap(),
            (
                OutputFormat::Table,
                Command {
                    pid: 10,
                    operation: Operation::Snapshot,
                }
            )
        );
        assert_eq!(
            parse(arguments(&["turn", "--pid", "10", "west"])).unwrap(),
            (
                OutputFormat::Table,
                Command {
                    pid: 10,
                    operation: Operation::Turn(Direction::West),
                }
            )
        );
        assert_eq!(
            parse(arguments(&["walk", "--pid", "10", "north"])).unwrap(),
            (
                OutputFormat::Table,
                Command {
                    pid: 10,
                    operation: Operation::Walk(WalkTarget::Direction(Direction::North)),
                }
            )
        );
        assert_eq!(
            parse(arguments(&["walk", "--pid", "10", "120", "85"])).unwrap(),
            (
                OutputFormat::Table,
                Command {
                    pid: 10,
                    operation: Operation::Walk(WalkTarget::Destination { x: 120, y: 85 }),
                }
            )
        );
        assert_eq!(
            parse(arguments(&["skill-use", "--pid", "10", "5"])).unwrap(),
            (
                OutputFormat::Table,
                Command {
                    pid: 10,
                    operation: Operation::UseSkill(SkillSlot::new(5).unwrap()),
                }
            )
        );
        assert_eq!(
            parse(arguments(&[
                "spell-cast",
                "--pid",
                "10",
                "7",
                "--input",
                "nothing",
            ]))
            .unwrap(),
            (
                OutputFormat::Table,
                Command {
                    pid: 10,
                    operation: Operation::CastSpell(SpellCast {
                        slot: SpellSlot::new(7).unwrap(),
                        arguments: SpellArguments::Input(SpellInput::new("nothing").unwrap()),
                    }),
                }
            )
        );
        assert_eq!(
            parse(arguments(&[
                "spell-cast",
                "--pid",
                "10",
                "8",
                "--target-id",
                "77",
            ]))
            .unwrap(),
            (
                OutputFormat::Table,
                Command {
                    pid: 10,
                    operation: Operation::CastSpell(SpellCast {
                        slot: SpellSlot::new(8).unwrap(),
                        arguments: SpellArguments::Target(SpellTarget::Object(
                            std::num::NonZeroU32::new(77).unwrap(),
                        )),
                    }),
                }
            )
        );
    }

    #[test]
    fn rejects_invalid_pid_and_extra_arguments() {
        assert!(parse(arguments(&["ping", "--pid", "0"])).is_err());
        assert!(parse(arguments(&["hello", "--pid", "1", "extra"])).is_err());
        assert!(parse(arguments(&["turn", "--pid", "1", "up"])).is_err());
        assert!(parse(arguments(&["walk", "--pid", "1", "10"])).is_err());
        assert!(parse(arguments(&["walk", "--pid", "1", "north", "extra"])).is_err());
        assert!(parse(arguments(&["skill-use", "--pid", "1", "0"])).is_err());
        assert!(parse(arguments(&["skill-use", "--pid", "1", "91"])).is_err());
        assert!(parse(arguments(&["spell-cast", "--pid", "1", "0"])).is_err());
        assert!(parse(arguments(&["spell-cast", "--pid", "1", "1", "--input"])).is_err());
    }

    #[test]
    fn prints_usage_once_for_incomplete_commands() {
        let error = parse(arguments(&["hello"])).unwrap_err();
        assert_eq!(error.message().matches("usage:").count(), 1);
    }

    #[test]
    fn rejects_echo_over_the_wire_limit() {
        let text = "a".repeat(darpc_protocol::MAX_ECHO_TEXT_LEN + 1);
        assert!(parse(vec!["echo".into(), "--pid".into(), "1".into(), text.into(),]).is_err());
    }
}
