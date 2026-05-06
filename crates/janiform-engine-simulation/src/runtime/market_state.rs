use std::str::FromStr;

use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use janiform_model_kernels::news::{
    SharedNewsEventMarketStateResolutionError, SharedNewsEventMarketStateResolver,
};

use crate::constants::ONE_SECOND_NANOS;
use crate::error::SeededTickSimulationRunError;
use janiform_market_time::calendar::{
    ExchangeCalendarDefinition, ExchangeCalendarRegistry, MarketState,
};

pub(crate) struct SeededSimulationMarketStateResolver<'a> {
    pub(crate) definition: &'a ExchangeCalendarDefinition,
    pub(crate) exchange_code: &'a str,
    pub(crate) timezone: Tz,
}

impl<'a> SeededSimulationMarketStateResolver<'a> {
    pub(crate) fn new(
        calendar_registry: &'a ExchangeCalendarRegistry,
        exchange_code: &'a str,
    ) -> Result<Self, SeededTickSimulationRunError> {
        let definition =
            calendar_registry
                .get_required(exchange_code)
                .map_err(|source| {
                    SeededTickSimulationRunError::NewsMarketStateResolutionFailed {
                        exchange_code: exchange_code.to_string(),
                        timestamp_utc_nanos: 0,
                        source: Box::new(match source {
                            janiform_market_time::calendar::ExchangeCalendarRegistryLookupError::MissingExchangeId {
                                exchange_code,
                            } => janiform_market_time::calendar::ExchangeCalendarResolveError::MissingExchangeId {
                                exchange_code,
                            },
                        }),
                    }
                })?;

        let timezone = Tz::from_str(definition.timezone_name()).map_err(|_| {
            SeededTickSimulationRunError::NewsMarketStateResolutionFailed {
                exchange_code: exchange_code.to_string(),
                timestamp_utc_nanos: 0,
                source: Box::new(janiform_market_time::calendar::ExchangeCalendarResolveError::InvalidTimezoneName {
                    exchange_code: exchange_code.to_string(),
                    timezone_name: definition.timezone_name().to_string(),
                }),
            }
        })?;

        Ok(Self {
            definition,
            exchange_code,
            timezone,
        })
    }

    pub(crate) fn resolve_market_state_at_nanos(
        &self,
        timestamp_utc_nanos: i64,
    ) -> Result<MarketState, SeededTickSimulationRunError> {
        let seconds = timestamp_utc_nanos.div_euclid(ONE_SECOND_NANOS);
        let nanoseconds = timestamp_utc_nanos.rem_euclid(ONE_SECOND_NANOS) as u32;

        let timestamp_utc = DateTime::<Utc>::from_timestamp(seconds, nanoseconds).ok_or(
            SeededTickSimulationRunError::InvalidKnownScheduledNewsTimestampUtcNanos {
                value: timestamp_utc_nanos,
            },
        )?;

        Ok(self
            .definition
            .resolve_market_state_at_utc_with_timezone(self.timezone, timestamp_utc))
    }
}

impl SharedNewsEventMarketStateResolver for SeededSimulationMarketStateResolver<'_> {
    fn resolve_market_state_at_utc(
        &self,
        exchange_code: &str,
        event_timestamp_utc: DateTime<Utc>,
    ) -> Result<MarketState, SharedNewsEventMarketStateResolutionError> {
        if exchange_code != self.exchange_code {
            return Err(SharedNewsEventMarketStateResolutionError {
                reason: format!(
                    "cached seeded market-state resolver is bound to exchange `{}` but received `{exchange_code}`",
                    self.exchange_code
                ),
            });
        }

        Ok(self
            .definition
            .resolve_market_state_at_utc_with_timezone(self.timezone, event_timestamp_utc))
    }
}
