use crate::{
    error::{ClientError, ErrorKind, Result},
    output::OutputFormat,
};
use darpc_model::{Direction, EquipmentSlot, emote_code, is_client_emote_code};
use darpc_protocol::{
    ChantText, DialogAction, DialogCommand, DialogText, ExchangeCommand, GoldTransfer,
    GroupCommand, GroupInvitationAction, GroupText, ItemSlot, ItemTransfer, MAX_DIALOG_INPUT_LEN,
    MAX_ECHO_TEXT_LEN, MAX_ITEM_SLOT, MAX_RAW_PACKET_PAYLOAD_LEN, MAX_SKILL_SLOT,
    MAX_SPELL_INPUT_LEN, MAX_SPELL_SLOT, RawPacket, RawPacketDirection, SkillSlot, SlotSwap,
    SpellArguments, SpellCast, SpellInput, SpellSlot, SpellTarget, TilePosition, TransferTarget,
    WalkTarget,
};
use std::ffi::OsString;

const USAGE: &str = "\
usage:
    darpc [--output <table|json>] hello --pid <pid>
    darpc [--output <table|json>] ping --pid <pid>
    darpc [--output <table|json>] tick health --pid <pid>
    darpc [--output <table|json>] snapshot --pid <pid>
    darpc [--output <table|json>] echo --pid <pid> <text>
    darpc [--output <table|json>] diagnostic --pid <pid>
    darpc [--output <table|json>] raw send --pid <pid> <client|server> <0xNN> [hex-payload]
    darpc [--output <table|json>] turn --pid <pid> <north|east|south|west>
    darpc [--output <table|json>] walk --pid <pid> <north|east|south|west>
    darpc [--output <table|json>] walk --pid <pid> <x> <y>
    darpc [--output <table|json>] skill use --pid <pid> <slot>
    darpc [--output <table|json>] skill swap --pid <pid> <source> <destination>
    darpc [--output <table|json>] spell cast --pid <pid> <slot>
    darpc [--output <table|json>] spell cast --pid <pid> <slot> --target-id <id>
    darpc [--output <table|json>] spell cast --pid <pid> <slot> --target <x> <y>
    darpc [--output <table|json>] spell cast --pid <pid> <slot> --input <text>
    darpc [--output <table|json>] spell swap --pid <pid> <source> <destination>
    darpc [--output <table|json>] item use --pid <pid> <slot>
    darpc [--output <table|json>] item drop --pid <pid> <slot> <x> <y> [quantity]
    darpc [--output <table|json>] item give --pid <pid> <slot> <object-id> [quantity]
    darpc [--output <table|json>] item swap --pid <pid> <source> <destination>
    darpc [--output <table|json>] gold drop --pid <pid> <amount> <x> <y>
    darpc [--output <table|json>] gold give --pid <pid> <amount> <object-id>
    darpc [--output <table|json>] item pickup --pid <pid> <x> <y>
    darpc [--output <table|json>] unequip --pid <pid> <slot>
    darpc [--output <table|json>] emote --pid <pid> <name|code>
    darpc [--output <table|json>] chant --pid <pid> <text>
    darpc [--output <table|json>] item sell --pid <pid> <name>
    darpc [--output <table|json>] item sell-all --pid <pid> <name>
    darpc [--output <table|json>] item deposit --pid <pid> <name>
    darpc [--output <table|json>] item withdraw --pid <pid> <name>
    darpc [--output <table|json>] item repair --pid <pid> <name>
    darpc [--output <table|json>] item repair-all --pid <pid>
    darpc [--output <table|json>] interact --pid <pid> <object-id>
    darpc [--output <table|json>] dialog select --pid <pid> <revision> <index> [quantity]
    darpc [--output <table|json>] dialog input --pid <pid> <revision> <text>
    darpc [--output <table|json>] dialog previous --pid <pid> <revision>
    darpc [--output <table|json>] dialog next --pid <pid> <revision>
    darpc [--output <table|json>] dialog close --pid <pid> <revision>
    darpc [--output <table|json>] group toggle --pid <pid>
    darpc [--output <table|json>] group invite --pid <pid> <player>
    darpc [--output <table|json>] group accept --pid <pid> <invitation-id>
    darpc [--output <table|json>] group decline --pid <pid> <invitation-id>
    darpc [--output <table|json>] exchange item --pid <pid> <slot> [quantity]
    darpc [--output <table|json>] exchange gold --pid <pid> <amount>
    darpc [--output <table|json>] exchange accept --pid <pid>
    darpc [--output <table|json>] exchange cancel --pid <pid>
    darpc [--output <table|json>] who --pid <pid>
    darpc [--output <table|json>] legend --pid <pid>
    darpc [--output <table|json>] command status --pid <pid> <command-id>
    darpc [--output <table|json>] command cancel --pid <pid> <command-id>";

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Command {
    pub(crate) pid: u32,
    pub(crate) operation: Operation,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Operation {
    Hello,
    Ping,
    TickHealth,
    Snapshot,
    Echo(String),
    Diagnostic,
    Raw(RawPacket),
    Turn(Direction),
    Walk(WalkTarget),
    UseSkill(SkillSlot),
    SwapSlots(SlotSwap),
    CastSpell(SpellCast),
    UseItem(ItemSlot),
    DropItem(ItemTransfer),
    GiveItem(ItemTransfer),
    DropGold(GoldTransfer),
    GiveGold(GoldTransfer),
    PickupItem(TilePosition),
    Unequip(EquipmentSlot),
    Emote(u8),
    Interact(std::num::NonZeroU32),
    Dialog(DialogCommand),
    Group(GroupCommand),
    Exchange(ExchangeCommand),
    Chant {
        action: ChantAction,
        text: ChantText,
    },
    Who,
    Legend,
    CommandStatus(u32),
    CommandCancel(u32),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ChantAction {
    Chant,
    Sell,
    SellAll,
    Deposit,
    Withdraw,
    Repair,
    RepairAll,
}

impl ChantAction {
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Chant => "chant",
            Self::Sell => "item sell",
            Self::SellAll => "item sell-all",
            Self::Deposit => "item deposit",
            Self::Withdraw => "item withdraw",
            Self::Repair => "item repair",
            Self::RepairAll => "item repair-all",
        }
    }
}

