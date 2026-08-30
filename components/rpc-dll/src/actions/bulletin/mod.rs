use super::{module_base, network, read};
use crate::bulletin::{
    RawBulletin, VIEW_BOARD_COMPOSE, VIEW_ENTRIES, VIEW_ENTRY, VIEW_MAIL_COMPOSE, VIEW_SECTIONS,
};
use darpc_game_client::{
    BULLETIN_DIALOG_VTABLE_RVAS, BULLETIN_LIST_COUNT_RVA, BULLETIN_LIST_ITEM_RVA,
    BULLETIN_LIST_SELECT_RVA, BULLETIN_OPEN_ARTICLE_COMPOSE_RVA, BULLETIN_OPEN_MAIL_COMPOSE_RVA,
    BULLETIN_SCROLL_MAX_RVA, BULLETIN_SCROLL_POSITION_RVA, BULLETIN_SCROLL_SET_RVA,
    BULLETIN_SESSION_BACK_RVA, BULLETIN_SESSION_CLOSE_RVA, BULLETIN_SESSION_FORWARD_RVA,
    BULLETIN_SESSION_POINTER_RVA, BULLETIN_STATIC_TEXT_VTABLE_RVA, BULLETIN_TEXT_COPY_RVA,
    BULLETIN_TEXT_EDIT_VTABLE_RVA, BULLETIN_TEXT_SET_RVA,
};
use darpc_model::BulletinOperation;
use darpc_protocol::{
    BulletinAction, BulletinCommand, BulletinComposeKind, BulletinNavigation, BulletinOpen,
    CommandFailure,
};
use std::{ffi::c_void, mem, ptr};

const SESSION_COUNT_OFFSET: usize = 0x190;
const SESSION_INDEX_OFFSET: usize = 0x191;
const SESSION_DIALOGS_OFFSET: usize = 0x194;
const DIALOG_CONTROLS_OFFSET: usize = 0x594;
const CONTROL_PANE_OFFSET: usize = 0x19C;
const LIST_SELECTION_OFFSET: usize = 0x1C0;
const CONTROL_GET_VTABLE_OFFSET: usize = 0x0C;
const SCROLL_AXIS: i32 = 0;

type SessionFn = unsafe extern "thiscall" fn(*mut c_void) -> u32;
type OpenArticleComposeFn = unsafe extern "thiscall" fn(*mut c_void) -> *mut c_void;
type OpenMailComposeFn = unsafe extern "thiscall" fn(*mut c_void, *const u8) -> *mut c_void;
type ControlGetFn = unsafe extern "thiscall" fn(*mut c_void, i32) -> *mut c_void;
type ListCountFn = unsafe extern "thiscall" fn(*mut c_void) -> i32;
type ListItemFn = unsafe extern "thiscall" fn(*mut c_void, i32) -> *mut c_void;
type ListSelectFn = unsafe extern "thiscall" fn(*mut c_void, i32) -> u32;
type TextCopyFn = unsafe extern "thiscall" fn(*mut c_void, *mut u8, i16) -> i16;
type TextSetFn = unsafe extern "thiscall" fn(*mut c_void, *const u8) -> u32;
type ScrollGetFn = unsafe extern "thiscall" fn(*mut c_void, i32) -> i32;
type ScrollSetFn = unsafe extern "thiscall" fn(*mut c_void, i32, i32) -> u32;

