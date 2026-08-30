use crate::{state::ObservationMetadata, stream::EventObservation};
use darpc_model as model;
use serde::Serialize;
use utoipa::ToSchema;

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct BulletinSnapshot {
    observation: ObservationMetadata,
    /// Active bulletin state, or null when no board, mailbox, message, or composer is open.
    bulletin: Option<BulletinState>,
}

impl BulletinSnapshot {
    pub(crate) fn from_model(pid: u32, snapshot: &model::ClientSnapshot) -> Self {
        Self {
            observation: ObservationMetadata::from_model(pid, snapshot),
            bulletin: snapshot.active_bulletin.as_ref().map(BulletinState::from),
        }
    }
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct BulletinState {
    /// Wrapping state revision required by all actions except open requests.
    revision: u32,
    pending: Option<BulletinOperation>,
    last_operation_result: Option<BulletinOperationResult>,
    can_go_back: bool,
    can_go_forward: bool,
    view: BulletinView,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum BulletinView {
    Sections {
        heading: String,
        sections: Vec<BulletinSection>,
        selected_section_id: Option<u16>,
        viewport: BulletinViewport,
        truncated: bool,
    },
    Entries {
        section: BulletinSection,
        entries: Vec<BulletinEntrySummary>,
        selected_entry_id: Option<i16>,
        viewport: BulletinViewport,
        pagination: BulletinPagination,
        truncated: bool,
    },
    Entry {
        section: BulletinSection,
        entry: BulletinEntry,
        viewport: BulletinViewport,
    },
    BoardPost {
        section: BulletinSection,
        author: String,
        subject: String,
        body: String,
    },
    PlayerMail {
        mailbox: BulletinSection,
        recipient: String,
        recipient_editable: bool,
        subject: String,
        body: String,
    },
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct BulletinSection {
    id: u16,
    name: String,
    kind: BulletinSectionKind,
    source: BulletinSource,
}

#[derive(Clone, Copy, Debug, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum BulletinSectionKind {
    Board,
    Mailbox,
    Unknown,
}

#[derive(Clone, Copy, Debug, Serialize, ToSchema)]
pub(crate) struct BulletinSource {
    kind: BulletinSourceKind,
    /// Client packet value retained even when its meaning is not known.
    raw: u8,
}

#[derive(Clone, Copy, Debug, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum BulletinSourceKind {
    Global,
    Clicked,
    Mail,
    Unknown,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct BulletinEntrySummary {
    id: i16,
    flags: u8,
    author: String,
    month: u8,
    day: u8,
    subject: String,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct BulletinEntry {
    id: i16,
    flags: Option<u8>,
    author: String,
    month: u8,
    day: u8,
    subject: String,
    body: String,
    /// Client navigation bit field retained without inferred semantics.
    navigation_flags: u8,
    /// Client packet byte retained without inferred semantics.
    unknown_before_id: u8,
}

#[derive(Clone, Copy, Debug, Serialize, ToSchema)]
pub(crate) struct BulletinViewport {
    position: i32,
    maximum: i32,
}

#[derive(Clone, Copy, Debug, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum BulletinPagination {
    Unknown,
    Ready,
    Loading,
    Exhausted,
}

#[derive(Clone, Copy, Debug, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum BulletinOperation {
    OpenSections,
    OpenWorldBoard,
    OpenSection,
    LoadOlder,
    OpenEntry,
    PreviousEntry,
    NextEntry,
    PostArticle,
    DeleteEntry,
    SendMail,
    HighlightArticle,
    SelectSection,
    SelectEntry,
    Scroll,
    Back,
    Forward,
    BeginBoardPost,
    BeginPlayerMail,
    BeginReply,
    UpdateCompose,
    Close,
    Unknown,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct BulletinOperationResult {
    operation: BulletinOperation,
    /// Uninterpreted status byte from the client protocol.
    raw_status: u8,
    message: Option<String>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct BulletinOpened {
    pub(crate) observation: EventObservation,
    bulletin: BulletinState,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct BulletinChanged {
    pub(crate) observation: EventObservation,
    bulletin: BulletinState,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct BulletinActionSubmitted {
    pub(crate) observation: EventObservation,
    bulletin: Option<BulletinState>,
    operation: BulletinOperation,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct BulletinOperationCompleted {
    pub(crate) observation: EventObservation,
    bulletin: BulletinState,
    result: BulletinOperationResult,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct BulletinClosed {
    pub(crate) observation: EventObservation,
    previous: BulletinState,
}

impl BulletinOpened {
    pub(crate) fn new(observation: EventObservation, state: model::BulletinState) -> Self {
        Self {
            observation,
            bulletin: BulletinState::from(&state),
        }
    }
}

impl BulletinChanged {
    pub(crate) fn new(observation: EventObservation, state: model::BulletinState) -> Self {
        Self {
            observation,
            bulletin: BulletinState::from(&state),
        }
    }
}

impl BulletinActionSubmitted {
    pub(crate) fn new(
        observation: EventObservation,
        state: Option<model::BulletinState>,
        operation: model::BulletinOperation,
    ) -> Self {
        Self {
            observation,
            bulletin: state.as_ref().map(BulletinState::from),
            operation: operation.into(),
        }
    }
}

impl BulletinOperationCompleted {
    pub(crate) fn new(
        observation: EventObservation,
        state: model::BulletinState,
        result: model::BulletinOperationResult,
    ) -> Self {
        Self {
            observation,
            bulletin: BulletinState::from(&state),
            result: result.into(),
        }
    }
}

impl BulletinClosed {
    pub(crate) fn new(observation: EventObservation, previous: model::BulletinState) -> Self {
        Self {
            observation,
            previous: BulletinState::from(&previous),
        }
    }
}

impl From<&model::BulletinState> for BulletinState {
    fn from(value: &model::BulletinState) -> Self {
        Self {
            revision: value.revision,
            pending: value.pending.map(Into::into),
            last_operation_result: value.last_operation_result.clone().map(Into::into),
            can_go_back: value.can_go_back,
            can_go_forward: value.can_go_forward,
            view: BulletinView::from(&value.view),
        }
    }
}

impl From<&model::BulletinView> for BulletinView {
    fn from(value: &model::BulletinView) -> Self {
        match value {
            model::BulletinView::Sections {
                heading,
                sections,
                selected_section_id,
                viewport,
                truncated,
            } => Self::Sections {
                heading: heading.clone(),
                sections: sections.iter().map(BulletinSection::from).collect(),
                selected_section_id: *selected_section_id,
                viewport: (*viewport).into(),
                truncated: *truncated,
            },
            model::BulletinView::Entries {
                section,
                entries,
                selected_entry_id,
                viewport,
                pagination,
                truncated,
            } => Self::Entries {
                section: section.into(),
                entries: entries.iter().map(BulletinEntrySummary::from).collect(),
                selected_entry_id: *selected_entry_id,
                viewport: (*viewport).into(),
                pagination: (*pagination).into(),
                truncated: *truncated,
            },
            model::BulletinView::Entry {
                section,
                entry,
                viewport,
            } => Self::Entry {
                section: section.into(),
                entry: entry.into(),
                viewport: (*viewport).into(),
            },
            model::BulletinView::Compose(model::BulletinCompose::BoardPost {
                section,
                author,
                subject,
                body,
            }) => Self::BoardPost {
                section: section.into(),
                author: author.clone(),
                subject: subject.clone(),
                body: body.clone(),
            },
            model::BulletinView::Compose(model::BulletinCompose::PlayerMail {
                mailbox,
                recipient,
                recipient_editable,
                subject,
                body,
            }) => Self::PlayerMail {
                mailbox: mailbox.into(),
                recipient: recipient.clone(),
                recipient_editable: *recipient_editable,
                subject: subject.clone(),
                body: body.clone(),
            },
        }
    }
}

impl From<&model::BulletinSection> for BulletinSection {
    fn from(value: &model::BulletinSection) -> Self {
        Self {
            id: value.id,
            name: value.name.clone(),
            kind: value.kind.into(),
            source: value.source.into(),
        }
    }
}

impl From<&model::BulletinEntrySummary> for BulletinEntrySummary {
    fn from(value: &model::BulletinEntrySummary) -> Self {
        Self {
            id: value.id,
            flags: value.flags,
            author: value.author.clone(),
            month: value.month,
            day: value.day,
            subject: value.subject.clone(),
        }
    }
}

impl From<&model::BulletinEntry> for BulletinEntry {
    fn from(value: &model::BulletinEntry) -> Self {
        Self {
            id: value.id,
            flags: value.flags,
            author: value.author.clone(),
            month: value.month,
            day: value.day,
            subject: value.subject.clone(),
            body: value.body.clone(),
            navigation_flags: value.navigation_flags,
            unknown_before_id: value.unknown_before_id,
        }
    }
}

impl From<model::BulletinViewport> for BulletinViewport {
    fn from(value: model::BulletinViewport) -> Self {
        Self {
            position: value.position,
            maximum: value.maximum,
        }
    }
}

impl From<model::BulletinSectionKind> for BulletinSectionKind {
    fn from(value: model::BulletinSectionKind) -> Self {
        match value {
            model::BulletinSectionKind::Board => Self::Board,
            model::BulletinSectionKind::Mailbox => Self::Mailbox,
            model::BulletinSectionKind::Unknown => Self::Unknown,
        }
    }
}

impl From<model::BulletinSource> for BulletinSource {
    fn from(value: model::BulletinSource) -> Self {
        Self {
            kind: match value {
                model::BulletinSource::Global => BulletinSourceKind::Global,
                model::BulletinSource::Clicked => BulletinSourceKind::Clicked,
                model::BulletinSource::Mail => BulletinSourceKind::Mail,
                model::BulletinSource::Unknown(_) => BulletinSourceKind::Unknown,
            },
            raw: value.raw(),
        }
    }
}

impl From<model::BulletinPagination> for BulletinPagination {
    fn from(value: model::BulletinPagination) -> Self {
        match value {
            model::BulletinPagination::Unknown => Self::Unknown,
            model::BulletinPagination::Ready => Self::Ready,
            model::BulletinPagination::Loading => Self::Loading,
            model::BulletinPagination::Exhausted => Self::Exhausted,
        }
    }
}

impl From<model::BulletinOperation> for BulletinOperation {
    fn from(value: model::BulletinOperation) -> Self {
        match value {
            model::BulletinOperation::OpenSections => Self::OpenSections,
            model::BulletinOperation::OpenWorldBoard => Self::OpenWorldBoard,
            model::BulletinOperation::OpenSection => Self::OpenSection,
            model::BulletinOperation::LoadOlder => Self::LoadOlder,
            model::BulletinOperation::OpenEntry => Self::OpenEntry,
            model::BulletinOperation::PreviousEntry => Self::PreviousEntry,
            model::BulletinOperation::NextEntry => Self::NextEntry,
            model::BulletinOperation::PostArticle => Self::PostArticle,
            model::BulletinOperation::DeleteEntry => Self::DeleteEntry,
            model::BulletinOperation::SendMail => Self::SendMail,
            model::BulletinOperation::HighlightArticle => Self::HighlightArticle,
            model::BulletinOperation::SelectSection => Self::SelectSection,
            model::BulletinOperation::SelectEntry => Self::SelectEntry,
            model::BulletinOperation::Scroll => Self::Scroll,
            model::BulletinOperation::Back => Self::Back,
            model::BulletinOperation::Forward => Self::Forward,
            model::BulletinOperation::BeginBoardPost => Self::BeginBoardPost,
            model::BulletinOperation::BeginPlayerMail => Self::BeginPlayerMail,
            model::BulletinOperation::BeginReply => Self::BeginReply,
            model::BulletinOperation::UpdateCompose => Self::UpdateCompose,
            model::BulletinOperation::Close => Self::Close,
            model::BulletinOperation::Unknown => Self::Unknown,
        }
    }
}

impl From<model::BulletinOperationResult> for BulletinOperationResult {
    fn from(value: model::BulletinOperationResult) -> Self {
        Self {
            operation: value.operation.into(),
            raw_status: value.raw_status,
            message: value.message,
        }
    }
}
