use darpc_model::{
    BulletinCompose, BulletinEntry, BulletinEntrySummary, BulletinOperation, BulletinPagination,
    BulletinSection, BulletinSectionKind, BulletinSource, BulletinState, BulletinView,
};
use serde_json::{Value, json};
use std::fmt::Write as _;

use crate::output::json_string;

pub(super) fn render_human(output: &mut String, state: Option<&BulletinState>) {
    let Some(state) = state else {
        output.push_str("\nbulletin: unavailable");
        return;
    };
    let pending = state.pending.map_or("none", operation);
    let _ = write!(
        output,
        "\nbulletin: revision={} pending={} can_go_back={} can_go_forward={}",
        state.revision, pending, state.can_go_back, state.can_go_forward,
    );
    match &state.view {
        BulletinView::Sections {
            heading,
            sections,
            selected_section_id,
            viewport,
            truncated,
        } => {
            let _ = write!(
                output,
                "\nbulletin_sections: heading={} selected_section_id={} position={} maximum={} truncated={}",
                json_string(heading),
                optional_i64(selected_section_id.map(i64::from)),
                viewport.position,
                viewport.maximum,
                truncated,
            );
            for section in sections {
                render_section(output, section);
            }
        }
        BulletinView::Entries {
            section,
            entries,
            selected_entry_id,
            viewport,
            pagination,
            truncated,
        } => {
            render_section(output, section);
            let _ = write!(
                output,
                "\nbulletin_entries: selected_entry_id={} position={} maximum={} pagination={} truncated={}",
                optional_i64(selected_entry_id.map(i64::from)),
                viewport.position,
                viewport.maximum,
                pagination_name(*pagination),
                truncated,
            );
            for entry in entries {
                render_entry_summary(output, entry);
            }
        }
        BulletinView::Entry {
            section,
            entry,
            viewport,
        } => {
            render_section(output, section);
            render_entry(output, entry, viewport.position, viewport.maximum);
        }
        BulletinView::Compose(compose) => render_compose(output, compose),
    }
    if let Some(result) = &state.last_operation_result {
        let _ = write!(
            output,
            "\nbulletin_result: operation={} raw_status={} message={}",
            operation(result.operation),
            result.raw_status,
            result
                .message
                .as_deref()
                .map_or("null".to_owned(), json_string),
        );
    }
}

pub(super) fn value(state: &BulletinState) -> Value {
    json!({
        "revision": state.revision,
        "pending": state.pending.map(operation),
        "last_operation_result": state.last_operation_result.as_ref().map(|result| json!({
            "operation": operation(result.operation),
            "raw_status": result.raw_status,
            "message": result.message,
        })),
        "can_go_back": state.can_go_back,
        "can_go_forward": state.can_go_forward,
        "view": view_value(&state.view),
    })
}

fn view_value(view: &BulletinView) -> Value {
    match view {
        BulletinView::Sections {
            heading,
            sections,
            selected_section_id,
            viewport,
            truncated,
        } => json!({
            "kind": "sections",
            "heading": heading,
            "sections": sections.iter().map(section_value).collect::<Vec<_>>(),
            "selected_section_id": selected_section_id,
            "viewport": { "position": viewport.position, "maximum": viewport.maximum },
            "truncated": truncated,
        }),
        BulletinView::Entries {
            section,
            entries,
            selected_entry_id,
            viewport,
            pagination,
            truncated,
        } => json!({
            "kind": "entries",
            "section": section_value(section),
            "entries": entries.iter().map(entry_summary_value).collect::<Vec<_>>(),
            "selected_entry_id": selected_entry_id,
            "viewport": { "position": viewport.position, "maximum": viewport.maximum },
            "pagination": pagination_name(*pagination),
            "truncated": truncated,
        }),
        BulletinView::Entry {
            section,
            entry,
            viewport,
        } => json!({
            "kind": "entry",
            "section": section_value(section),
            "entry": entry_value(entry),
            "viewport": { "position": viewport.position, "maximum": viewport.maximum },
        }),
        BulletinView::Compose(BulletinCompose::BoardPost {
            section,
            author,
            subject,
            body,
        }) => json!({
            "kind": "compose_board_post",
            "section": section_value(section),
            "author": author,
            "subject": subject,
            "body": body,
        }),
        BulletinView::Compose(BulletinCompose::PlayerMail {
            mailbox,
            recipient,
            recipient_editable,
            subject,
            body,
        }) => json!({
            "kind": "compose_player_mail",
            "mailbox": section_value(mailbox),
            "recipient": recipient,
            "recipient_editable": recipient_editable,
            "subject": subject,
            "body": body,
        }),
    }
}

fn render_section(output: &mut String, section: &BulletinSection) {
    let _ = write!(
        output,
        "\nbulletin_section: id={} name={} kind={} source={} source_raw={}",
        section.id,
        json_string(&section.name),
        section_kind(section.kind),
        source(section.source),
        section.source.raw(),
    );
}