pub(super) fn submit(command: BulletinCommand) -> Result<(), CommandFailure> {
    validate_revision(command)?;
    let local_operation = match command.action {
        BulletinAction::Open(open) => {
            open_bulletin(open)?;
            None
        }
        BulletinAction::OpenSection { section_id } => {
            require_current(|state| state.has_section(section_id))?;
            request_entries(section_id, 0x7FFF)?;
            None
        }
        BulletinAction::SelectSection { section_id } => {
            require_current(|state| state.has_section(section_id))?;
            select_id(DialogKind::Sections, section_id as i16)?;
            Some(BulletinOperation::SelectSection)
        }
        BulletinAction::OpenEntry { entry_id } => {
            let section_id = current_value(|state| {
                (state.view() == VIEW_ENTRIES && state.has_entry(entry_id))
                    .then_some(state.section_id())
            })?;
            request_entry(section_id, entry_id, 0)?;
            None
        }
        BulletinAction::SelectEntry { entry_id } => {
            require_current(|state| state.view() == VIEW_ENTRIES && state.has_entry(entry_id))?;
            let dialog = current_dialog()?;
            select_id(dialog.kind, entry_id)?;
            Some(BulletinOperation::SelectEntry)
        }
        BulletinAction::LoadOlder => {
            let (section_id, cursor) = current_value(|state| {
                (state.view() == VIEW_ENTRIES)
                    .then(|| state.oldest_entry_id())
                    .flatten()
                    .and_then(|oldest| oldest.checked_sub(1))
                    .filter(|cursor| *cursor > 0)
                    .map(|cursor| (state.section_id(), cursor))
            })?;
            request_entries(section_id, cursor)?;
            None
        }
        BulletinAction::Scroll { position } => {
            scroll(position)?;
            Some(BulletinOperation::Scroll)
        }
        BulletinAction::Navigate(navigation) => match navigation {
            BulletinNavigation::Back => {
                if !current_dialog()?.can_go_back {
                    return Err(CommandFailure::InvalidState);
                }
                call_session(BULLETIN_SESSION_BACK_RVA)?;
                Some(BulletinOperation::Back)
            }
            BulletinNavigation::Forward => {
                if !current_dialog()?.can_go_forward {
                    return Err(CommandFailure::InvalidState);
                }
                call_session(BULLETIN_SESSION_FORWARD_RVA)?;
                Some(BulletinOperation::Forward)
            }
            BulletinNavigation::PreviousEntry => {
                navigate_entry(0xFF)?;
                None
            }
            BulletinNavigation::NextEntry => {
                navigate_entry(1)?;
                None
            }
        },
        BulletinAction::BeginCompose(kind) => {
            begin_compose(kind)?;
            Some(match kind {
                BulletinComposeKind::BoardPost => BulletinOperation::BeginBoardPost,
                BulletinComposeKind::PlayerMail => BulletinOperation::BeginPlayerMail,
                BulletinComposeKind::Reply => BulletinOperation::BeginReply,
            })
        }
        BulletinAction::UpdateBoardPost { subject, body } => {
            update_board_post(subject.as_bytes(), body.as_bytes())?;
            Some(BulletinOperation::UpdateCompose)
        }
        BulletinAction::UpdatePlayerMail {
            recipient,
            subject,
            body,
        } => {
            update_player_mail(recipient.as_bytes(), subject.as_bytes(), body.as_bytes())?;
            Some(BulletinOperation::UpdateCompose)
        }
        BulletinAction::SubmitCompose => {
            submit_compose()?;
            None
        }
        BulletinAction::DeleteEntry { entry_id } => {
            delete_entry(entry_id)?;
            None
        }
        BulletinAction::HighlightEntry { entry_id } => {
            highlight_entry(entry_id)?;
            None
        }
        BulletinAction::Close => {
            call_session(BULLETIN_SESSION_CLOSE_RVA)?;
            Some(BulletinOperation::Close)
        }
    };
    if let Some(operation) = local_operation {
        let _ = crate::bulletin::refresh_ui();
        crate::state::observe_bulletin_submission(operation, darpc_win32::pipe::sender_tick_ms());
    }
    Ok(())
}

