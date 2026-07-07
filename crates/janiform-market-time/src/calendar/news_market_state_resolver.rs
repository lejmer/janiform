use chrono::{DateTime, Utc};
use janiform_model_kernels::news::{
    SharedNewsEventMarketStateResolutionError, SharedNewsEventMarketStateResolver,
};

use crate::calendar::{ExchangeCalendarRegistry, ExchangeCalendarResolveError};

impl SharedNewsEventMarketStateResolver for ExchangeCalendarRegistry {
    fn resolve_market_state_at_utc(
        &self,
        exchange_code: &str,
        event_timestamp_utc: DateTime<Utc>,
    ) -> Result<
        janiform_foundations::market_state::MarketState,
        SharedNewsEventMarketStateResolutionError,
    > {
        ExchangeCalendarRegistry::resolve_market_state_at_utc(
            self,
            exchange_code,
            event_timestamp_utc,
        )
        .map_err(|source: ExchangeCalendarResolveError| {
            SharedNewsEventMarketStateResolutionError {
                reason: source.to_string(),
            }
        })
    }
}
