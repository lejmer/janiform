pub use janiform_model_kernels::news::*;

use chrono::{DateTime, Utc};

use crate::calendar::ExchangeCalendarRegistry;

impl SharedNewsEventMarketStateResolver for ExchangeCalendarRegistry {
    fn resolve_market_state_at_utc(
        &self,
        exchange_code: &str,
        event_timestamp_utc: DateTime<Utc>,
    ) -> Result<
        janiform_foundations::market_time::MarketState,
        SharedNewsEventMarketStateResolutionError,
    > {
        ExchangeCalendarRegistry::resolve_market_state_at_utc(
            self,
            exchange_code,
            event_timestamp_utc,
        )
        .map_err(|source: crate::calendar::ExchangeCalendarResolveError| {
            SharedNewsEventMarketStateResolutionError {
                reason: source.to_string(),
            }
        })
    }
}