pub(crate) fn observe_ui(raw: &mut RawBulletin) -> Result<bool, ()> {
    let dialog = current_dialog().map_err(|_| ())?;
    let mut changed = raw.set_ui_navigation(dialog.can_go_back, dialog.can_go_forward);
    changed |= raw.set_ui_view(dialog.kind.view());
    match dialog.kind {
        DialogKind::Sections => {
            let control = control(dialog.pointer, 2).map_err(|_| ())?;
            changed |= raw.set_selected_section(selected_id(control).map(|id| id as u16));
            changed |= observe_viewport(raw, control)?;
        }
        DialogKind::ArticleList | DialogKind::MailList => {
            let index = if dialog.kind == DialogKind::ArticleList {
                5
            } else {
                6
            };
            let control = control(dialog.pointer, index).map_err(|_| ())?;
            changed |= raw.set_selected_entry(selected_id(control));
            changed |= observe_viewport(raw, control)?;
        }
        DialogKind::Article | DialogKind::Mail => {
            let index = if dialog.kind == DialogKind::Article {
                8
            } else {
                9
            };
            changed |= observe_viewport(raw, control(dialog.pointer, index).map_err(|_| ())?)?;
        }
        DialogKind::NewArticle => {
            let mut author = [0; 256];
            let mut subject = [0; 256];
            let mut body = [0; darpc_protocol::MAX_BULLETIN_COMPOSE_BODY_LEN + 1];
            let author_length =
                copy_text(control(dialog.pointer, 2).map_err(|_| ())?, &mut author)?;
            let subject_length =
                copy_text(control(dialog.pointer, 3).map_err(|_| ())?, &mut subject)?;
            let body_length = copy_text(control(dialog.pointer, 4).map_err(|_| ())?, &mut body)?;
            changed |= raw.set_compose_author(&author[..author_length]);
            changed |= raw.set_compose_subject(&subject[..subject_length]);
            changed |= raw.set_compose_body(&body[..body_length]);
        }
        DialogKind::NewMail => {
            let recipient_control = control(dialog.pointer, 2).map_err(|_| ())?;
            let mut recipient = [0; 256];
            let mut subject = [0; 256];
            let mut body = [0; darpc_protocol::MAX_BULLETIN_COMPOSE_BODY_LEN + 1];
            let recipient_length = copy_text(recipient_control, &mut recipient)?;
            let subject_length =
                copy_text(control(dialog.pointer, 3).map_err(|_| ())?, &mut subject)?;
            let body_length = copy_text(control(dialog.pointer, 4).map_err(|_| ())?, &mut body)?;
            changed |= raw.set_compose_recipient(
                &recipient[..recipient_length],
                text_control_editable(dialog.module_base, recipient_control).map_err(|_| ())?,
            );
            changed |= raw.set_compose_subject(&subject[..subject_length]);
            changed |= raw.set_compose_body(&body[..body_length]);
        }
    }
    Ok(changed)
}

fn validate_revision(command: BulletinCommand) -> Result<(), CommandFailure> {
    if matches!(command.action, BulletinAction::Open(_)) {
        return (command.revision == 0)
            .then_some(())
            .ok_or(CommandFailure::InvalidArguments);
    }
    (crate::bulletin::current_revision() == Some(command.revision))
        .then_some(())
        .ok_or(CommandFailure::InvalidState)
}

fn open_bulletin(open: BulletinOpen) -> Result<(), CommandFailure> {
    match open {
        BulletinOpen::ServerList => network::submit(&[0x3B, 1]),
        BulletinOpen::WorldTile { x, y } => {
            let mut packet = [0x43, 3, 0, 0, 0, 0, 1];
            packet[2..4].copy_from_slice(&x.to_be_bytes());
            packet[4..6].copy_from_slice(&y.to_be_bytes());
            network::submit(&packet)
        }
    }
}

fn request_entries(section_id: u16, cursor: i16) -> Result<(), CommandFailure> {
    let mut packet = [0x3B, 2, 0, 0, 0, 0, 0xF0];
    packet[2..4].copy_from_slice(&section_id.to_be_bytes());
    packet[4..6].copy_from_slice(&cursor.to_be_bytes());
    network::submit(&packet)
}

fn request_entry(section_id: u16, entry_id: i16, navigation: u8) -> Result<(), CommandFailure> {
    let mut packet = [0x3B, 3, 0, 0, 0, 0, navigation];
    packet[2..4].copy_from_slice(&section_id.to_be_bytes());
    packet[4..6].copy_from_slice(&entry_id.to_be_bytes());
    network::submit(&packet)
}