impl Command {
    pub(crate) const fn name(&self) -> &'static str {
        match &self.operation {
            Operation::Hello => "hello",
            Operation::Ping => "ping",
            Operation::TickHealth => "tick health",
            Operation::Snapshot => "snapshot",
            Operation::Echo(_) => "echo",
            Operation::Diagnostic => "diagnostic",
            Operation::Raw(_) => "raw send",
            Operation::Turn(_) => "turn",
            Operation::Walk(_) => "walk",
            Operation::UseSkill(_) => "skill use",
            Operation::SwapSlots(SlotSwap::Skillbook { .. }) => "skill swap",
            Operation::SwapSlots(SlotSwap::Spellbook { .. }) => "spell swap",
            Operation::SwapSlots(SlotSwap::Inventory { .. }) => "item swap",
            Operation::CastSpell(_) => "spell cast",
            Operation::UseItem(_) => "item use",
            Operation::DropItem(_) => "item drop",
            Operation::GiveItem(_) => "item give",
            Operation::DropGold(_) => "gold drop",
            Operation::GiveGold(_) => "gold give",
            Operation::PickupItem(_) => "item pickup",
            Operation::Unequip(_) => "unequip",
            Operation::Emote(_) => "emote",
            Operation::Interact(_) => "interact",
            Operation::Dialog(DialogCommand {
                action: DialogAction::Select { .. },
                ..
            }) => "dialog select",
            Operation::Dialog(DialogCommand {
                action: DialogAction::Input(_),
                ..
            }) => "dialog input",
            Operation::Dialog(DialogCommand {
                action: DialogAction::Previous,
                ..
            }) => "dialog previous",
            Operation::Dialog(DialogCommand {
                action: DialogAction::Next,
                ..
            }) => "dialog next",
            Operation::Dialog(DialogCommand {
                action: DialogAction::Close,
                ..
            }) => "dialog close",
            Operation::Group(GroupCommand::Toggle) => "group toggle",
            Operation::Group(GroupCommand::Invite(_)) => "group invite",
            Operation::Group(GroupCommand::Respond {
                action: GroupInvitationAction::Accept,
                ..
            }) => "group accept",
            Operation::Group(GroupCommand::Respond {
                action: GroupInvitationAction::Decline,
                ..
            }) => "group decline",
            Operation::Exchange(ExchangeCommand::AddItem { .. }) => "exchange item",
            Operation::Exchange(ExchangeCommand::SetGold(_)) => "exchange gold",
            Operation::Exchange(ExchangeCommand::Accept) => "exchange accept",
            Operation::Exchange(ExchangeCommand::Cancel) => "exchange cancel",
            Operation::Chant { action, .. } => action.name(),
            Operation::Who => "who",
            Operation::Legend => "legend",
            Operation::CommandStatus(_) => "command status",
            Operation::CommandCancel(_) => "command cancel",
        }
    }
}