fn render_entry_summary(output: &mut String, entry: &BulletinEntrySummary) {
    let _ = write!(
        output,
        "\nbulletin_entry: id={} flags={} author={} month={} day={} subject={}",
        entry.id,
        entry.flags,
        json_string(&entry.author),
        entry.month,
        entry.day,
        json_string(&entry.subject),
    );
}

fn render_entry(output: &mut String, entry: &BulletinEntry, position: i32, maximum: i32) {
    let _ = write!(
        output,
        "\nbulletin_entry: id={} flags={} author={} month={} day={} subject={} body={} navigation_flags={} unknown_before_id={} position={} maximum={}",
        entry.id,
        entry
            .flags
            .map_or("null".to_owned(), |value| value.to_string()),
        json_string(&entry.author),
        entry.month,
        entry.day,
        json_string(&entry.subject),
        json_string(&entry.body),
        entry.navigation_flags,
        entry.unknown_before_id,
        position,
        maximum,
    );
}

fn render_compose(output: &mut String, compose: &BulletinCompose) {
    match compose {
        BulletinCompose::BoardPost {
            section,
            author,
            subject,
            body,
        } => {
            render_section(output, section);
            let _ = write!(
                output,
                "\nbulletin_compose: kind=board_post author={} subject={} body={}",
                json_string(author),
                json_string(subject),
                json_string(body),
            );
        }
        BulletinCompose::PlayerMail {
            mailbox,
            recipient,
            recipient_editable,
            subject,
            body,
        } => {
            render_section(output, mailbox);
            let _ = write!(
                output,
                "\nbulletin_compose: kind=player_mail recipient={} recipient_editable={} subject={} body={}",
                json_string(recipient),
                recipient_editable,
                json_string(subject),
                json_string(body),
            );
        }
    }
}

fn section_value(section: &BulletinSection) -> Value {
    json!({
        "id": section.id,
        "name": section.name,
        "kind": section_kind(section.kind),
        "source": {
            "kind": source(section.source),
            "raw": section.source.raw(),
        },
    })
}

fn entry_summary_value(entry: &BulletinEntrySummary) -> Value {
    json!({
        "id": entry.id,
        "flags": entry.flags,
        "author": entry.author,
        "month": entry.month,
        "day": entry.day,
        "subject": entry.subject,
    })
}

fn entry_value(entry: &BulletinEntry) -> Value {
    json!({
        "id": entry.id,
        "flags": entry.flags,
        "author": entry.author,
        "month": entry.month,
        "day": entry.day,
        "subject": entry.subject,
        "body": entry.body,
        "navigation_flags": entry.navigation_flags,
        "unknown_before_id": entry.unknown_before_id,
    })
}

fn source(value: BulletinSource) -> &'static str {
    match value {
        BulletinSource::Global => "global",
        BulletinSource::Clicked => "clicked",
        BulletinSource::Mail => "mail",
        BulletinSource::Unknown(_) => "unknown",
    }
}

fn section_kind(value: BulletinSectionKind) -> &'static str {
    match value {
        BulletinSectionKind::Board => "board",
        BulletinSectionKind::Mailbox => "mailbox",
        BulletinSectionKind::Unknown => "unknown",
    }
}

fn pagination_name(value: BulletinPagination) -> &'static str {
    match value {
        BulletinPagination::Unknown => "unknown",
        BulletinPagination::Ready => "ready",
        BulletinPagination::Loading => "loading",
        BulletinPagination::Exhausted => "exhausted",
    }
}

fn operation(value: BulletinOperation) -> &'static str {
    match value {
        BulletinOperation::OpenSections => "open_sections",
        BulletinOperation::OpenWorldBoard => "open_world_board",
        BulletinOperation::OpenSection => "open_section",
        BulletinOperation::LoadOlder => "load_older",
        BulletinOperation::OpenEntry => "open_entry",
        BulletinOperation::PreviousEntry => "previous_entry",
        BulletinOperation::NextEntry => "next_entry",
        BulletinOperation::PostArticle => "post_article",
        BulletinOperation::DeleteEntry => "delete_entry",
        BulletinOperation::SendMail => "send_mail",
        BulletinOperation::HighlightArticle => "highlight_article",
        BulletinOperation::SelectSection => "select_section",
        BulletinOperation::SelectEntry => "select_entry",
        BulletinOperation::Scroll => "scroll",
        BulletinOperation::Back => "back",
        BulletinOperation::Forward => "forward",
        BulletinOperation::BeginBoardPost => "begin_board_post",
        BulletinOperation::BeginPlayerMail => "begin_player_mail",
        BulletinOperation::BeginReply => "begin_reply",
        BulletinOperation::UpdateCompose => "update_compose",
        BulletinOperation::Close => "close",
        BulletinOperation::Unknown => "unknown",
    }
}

fn optional_i64(value: Option<i64>) -> String {
    value.map_or("null".to_owned(), |value| value.to_string())
}