fn navigate_entry(navigation: u8) -> Result<(), CommandFailure> {
    let (section, entry) = current_value(|state| {
        (state.view() == VIEW_ENTRY).then_some((state.section_id(), state.entry_id()))
    })?;
    request_entry(section, entry, navigation)
}

fn delete_entry(entry_id: i16) -> Result<(), CommandFailure> {
    let (section, mail) = current_value(|state| {
        state
            .has_entry(entry_id)
            .then_some((state.section_id(), state.is_mailbox()))
    })?;
    let mut packet = [0x3B, 5, 0, 0, 0, 0, 0];
    packet[2..4].copy_from_slice(&section.to_be_bytes());
    packet[4..6].copy_from_slice(&entry_id.to_be_bytes());
    network::submit(&packet[..if mail { 7 } else { 6 }])
}

fn highlight_entry(entry_id: i16) -> Result<(), CommandFailure> {
    let section = current_value(|state| {
        (state.has_entry(entry_id) && !state.is_mailbox()).then_some(state.section_id())
    })?;
    let mut packet = [0x3B, 7, 0, 0, 0, 0];
    packet[2..4].copy_from_slice(&section.to_be_bytes());
    packet[4..6].copy_from_slice(&entry_id.to_be_bytes());
    network::submit(&packet)
}

fn begin_compose(kind: BulletinComposeKind) -> Result<(), CommandFailure> {
    let dialog = current_dialog()?;
    let session = session()?;
    match kind {
        BulletinComposeKind::BoardPost => {
            require_current(|state| state.view() == VIEW_ENTRIES && !state.is_mailbox())?;
            call_open_article(dialog.module_base, session)?;
        }
        BulletinComposeKind::PlayerMail => {
            require_current(|state| state.view() == VIEW_ENTRIES && state.is_mailbox())?;
            call_open_mail(dialog.module_base, session, ptr::null())?;
        }
        BulletinComposeKind::Reply => {
            let mut recipient = [0; 256];
            current_value(|state| {
                let author = state.entry_author();
                (state.view() == VIEW_ENTRY && !author.is_empty()).then(|| {
                    recipient[..author.len()].copy_from_slice(author);
                })
            })?;
            call_open_mail(dialog.module_base, session, recipient.as_ptr())?;
        }
    }
    Ok(())
}

fn update_board_post(subject: &[u8], body: &[u8]) -> Result<(), CommandFailure> {
    let dialog = current_dialog()?;
    if dialog.kind != DialogKind::NewArticle {
        return Err(CommandFailure::InvalidState);
    }
    set_text(dialog, 3, subject, true)?;
    set_text(dialog, 4, body, true)
}

fn update_player_mail(recipient: &[u8], subject: &[u8], body: &[u8]) -> Result<(), CommandFailure> {
    let dialog = current_dialog()?;
    if dialog.kind != DialogKind::NewMail {
        return Err(CommandFailure::InvalidState);
    }
    let recipient_control = control(dialog.pointer, 2)?;
    let recipient_editable = text_control_editable(dialog.module_base, recipient_control)?;
    if recipient_editable {
        set_control_text(dialog.module_base, recipient_control, recipient)?;
    } else if crate::bulletin::with_current(|state| state.compose_recipient() != recipient)
        .unwrap_or(true)
    {
        return Err(CommandFailure::InvalidArguments);
    }
    set_text(dialog, 3, subject, true)?;
    set_text(dialog, 4, body, true)
}