#[cfg(test)]
fn parse(mut arguments: Vec<OsString>) -> Result<(OutputFormat, Command)> {
    let format = parse_output_format(&mut arguments)?;
    let command = parse_command(arguments)?;
    Ok((format, command))
}

pub(crate) fn parse_output_format(arguments: &mut Vec<OsString>) -> Result<OutputFormat> {
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

pub(crate) fn parse_command(arguments: Vec<OsString>) -> Result<Command> {
    let mut arguments = arguments.into_iter();
    let action = parse_action(&mut arguments)?;
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
        "tick health" => Operation::TickHealth,
        "snapshot" => Operation::Snapshot,
        "diagnostic" => Operation::Diagnostic,
        "raw send" => Operation::Raw(parse_raw_packet(&mut arguments)?),
        "turn" => Operation::Turn(parse_direction(arguments.next())?),
        "walk" => Operation::Walk(parse_walk_target(&mut arguments)?),
        "skill use" => Operation::UseSkill(parse_skill_slot(arguments.next())?),
        "skill swap" => Operation::SwapSlots(SlotSwap::Skillbook {
            source: parse_skill_slot(arguments.next())?,
            destination: parse_skill_slot(arguments.next())?,
        }),
        "spell cast" => Operation::CastSpell(parse_spell_cast(&mut arguments)?),
        "spell swap" => Operation::SwapSlots(SlotSwap::Spellbook {
            source: parse_spell_slot(arguments.next())?,
            destination: parse_spell_slot(arguments.next())?,
        }),
        "item use" => Operation::UseItem(parse_item_slot(arguments.next())?),
        "item drop" => Operation::DropItem(parse_item_transfer(&mut arguments, false)?),
        "item give" => Operation::GiveItem(parse_item_transfer(&mut arguments, true)?),
        "item swap" => Operation::SwapSlots(SlotSwap::Inventory {
            source: parse_item_slot(arguments.next())?,
            destination: parse_item_slot(arguments.next())?,
        }),
        "gold drop" => Operation::DropGold(parse_gold_transfer(&mut arguments, false)?),
        "gold give" => Operation::GiveGold(parse_gold_transfer(&mut arguments, true)?),
        "item pickup" => Operation::PickupItem(TilePosition {
            x: parse_coordinate(arguments.next(), "x")?,
            y: parse_coordinate(arguments.next(), "y")?,
        }),
        "unequip" => Operation::Unequip(parse_equipment_slot(arguments.next())?),
        "emote" => Operation::Emote(parse_emote(arguments.next())?),
        "chant" => Operation::Chant {
            action: ChantAction::Chant,
            text: parse_chant(arguments.next(), "chant text", ChantText::new)?,
        },
        "item sell" => Operation::Chant {
            action: ChantAction::Sell,
            text: parse_chant(arguments.next(), "item name", ChantText::sell)?,
        },
        "item sell-all" => Operation::Chant {
            action: ChantAction::SellAll,
            text: parse_chant(arguments.next(), "item name", ChantText::sell_all)?,
        },
        "item deposit" => Operation::Chant {
            action: ChantAction::Deposit,
            text: parse_chant(arguments.next(), "item name", ChantText::deposit)?,
        },
        "item withdraw" => Operation::Chant {
            action: ChantAction::Withdraw,
            text: parse_chant(arguments.next(), "item name", ChantText::withdraw)?,
        },
        "item repair" => Operation::Chant {
            action: ChantAction::Repair,
            text: parse_chant(arguments.next(), "item name", ChantText::repair)?,
        },
        "item repair-all" => Operation::Chant {
            action: ChantAction::RepairAll,
            text: ChantText::repair_all(),
        },
        "interact" => Operation::Interact(parse_nonzero_u32(arguments.next(), "object ID")?),
        "dialog select" => Operation::Dialog(parse_dialog_select(&mut arguments)?),
        "dialog input" => Operation::Dialog(parse_dialog_input(&mut arguments)?),
        "dialog previous" => Operation::Dialog(parse_dialog_revision(
            arguments.next(),
            DialogAction::Previous,
        )?),
        "dialog next" => {
            Operation::Dialog(parse_dialog_revision(arguments.next(), DialogAction::Next)?)
        }
        "dialog close" => Operation::Dialog(parse_dialog_revision(
            arguments.next(),
            DialogAction::Close,
        )?),
        "group toggle" => Operation::Group(GroupCommand::Toggle),
        "group invite" => {
            Operation::Group(GroupCommand::Invite(parse_group_name(arguments.next())?))
        }
        "group accept" => Operation::Group(GroupCommand::Respond {
            invitation_id: parse_group_invitation_id(arguments.next())?,
            action: GroupInvitationAction::Accept,
        }),
        "group decline" => Operation::Group(GroupCommand::Respond {
            invitation_id: parse_group_invitation_id(arguments.next())?,
            action: GroupInvitationAction::Decline,
        }),
        "exchange item" => Operation::Exchange(ExchangeCommand::AddItem {
            slot: parse_item_slot(arguments.next())?,
            quantity: arguments
                .next()
                .map(|value| parse_u8(Some(value), "exchange quantity"))
                .transpose()?
                .unwrap_or(1),
        }),
        "exchange gold" => Operation::Exchange(ExchangeCommand::SetGold(parse_u32(
            arguments.next(),
            "exchange gold",
        )?)),
        "exchange accept" => Operation::Exchange(ExchangeCommand::Accept),
        "exchange cancel" => Operation::Exchange(ExchangeCommand::Cancel),
        "who" => Operation::Who,
        "legend" => Operation::Legend,
        "command status" => Operation::CommandStatus(parse_command_id(arguments.next())?),
        "command cancel" => Operation::CommandCancel(parse_command_id(arguments.next())?),
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

fn parse_action(arguments: &mut impl Iterator<Item = OsString>) -> Result<String> {
    let command = arguments
        .next()
        .and_then(|value| value.into_string().ok())
        .ok_or_else(|| ClientError::new(ErrorKind::InvalidArguments, USAGE))?;
    let subcommands: &[&str] = match command.as_str() {
        "tick" => &["health"],
        "skill" => &["use", "swap"],
        "spell" => &["cast", "swap"],
        "item" => &[
            "use",
            "drop",
            "give",
            "swap",
            "pickup",
            "sell",
            "sell-all",
            "deposit",
            "withdraw",
            "repair",
            "repair-all",
        ],
        "gold" => &["drop", "give"],
        "dialog" => &["select", "input", "previous", "next", "close"],
        "group" => &["toggle", "invite", "accept", "decline"],
        "exchange" => &["item", "gold", "accept", "cancel"],
        "command" => &["status", "cancel"],
        "raw" => &["send"],
        _ => return Ok(command),
    };
    let subcommand = arguments
        .next()
        .and_then(|value| value.into_string().ok())
        .ok_or_else(|| invalid_arguments(format!("{command} requires a subcommand")))?;
    if !subcommands.contains(&subcommand.as_str()) {
        return Err(invalid_arguments(format!(
            "unknown {command} subcommand `{subcommand}`"
        )));
    }
    Ok(format!("{command} {subcommand}"))
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

fn parse_raw_packet(arguments: &mut impl Iterator<Item = OsString>) -> Result<RawPacket> {
    let direction = match arguments
        .next()
        .as_deref()
        .and_then(std::ffi::OsStr::to_str)
    {
        Some("client") => RawPacketDirection::Client,
        Some("server") => RawPacketDirection::Server,
        _ => return Err(invalid_arguments("raw direction must be client or server")),
    };
    let command = arguments
        .next()
        .and_then(|value| value.into_string().ok())
        .ok_or_else(|| invalid_arguments("raw command is required"))?;
    let digits = command
        .strip_prefix("0x")
        .or_else(|| command.strip_prefix("0X"))
        .ok_or_else(|| invalid_arguments("raw command must be 0x followed by two hex digits"))?;
    if digits.len() != 2 || !digits.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(invalid_arguments(
            "raw command must be 0x followed by two hex digits",
        ));
    }
    let command = u8::from_str_radix(digits, 16)
        .map_err(|_| invalid_arguments("raw command is not a byte"))?;
    let payload = arguments
        .next()
        .map(|value| {
            value
                .into_string()
                .map_err(|_| invalid_arguments("raw payload must be valid Unicode"))
        })
        .transpose()?
        .unwrap_or_default();
    let mut bytes = Vec::new();
    for token in payload.split_ascii_whitespace() {
        if token.len() != 2 || !token.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(invalid_arguments(format!(
                "raw payload token `{token}` is not exactly two hex digits"
            )));
        }
        if bytes.len() == MAX_RAW_PACKET_PAYLOAD_LEN {
            return Err(invalid_arguments(format!(
                "raw payload exceeds the {MAX_RAW_PACKET_PAYLOAD_LEN}-byte limit"
            )));
        }
        bytes.push(
            u8::from_str_radix(token, 16)
                .map_err(|_| invalid_arguments("raw payload contains a non-hex byte"))?,
        );
    }
    RawPacket::new(direction, command, &bytes)
        .ok_or_else(|| invalid_arguments("raw payload is too large"))
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

fn parse_item_slot(argument: Option<OsString>) -> Result<ItemSlot> {
    let slot = parse_u8(argument, "item slot")?;
    ItemSlot::new(slot).ok_or_else(|| {
        invalid_arguments(format!("item slot must be between 1 and {MAX_ITEM_SLOT}"))
    })
}

fn parse_item_transfer(
    arguments: &mut impl Iterator<Item = OsString>,
    object: bool,
) -> Result<ItemTransfer> {
    let slot = parse_item_slot(arguments.next())?;
    let target = if object {
        TransferTarget::Object(
            std::num::NonZeroU32::new(parse_u32(arguments.next(), "object ID")?)
                .ok_or_else(|| invalid_arguments("object ID must be greater than zero"))?,
        )
    } else {
        TransferTarget::Tile(TilePosition {
            x: parse_coordinate(arguments.next(), "x")?,
            y: parse_coordinate(arguments.next(), "y")?,
        })
    };
    let quantity = arguments
        .next()
        .map(|value| parse_u32(Some(value), "quantity"))
        .transpose()?
        .unwrap_or(1);
    Ok(ItemTransfer {
        slot,
        quantity,
        target,
    })
}

fn parse_gold_transfer(
    arguments: &mut impl Iterator<Item = OsString>,
    object: bool,
) -> Result<GoldTransfer> {
    let amount = parse_u32(arguments.next(), "gold amount")?;
    let target = if object {
        TransferTarget::Object(
            std::num::NonZeroU32::new(parse_u32(arguments.next(), "object ID")?)
                .ok_or_else(|| invalid_arguments("object ID must be greater than zero"))?,
        )
    } else {
        TransferTarget::Tile(TilePosition {
            x: parse_coordinate(arguments.next(), "x")?,
            y: parse_coordinate(arguments.next(), "y")?,
        })
    };
    Ok(GoldTransfer { amount, target })
}

fn parse_equipment_slot(argument: Option<OsString>) -> Result<EquipmentSlot> {
    let slot = parse_u8(argument, "equipment slot")?;
    EquipmentSlot::from_raw(slot)
        .ok_or_else(|| invalid_arguments("equipment slot must be between 1 and 18"))
}

fn parse_dialog_select(arguments: &mut impl Iterator<Item = OsString>) -> Result<DialogCommand> {
    let revision = parse_u32(arguments.next(), "dialog revision")?;
    let index = parse_u16(arguments.next(), "dialog index")?;
    let quantity = arguments
        .next()
        .map(|value| parse_u8(Some(value), "dialog quantity"))
        .transpose()?
        .unwrap_or(1);
    if quantity == 0 {
        return Err(invalid_arguments(
            "dialog quantity must be greater than zero",
        ));
    }
    Ok(DialogCommand {
        revision,
        action: DialogAction::Select { index, quantity },
    })
}

fn parse_dialog_input(arguments: &mut impl Iterator<Item = OsString>) -> Result<DialogCommand> {
    let revision = parse_u32(arguments.next(), "dialog revision")?;
    let input = arguments
        .next()
        .and_then(|value| value.into_string().ok())
        .ok_or_else(|| invalid_arguments("dialog input must be valid Unicode"))?;
    let input = DialogText::new(&input).ok_or_else(|| {
        invalid_arguments(format!(
            "dialog input must contain between 1 and {MAX_DIALOG_INPUT_LEN} ASCII bytes"
        ))
    })?;
    Ok(DialogCommand {
        revision,
        action: DialogAction::Input(input),
    })
}

fn parse_dialog_revision(
    argument: Option<OsString>,
    action: DialogAction,
) -> Result<DialogCommand> {
    Ok(DialogCommand {
        revision: parse_u32(argument, "dialog revision")?,
        action,
    })
}

fn parse_u8(argument: Option<OsString>, name: &str) -> Result<u8> {
    u8::try_from(parse_u32(argument, name)?)
        .map_err(|_| invalid_arguments(format!("{name} must be an unsigned 8-bit integer")))
}

fn parse_u16(argument: Option<OsString>, name: &str) -> Result<u16> {
    u16::try_from(parse_u32(argument, name)?)
        .map_err(|_| invalid_arguments(format!("{name} must be an unsigned 16-bit integer")))
}

fn parse_u32(argument: Option<OsString>, name: &str) -> Result<u32> {
    let argument = argument.ok_or_else(|| invalid_arguments(format!("{name} is required")))?;
    argument
        .to_str()
        .ok_or_else(|| invalid_arguments(format!("{name} must be valid Unicode")))?
        .parse()
        .map_err(|_| invalid_arguments(format!("{name} must be an unsigned 32-bit integer")))
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

fn parse_group_invitation_id(argument: Option<OsString>) -> Result<u32> {
    let invitation_id = parse_u32(argument, "group invitation ID")?;
    if invitation_id == 0 {
        return Err(invalid_arguments(
            "group invitation ID must be greater than zero",
        ));
    }
    Ok(invitation_id)
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

fn parse_chant(
    argument: Option<OsString>,
    name: &str,
    build: impl FnOnce(&str) -> Option<ChantText>,
) -> Result<ChantText> {
    let value = argument
        .and_then(|value| value.into_string().ok())
        .ok_or_else(|| invalid_arguments(format!("{name} must be valid Unicode")))?;
    build(&value).ok_or_else(|| {
        invalid_arguments(format!(
            "{name} must be nonempty ASCII text that fits the chant packet"
        ))
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

fn parse_emote(argument: Option<OsString>) -> Result<u8> {
    let value = argument
        .ok_or_else(|| invalid_arguments("emote requires a name or code"))?
        .into_string()
        .map_err(|_| invalid_arguments("emote name must be valid Unicode"))?;
    let code = value.parse::<u8>().ok().or_else(|| emote_code(&value));
    match code {
        Some(code) if is_client_emote_code(code) => Ok(code),
        _ => Err(invalid_arguments("emote name or code is not recognized")),
    }
}

fn parse_group_name(argument: Option<OsString>) -> Result<GroupText> {
    let name = argument
        .and_then(|value| value.into_string().ok())
        .ok_or_else(|| invalid_arguments("group invite requires an ASCII player name"))?;
    GroupText::new(&name).ok_or_else(|| invalid_arguments("group player name is invalid"))
}

fn invalid_arguments(message: impl Into<String>) -> ClientError {
    ClientError::new(
        ErrorKind::InvalidArguments,
        format!("{}\n{USAGE}", message.into()),
    )
}

#[cfg(test)]
mod tests {
    use super::{ChantAction, Command, Operation, OutputFormat, parse};
    use darpc_model::Direction;
    use darpc_protocol::{
        ChantText, DialogAction, DialogCommand, DialogText, GroupCommand, GroupInvitationAction,
        GroupText, ItemSlot, RawPacket, RawPacketDirection, SkillSlot, SlotSwap, SpellArguments,
        SpellCast, SpellInput, SpellSlot, SpellTarget, WalkTarget,
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
            parse(arguments(&["legend", "--pid", "42"])).unwrap(),
            (
                OutputFormat::Table,
                Command {
                    pid: 42,
                    operation: Operation::Legend,
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
            parse(arguments(&["tick", "health", "--pid", "9"])).unwrap(),
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
            parse(arguments(&["command", "status", "--pid", "9", "17",])).unwrap(),
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
            parse(arguments(&["skill", "use", "--pid", "10", "5"])).unwrap(),
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
                "spell", "cast", "--pid", "10", "7", "--input", "nothing",
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
                "spell",
                "cast",
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
        assert_eq!(
            parse(arguments(&["emote", "--pid", "10", "WaVe"])).unwrap(),
            (
                OutputFormat::Table,
                Command {
                    pid: 10,
                    operation: Operation::Emote(13),
                }
            )
        );
        assert_eq!(
            parse(arguments(&["emote", "--pid", "10", "24"])).unwrap(),
            (
                OutputFormat::Table,
                Command {
                    pid: 10,
                    operation: Operation::Emote(24),
                }
            )
        );
        assert_eq!(
            parse(arguments(&["group", "toggle", "--pid", "10"])).unwrap(),
            (
                OutputFormat::Table,
                Command {
                    pid: 10,
                    operation: Operation::Group(GroupCommand::Toggle),
                }
            )
        );
        assert_eq!(
            parse(arguments(&["group", "invite", "--pid", "10", "ZiLo",])).unwrap(),
            (
                OutputFormat::Table,
                Command {
                    pid: 10,
                    operation: Operation::Group(GroupCommand::Invite(
                        GroupText::new("ZiLo").unwrap()
                    )),
                }
            )
        );
        assert_eq!(
            parse(arguments(&["group", "accept", "--pid", "10", "7",])).unwrap(),
            (
                OutputFormat::Table,
                Command {
                    pid: 10,
                    operation: Operation::Group(GroupCommand::Respond {
                        invitation_id: 7,
                        action: GroupInvitationAction::Accept,
                    }),
                }
            )
        );
    }

    #[test]
    fn parses_raw_packets_and_rejects_malformed_hex() {
        assert_eq!(
            parse(arguments(&[
                "raw", "send", "--pid", "9", "client", "0x7E", "00 03 02",
            ]))
            .unwrap(),
            (
                OutputFormat::Table,
                Command {
                    pid: 9,
                    operation: Operation::Raw(
                        RawPacket::new(RawPacketDirection::Client, 0x7e, &[0x00, 0x03, 0x02],)
                            .unwrap(),
                    ),
                },
            )
        );
        assert!(parse(arguments(&["raw", "send", "--pid", "9", "server", "7E"])).is_err());
        assert!(
            parse(arguments(&[
                "raw", "send", "--pid", "9", "client", "0x7E", "000302",
            ]))
            .is_err()
        );
    }

    #[test]
    fn parses_swaps_interaction_dialog_and_every_group_response() {
        assert_eq!(
            parse(arguments(&["item", "swap", "--pid", "10", "2", "59",]))
                .unwrap()
                .1
                .operation,
            Operation::SwapSlots(SlotSwap::Inventory {
                source: ItemSlot::new(2).unwrap(),
                destination: ItemSlot::new(59).unwrap(),
            })
        );
        assert_eq!(
            parse(arguments(&["skill", "swap", "--pid", "10", "3", "90",]))
                .unwrap()
                .1
                .operation,
            Operation::SwapSlots(SlotSwap::Skillbook {
                source: SkillSlot::new(3).unwrap(),
                destination: SkillSlot::new(90).unwrap(),
            })
        );
        assert_eq!(
            parse(arguments(&["spell", "swap", "--pid", "10", "4", "90",]))
                .unwrap()
                .1
                .operation,
            Operation::SwapSlots(SlotSwap::Spellbook {
                source: SpellSlot::new(4).unwrap(),
                destination: SpellSlot::new(90).unwrap(),
            })
        );
        assert_eq!(
            parse(arguments(&["interact", "--pid", "10", "77"]))
                .unwrap()
                .1
                .operation,
            Operation::Interact(std::num::NonZeroU32::new(77).unwrap())
        );
        assert_eq!(
            parse(arguments(&["dialog", "select", "--pid", "10", "7", "0",]))
                .unwrap()
                .1
                .operation,
            Operation::Dialog(DialogCommand {
                revision: 7,
                action: DialogAction::Select {
                    index: 0,
                    quantity: 1,
                },
            })
        );
        assert_eq!(
            parse(arguments(&[
                "dialog", "select", "--pid", "10", "7", "2", "4",
            ]))
            .unwrap()
            .1
            .operation,
            Operation::Dialog(DialogCommand {
                revision: 7,
                action: DialogAction::Select {
                    index: 2,
                    quantity: 4,
                },
            })
        );
        assert_eq!(
            parse(arguments(&["dialog", "input", "--pid", "10", "8", "ZiLo",]))
                .unwrap()
                .1
                .operation,
            Operation::Dialog(DialogCommand {
                revision: 8,
                action: DialogAction::Input(DialogText::new("ZiLo").unwrap()),
            })
        );
        for (subcommand, action) in [
            ("previous", DialogAction::Previous),
            ("next", DialogAction::Next),
            ("close", DialogAction::Close),
        ] {
            assert_eq!(
                parse(arguments(&["dialog", subcommand, "--pid", "10", "9"]))
                    .unwrap()
                    .1
                    .operation,
                Operation::Dialog(DialogCommand {
                    revision: 9,
                    action,
                })
            );
        }
        assert_eq!(
            parse(arguments(&["group", "decline", "--pid", "10", "11",]))
                .unwrap()
                .1
                .operation,
            Operation::Group(GroupCommand::Respond {
                invitation_id: 11,
                action: GroupInvitationAction::Decline,
            })
        );
    }

    #[test]
    fn parses_chant_and_item_chants_without_normalizing_text() {
        let item = "Dark-Belt  (Fine)";
        let cases = [
            (
                vec!["chant", "--pid", "10", item],
                ChantAction::Chant,
                ChantText::new(item).unwrap(),
            ),
            (
                vec!["item", "sell", "--pid", "10", item],
                ChantAction::Sell,
                ChantText::sell(item).unwrap(),
            ),
            (
                vec!["item", "sell-all", "--pid", "10", item],
                ChantAction::SellAll,
                ChantText::sell_all(item).unwrap(),
            ),
            (
                vec!["item", "deposit", "--pid", "10", item],
                ChantAction::Deposit,
                ChantText::deposit(item).unwrap(),
            ),
            (
                vec!["item", "withdraw", "--pid", "10", item],
                ChantAction::Withdraw,
                ChantText::withdraw(item).unwrap(),
            ),
            (
                vec!["item", "repair", "--pid", "10", item],
                ChantAction::Repair,
                ChantText::repair(item).unwrap(),
            ),
            (
                vec!["item", "repair-all", "--pid", "10"],
                ChantAction::RepairAll,
                ChantText::repair_all(),
            ),
        ];
        for (argv, action, text) in cases {
            assert_eq!(
                parse(arguments(&argv)).unwrap().1.operation,
                Operation::Chant { action, text }
            );
        }
    }

    #[test]
    fn rejects_invalid_pid_and_extra_arguments() {
        assert!(parse(arguments(&["skill-use", "--pid", "1", "1"])).is_err());
        assert!(parse(arguments(&["command-status", "--pid", "1", "1"])).is_err());
        assert!(parse(arguments(&["skill", "cast", "--pid", "1", "1"])).is_err());
        assert!(parse(arguments(&["skill", "--pid", "1", "1"])).is_err());
        assert!(parse(arguments(&["ping", "--pid", "0"])).is_err());
        assert!(parse(arguments(&["hello", "--pid", "1", "extra"])).is_err());
        assert!(parse(arguments(&["turn", "--pid", "1", "up"])).is_err());
        assert!(parse(arguments(&["walk", "--pid", "1", "10"])).is_err());
        assert!(parse(arguments(&["walk", "--pid", "1", "north", "extra"])).is_err());
        assert!(parse(arguments(&["emote", "--pid", "1", "unknown"])).is_err());
        assert!(parse(arguments(&["emote", "--pid", "1", "9"])).is_err());
        assert!(parse(arguments(&["skill", "use", "--pid", "1", "0"])).is_err());
        assert!(parse(arguments(&["skill", "use", "--pid", "1", "91"])).is_err());
        assert!(parse(arguments(&["spell", "cast", "--pid", "1", "0"])).is_err());
        assert!(parse(arguments(&["spell", "cast", "--pid", "1", "1", "--input",])).is_err());
        assert!(parse(arguments(&["group", "invite", "--pid", "1", ""])).is_err());
        assert!(parse(arguments(&["group", "accept", "--pid", "1", "0"])).is_err());
        assert!(parse(arguments(&["item", "swap", "--pid", "1", "0", "2"])).is_err());
        assert!(parse(arguments(&["skill", "swap", "--pid", "1", "1", "91",])).is_err());
        assert!(parse(arguments(&["spell", "swap", "--pid", "1", "91", "1",])).is_err());
        assert!(parse(arguments(&["interact", "--pid", "1", "0"])).is_err());
        assert!(
            parse(arguments(
                &["dialog", "select", "--pid", "1", "7", "65536",]
            ))
            .is_err()
        );
        assert!(
            parse(arguments(&[
                "dialog", "select", "--pid", "1", "7", "0", "0",
            ]))
            .is_err()
        );
        assert!(parse(arguments(&["dialog", "input", "--pid", "1", "7", ""])).is_err());
        assert!(parse(arguments(&["dialog", "input", "--pid", "1", "7", "é"])).is_err());
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
