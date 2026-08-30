#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BulletinSource {
    Global,
    Clicked,
    Mail,
    Unknown(u8),
}

impl BulletinSource {
    #[must_use]
    pub const fn from_raw(value: u8) -> Self {
        match value {
            1 => Self::Global,
            2 => Self::Clicked,
            3 => Self::Mail,
            value => Self::Unknown(value),
        }
    }

    #[must_use]
    pub const fn raw(self) -> u8 {
        match self {
            Self::Global => 1,
            Self::Clicked => 2,
            Self::Mail => 3,
            Self::Unknown(value) => value,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BulletinSectionKind {
    Board,
    Mailbox,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BulletinSection {
    pub id: u16,
    pub name: String,
    pub kind: BulletinSectionKind,
    pub source: BulletinSource,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BulletinEntrySummary {
    pub id: i16,
    pub flags: u8,
    pub author: String,
    pub month: u8,
    pub day: u8,
    pub subject: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BulletinEntry {
    pub id: i16,
    pub flags: Option<u8>,
    pub author: String,
    pub month: u8,
    pub day: u8,
    pub subject: String,
    pub body: String,
    pub navigation_flags: u8,
    pub unknown_before_id: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BulletinViewport {
    pub position: i32,
    pub maximum: i32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BulletinPagination {
    Unknown,
    Ready,
    Loading,
    Exhausted,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BulletinCompose {
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BulletinView {
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
    Compose(BulletinCompose),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BulletinOperation {
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BulletinOperationResult {
    pub operation: BulletinOperation,
    pub raw_status: u8,
    pub message: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BulletinState {
    pub revision: u32,
    pub pending: Option<BulletinOperation>,
    pub last_operation_result: Option<BulletinOperationResult>,
    pub can_go_back: bool,
    pub can_go_forward: bool,
    pub view: BulletinView,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BulletinUpdate {
    Opened(BulletinState),
    Changed(BulletinState),
    ActionSubmitted {
        state: Option<BulletinState>,
        operation: BulletinOperation,
    },
    OperationResult {
        state: BulletinState,
        result: BulletinOperationResult,
    },
    Closed {
        previous: BulletinState,
    },
}