fn submit_compose() -> Result<(), CommandFailure> {
    crate::bulletin::refresh_ui().map_err(|_| CommandFailure::InvalidState)?;
    let mut packet = [0; 2
        + 2
        + 1
        + darpc_protocol::MAX_BULLETIN_RECIPIENT_LEN
        + 1
        + darpc_protocol::MAX_BULLETIN_SUBJECT_LEN
        + 2
        + darpc_protocol::MAX_BULLETIN_COMPOSE_BODY_LEN];
    let length = current_value(|state| {
        let view = state.view();
        matches!(view, VIEW_BOARD_COMPOSE | VIEW_MAIL_COMPOSE).then(|| {
            packet[0] = 0x3B;
            packet[1] = if view == VIEW_BOARD_COMPOSE { 4 } else { 6 };
            packet[2..4].copy_from_slice(&state.section_id().to_be_bytes());
            let mut length = 4;
            if view == VIEW_MAIL_COMPOSE {
                length = append_string8(&mut packet, length, state.compose_recipient())?;
            }
            length = append_string8(&mut packet, length, state.compose_subject())?;
            append_string16(&mut packet, length, state.compose_body())
        })
    })??;
    network::submit(&packet[..length])
}

fn scroll(position: i32) -> Result<(), CommandFailure> {
    let dialog = current_dialog()?;
    let index = dialog.kind.scroll_control();
    let control = control(dialog.pointer, index)?;
    let maximum = scroll_max(dialog.module_base, control)?;
    if position < 0 || position > maximum {
        return Err(CommandFailure::InvalidArguments);
    }
    // SAFETY: the exact live bulletin dialog, control lookup, supported client
    // fingerprint, and main-thread command boundary establish this control ABI.
    unsafe {
        function::<ScrollSetFn>(dialog.module_base, BULLETIN_SCROLL_SET_RVA)(
            control as *mut c_void,
            SCROLL_AXIS,
            position,
        );
    }
    Ok(())
}

fn select_id(kind: DialogKind, wanted: i16) -> Result<(), CommandFailure> {
    let dialog = current_dialog()?;
    if dialog.kind != kind {
        return Err(CommandFailure::InvalidState);
    }
    let index = if kind == DialogKind::Sections {
        2
    } else if kind == DialogKind::ArticleList {
        5
    } else {
        6
    };
    let control = control(dialog.pointer, index)?;
    let pane = list_pane(control)?;
    let count = list_count(dialog.module_base, pane)?;
    for row in 0..count {
        let item = list_item(dialog.module_base, pane, row)?;
        if read::<i16>(item).ok_or(CommandFailure::InvalidState)? == wanted {
            // SAFETY: the validated list pane and bounded row establish the
            // supported client's list-selection ABI.
            unsafe {
                function::<ListSelectFn>(dialog.module_base, BULLETIN_LIST_SELECT_RVA)(
                    pane as *mut c_void,
                    row,
                );
            }
            return Ok(());
        }
    }
    Err(CommandFailure::InvalidTarget)
}

fn observe_viewport(raw: &mut RawBulletin, control: usize) -> Result<bool, ()> {
    let module_base = module_base().map_err(|_| ())?;
    let position = scroll_position(module_base, control).map_err(|_| ())?;
    let maximum = scroll_max(module_base, control).map_err(|_| ())?;
    Ok(raw.set_viewport(position, maximum))
}

