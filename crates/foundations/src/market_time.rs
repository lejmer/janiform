use std::fmt::{Display, Formatter};

pub const MARKET_STATE_RESOLUTION_POLICY: &str = "resolve_market_state_at_utc.v1";
pub const SESSION_WINDOW_BOUNDARY_POLICY: &str = "open_inclusive_close_exclusive.v1";

/// Canonical market state returned by calendar resolution for v0.1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarketState {
    Open,
    Closed,
}

impl Display for MarketState {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        let value = match self {
            Self::Open => "open",
            Self::Closed => "closed",
        };
        formatter.write_str(value)
    }
}
