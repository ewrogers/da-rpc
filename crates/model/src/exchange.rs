#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExchangeParty {
    Local,
    Other,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExchangeItem {
    pub index: u8,
    pub sprite: u16,
    pub dye_color: u8,
    /// Known for locally submitted items and when the server-provided name
    /// carries a stack count. The exchange packet does not otherwise include it.
    pub quantity: Option<u8>,
    pub name: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ExchangeOffer {
    pub items: Vec<ExchangeItem>,
    pub gold: u32,
    pub accepted: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExchangeState {
    pub id: u32,
    pub partner: String,
    pub local: ExchangeOffer,
    pub other: ExchangeOffer,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExchangeUpdate {
    Opened(ExchangeState),
    ItemAdded {
        state: ExchangeState,
        party: ExchangeParty,
        item: ExchangeItem,
    },
    GoldChanged {
        state: ExchangeState,
        party: ExchangeParty,
        gold: u32,
    },
    Accepted {
        state: ExchangeState,
        party: ExchangeParty,
        message: String,
    },
    Completed {
        state: ExchangeState,
        message: String,
    },
    Cancelled {
        state: ExchangeState,
        message: String,
    },
}

impl ExchangeUpdate {
    #[must_use]
    pub const fn state(&self) -> &ExchangeState {
        match self {
            Self::Opened(state)
            | Self::ItemAdded { state, .. }
            | Self::GoldChanged { state, .. }
            | Self::Accepted { state, .. }
            | Self::Completed { state, .. }
            | Self::Cancelled { state, .. } => state,
        }
    }
}