fn selected_id(control: usize) -> Option<i16> {
    let module_base = module_base().ok()?;
    let pane = list_pane(control).ok()?;
    let selected = read::<i32>(pane + LIST_SELECTION_OFFSET)?;
    if selected < 0 || selected >= list_count(module_base, pane).ok()? {
        return None;
    }
    read::<i16>(list_item(module_base, pane, selected).ok()?)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DialogKind {
    Sections,
    ArticleList,
    Article,
    NewArticle,
    MailList,
    Mail,
    NewMail,
}

impl DialogKind {
    const fn view(self) -> u8 {
        match self {
            Self::Sections => VIEW_SECTIONS,
            Self::ArticleList | Self::MailList => VIEW_ENTRIES,
            Self::Article | Self::Mail => VIEW_ENTRY,
            Self::NewArticle => VIEW_BOARD_COMPOSE,
            Self::NewMail => VIEW_MAIL_COMPOSE,
        }
    }

    const fn scroll_control(self) -> i32 {
        match self {
            Self::Sections => 2,
            Self::ArticleList => 5,
            Self::MailList => 6,
            Self::Article => 8,
            Self::Mail => 9,
            Self::NewArticle | Self::NewMail => 4,
        }
    }
}

#[derive(Clone, Copy)]
struct CurrentDialog {
    module_base: usize,
    pointer: usize,
    kind: DialogKind,
    can_go_back: bool,
    can_go_forward: bool,
}

fn current_dialog() -> Result<CurrentDialog, CommandFailure> {
    let module_base = module_base()?;
    let session = session_at(module_base)?;
    let count = usize::from(
        read::<u8>(session + SESSION_COUNT_OFFSET).ok_or(CommandFailure::InvalidState)?,
    );
    let index = read::<i8>(session + SESSION_INDEX_OFFSET).ok_or(CommandFailure::InvalidState)?;
    if index < 0 || usize::from(index as u8) >= count || count > 10 {
        return Err(CommandFailure::InvalidState);
    }
    let index = usize::from(index as u8);
    let pointer = read::<u32>(session + SESSION_DIALOGS_OFFSET + index * 4)
        .filter(|pointer| *pointer != 0)
        .ok_or(CommandFailure::InvalidState)? as usize;
    let vtable = read::<u32>(pointer).ok_or(CommandFailure::InvalidState)? as usize;
    let relative = vtable
        .checked_sub(module_base)
        .ok_or(CommandFailure::InvalidState)?;
    let kind = match BULLETIN_DIALOG_VTABLE_RVAS
        .iter()
        .position(|rva| *rva == relative)
    {
        Some(0) => DialogKind::Sections,
        Some(1) => DialogKind::ArticleList,
        Some(2) => DialogKind::Article,
        Some(3) => DialogKind::NewArticle,
        Some(4) => DialogKind::MailList,
        Some(5) => DialogKind::Mail,
        Some(6) => DialogKind::NewMail,
        _ => return Err(CommandFailure::InvalidState),
    };
    Ok(CurrentDialog {
        module_base,
        pointer,
        kind,
        can_go_back: index != 0,
        can_go_forward: index + 1 < count,
    })
}

fn session() -> Result<usize, CommandFailure> {
    session_at(module_base()?)
}

fn session_at(module_base: usize) -> Result<usize, CommandFailure> {
    read::<u32>(module_base + BULLETIN_SESSION_POINTER_RVA)
        .filter(|pointer| *pointer != 0)
        .map(|pointer| pointer as usize)
        .ok_or(CommandFailure::InvalidState)
}

fn control(dialog: usize, index: i32) -> Result<usize, CommandFailure> {
    let collection = read::<u32>(dialog + DIALOG_CONTROLS_OFFSET)
        .filter(|pointer| *pointer != 0)
        .ok_or(CommandFailure::InvalidState)? as usize;
    let vtable = read::<u32>(collection).ok_or(CommandFailure::InvalidState)? as usize;
    let function = read::<u32>(vtable + CONTROL_GET_VTABLE_OFFSET)
        .filter(|pointer| *pointer != 0)
        .ok_or(CommandFailure::InvalidState)? as usize;
    // SAFETY: an exact live bulletin dialog owns this control collection, the
    // supported fingerprint fixes its vtable, and commands run on the main thread.
    let pointer = unsafe {
        let function: ControlGetFn = mem::transmute(function);
        function(collection as *mut c_void, index)
    };
    (!pointer.is_null())
        .then_some(pointer as usize)
        .ok_or(CommandFailure::InvalidState)
}

fn control_vtable(control: usize) -> Result<usize, ()> {
    read::<u32>(control).map(|value| value as usize).ok_or(())
}

fn text_control_editable(module_base: usize, control: usize) -> Result<bool, CommandFailure> {
    match control_vtable(control).map_err(|_| CommandFailure::InvalidState)? {
        vtable if vtable == module_base + BULLETIN_TEXT_EDIT_VTABLE_RVA => Ok(true),
        vtable if vtable == module_base + BULLETIN_STATIC_TEXT_VTABLE_RVA => Ok(false),
        _ => Err(CommandFailure::InvalidState),
    }
}

fn list_pane(control: usize) -> Result<usize, CommandFailure> {
    read::<u32>(control + CONTROL_PANE_OFFSET)
        .filter(|pointer| *pointer != 0)
        .map(|pointer| pointer as usize)
        .ok_or(CommandFailure::InvalidState)
}

fn list_count(module_base: usize, pane: usize) -> Result<i32, CommandFailure> {
    // SAFETY: the control lookup returned the inner pane of an exact bulletin
    // list control and the supported fingerprint fixes this ABI.
    let count = unsafe {
        function::<ListCountFn>(module_base, BULLETIN_LIST_COUNT_RVA)(pane as *mut c_void)
    };
    (0..=crate::bulletin::MAX_BULLETIN_ENTRIES as i32)
        .contains(&count)
        .then_some(count)
        .ok_or(CommandFailure::InvalidState)
}

fn list_item(module_base: usize, pane: usize, index: i32) -> Result<usize, CommandFailure> {
    // SAFETY: list_count validated the row index domain and the supported
    // fingerprint fixes this ABI.
    let pointer = unsafe {
        function::<ListItemFn>(module_base, BULLETIN_LIST_ITEM_RVA)(pane as *mut c_void, index)
    };
    (!pointer.is_null())
        .then_some(pointer as usize)
        .ok_or(CommandFailure::InvalidState)
}

fn copy_text<const N: usize>(control: usize, output: &mut [u8; N]) -> Result<usize, ()> {
    let module_base = module_base().map_err(|_| ())?;
    let maximum = i16::try_from(N.saturating_sub(1)).map_err(|_| ())?;
    // SAFETY: the control came from an exact bulletin dialog, the destination
    // is writable and NUL-capable, and the supported fingerprint fixes this ABI.
    let length = unsafe {
        function::<TextCopyFn>(module_base, BULLETIN_TEXT_COPY_RVA)(
            control as *mut c_void,
            output.as_mut_ptr(),
            maximum,
        )
    };
    usize::try_from(length)
        .ok()
        .filter(|length| *length < N)
        .ok_or(())
}

fn set_text(
    dialog: CurrentDialog,
    index: i32,
    value: &[u8],
    require_editable: bool,
) -> Result<(), CommandFailure> {
    let control = control(dialog.pointer, index)?;
    if require_editable
        && control_vtable(control).map_err(|_| CommandFailure::InvalidState)?
            != dialog.module_base + BULLETIN_TEXT_EDIT_VTABLE_RVA
    {
        return Err(CommandFailure::InvalidState);
    }
    set_control_text(dialog.module_base, control, value)
}

fn set_control_text(
    module_base: usize,
    control: usize,
    value: &[u8],
) -> Result<(), CommandFailure> {
    if value.contains(&0) || value.len() > darpc_protocol::MAX_BULLETIN_COMPOSE_BODY_LEN {
        return Err(CommandFailure::InvalidArguments);
    }
    let mut text = [0; darpc_protocol::MAX_BULLETIN_COMPOSE_BODY_LEN + 1];
    text[..value.len()].copy_from_slice(value);
    // SAFETY: the exact text control is live, text is bounded and NUL
    // terminated, and the supported fingerprint fixes this main-thread ABI.
    unsafe {
        function::<TextSetFn>(module_base, BULLETIN_TEXT_SET_RVA)(
            control as *mut c_void,
            text.as_ptr(),
        );
    }
    Ok(())
}

fn scroll_position(module_base: usize, control: usize) -> Result<i32, CommandFailure> {
    // SAFETY: the control is a validated bulletin scroll control.
    Ok(unsafe {
        function::<ScrollGetFn>(module_base, BULLETIN_SCROLL_POSITION_RVA)(
            control as *mut c_void,
            SCROLL_AXIS,
        )
    })
}

fn scroll_max(module_base: usize, control: usize) -> Result<i32, CommandFailure> {
    // SAFETY: the control is a validated bulletin scroll control.
    let maximum = unsafe {
        function::<ScrollGetFn>(module_base, BULLETIN_SCROLL_MAX_RVA)(
            control as *mut c_void,
            SCROLL_AXIS,
        )
    };
    (maximum >= 0)
        .then_some(maximum)
        .ok_or(CommandFailure::InvalidState)
}

fn call_session(rva: usize) -> Result<(), CommandFailure> {
    let module_base = module_base()?;
    let session = session_at(module_base)?;
    // SAFETY: the session singleton is live, the command runs on the main
    // thread, and the supported fingerprint fixes this ABI.
    unsafe {
        function::<SessionFn>(module_base, rva)(session as *mut c_void);
    }
    Ok(())
}

fn call_open_article(module_base: usize, session: usize) -> Result<(), CommandFailure> {
    // SAFETY: the session singleton is live and the supported fingerprint
    // fixes this main-thread ABI.
    let result = unsafe {
        function::<OpenArticleComposeFn>(module_base, BULLETIN_OPEN_ARTICLE_COMPOSE_RVA)(
            session as *mut c_void,
        )
    };
    (!result.is_null())
        .then_some(())
        .ok_or(CommandFailure::Internal)
}

fn call_open_mail(
    module_base: usize,
    session: usize,
    recipient: *const u8,
) -> Result<(), CommandFailure> {
    // SAFETY: recipient is null or NUL-terminated, the session singleton is
    // live, and the supported fingerprint fixes this main-thread ABI.
    let result = unsafe {
        function::<OpenMailComposeFn>(module_base, BULLETIN_OPEN_MAIL_COMPOSE_RVA)(
            session as *mut c_void,
            recipient,
        )
    };
    (!result.is_null())
        .then_some(())
        .ok_or(CommandFailure::Internal)
}

fn append_string8(output: &mut [u8], offset: usize, value: &[u8]) -> Result<usize, CommandFailure> {
    let length = u8::try_from(value.len()).map_err(|_| CommandFailure::InvalidArguments)?;
    let start = offset.checked_add(1).ok_or(CommandFailure::Internal)?;
    let end = start
        .checked_add(value.len())
        .ok_or(CommandFailure::Internal)?;
    *output.get_mut(offset).ok_or(CommandFailure::Internal)? = length;
    output
        .get_mut(start..end)
        .ok_or(CommandFailure::Internal)?
        .copy_from_slice(value);
    Ok(end)
}

fn append_string16(
    output: &mut [u8],
    offset: usize,
    value: &[u8],
) -> Result<usize, CommandFailure> {
    let length = u16::try_from(value.len()).map_err(|_| CommandFailure::InvalidArguments)?;
    let start = offset.checked_add(2).ok_or(CommandFailure::Internal)?;
    let end = start
        .checked_add(value.len())
        .ok_or(CommandFailure::Internal)?;
    output
        .get_mut(offset..start)
        .ok_or(CommandFailure::Internal)?
        .copy_from_slice(&length.to_be_bytes());
    output
        .get_mut(start..end)
        .ok_or(CommandFailure::Internal)?
        .copy_from_slice(value);
    Ok(end)
}

fn require_current(predicate: impl FnOnce(&RawBulletin) -> bool) -> Result<(), CommandFailure> {
    crate::bulletin::with_current(predicate)
        .filter(|accepted| *accepted)
        .map(|_| ())
        .ok_or(CommandFailure::InvalidState)
}

fn current_value<T>(
    operation: impl FnOnce(&RawBulletin) -> Option<T>,
) -> Result<T, CommandFailure> {
    crate::bulletin::with_current(operation)
        .flatten()
        .ok_or(CommandFailure::InvalidState)
}

// SAFETY: callers only use this with validated executable-relative function
// addresses and the exact native function type established above.
unsafe fn function<T: Copy>(module_base: usize, rva: usize) -> T {
    // SAFETY: upheld by the caller and the supported-client fingerprint.
    unsafe { mem::transmute_copy(&(module_base + rva)) }
}
