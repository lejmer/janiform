use std::collections::BTreeMap;
use std::str::FromStr;

use chrono::{DateTime, Utc};
use chrono_tz::Tz;

use crate::calendar::definition::resolve_market_state_for_definition_at_utc;
use crate::calendar::errors::{
    ExchangeCalendarRegistryLoadError, ExchangeCalendarRegistryLookupError,
    ExchangeCalendarResolveError, TimestampEventPreprocessingError,
};
use crate::calendar::preprocessing::preprocess_timestamp_events;
use crate::calendar::types::{
    ExchangeCalendarDefinition, ExchangeCalendarRegistryArtifact, MarketState,
    TimestampEventInputRecord, TimestampEventPreprocessingOutput, TimestampEventSessionFilter,
};
use crate::calendar::validation::{require_non_empty, validate_iso_date};

const CALENDAR_REGISTRY_SCHEMA_VERSION: &str = "calendar.registry.v1";

/// Immutable in-memory calendar registry keyed by exchange code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExchangeCalendarRegistry {
    definitions_by_exchange: BTreeMap<String, ExchangeCalendarDefinition>,
}

impl ExchangeCalendarRegistry {
    /// Build a registry from a parsed registry artifact.
    pub fn from_artifact(
        artifact: ExchangeCalendarRegistryArtifact,
    ) -> Result<Self, ExchangeCalendarRegistryLoadError> {
        let schema_version = require_non_empty(artifact.schema_version, "schema_version")
            .map_err(ExchangeCalendarRegistryLoadError::from_validation_error)?;
        if schema_version != CALENDAR_REGISTRY_SCHEMA_VERSION {
            return Err(
                ExchangeCalendarRegistryLoadError::UnsupportedSchemaVersion {
                    expected: CALENDAR_REGISTRY_SCHEMA_VERSION,
                    actual: schema_version,
                },
            );
        }

        if artifact.exchanges.is_empty() {
            return Err(ExchangeCalendarRegistryLoadError::MissingField { field: "exchanges" });
        }

        validate_iso_date(&artifact.artifact_date).map_err(|reason| {
            ExchangeCalendarRegistryLoadError::MalformedDate {
                field: "artifact_date",
                value: artifact.artifact_date.clone(),
                reason,
            }
        })?;

        let mut definitions_by_exchange: BTreeMap<String, ExchangeCalendarDefinition> =
            BTreeMap::new();

        for exchange_artifact in artifact.exchanges {
            let exchange_code = exchange_artifact.exchange_code.trim().to_string();
            let definition =
                ExchangeCalendarDefinition::from_artifact(exchange_artifact).map_err(|source| {
                    ExchangeCalendarRegistryLoadError::InvalidExchangeArtifact {
                        exchange_code: (!exchange_code.is_empty()).then_some(exchange_code.clone()),
                        source,
                    }
                })?;
            if definitions_by_exchange
                .insert(exchange_code.clone(), definition)
                .is_some()
            {
                return Err(ExchangeCalendarRegistryLoadError::DuplicateExchangeId {
                    exchange_code,
                });
            }
        }

        Ok(Self {
            definitions_by_exchange,
        })
    }

    /// Lookup a loaded exchange by identifier.
    pub fn get(&self, exchange_code: &str) -> Option<&ExchangeCalendarDefinition> {
        self.definitions_by_exchange.get(exchange_code)
    }

    /// Lookup a loaded exchange by identifier with a structured error when missing.
    pub fn get_required(
        &self,
        exchange_code: &str,
    ) -> Result<&ExchangeCalendarDefinition, ExchangeCalendarRegistryLookupError> {
        self.get(exchange_code).ok_or_else(|| {
            ExchangeCalendarRegistryLookupError::MissingExchangeId {
                exchange_code: exchange_code.to_string(),
            }
        })
    }

    /// Resolve market state for a UTC timestamp and exchange identifier.
    ///
    /// Boundary policy:
    /// - Session windows are interpreted as half-open intervals `[open, close)`.
    /// - Exact session open timestamp resolves to `MarketState::Open`.
    /// - Exact session close timestamp resolves to `MarketState::Closed`.
    /// Rule precedence:
    /// - Holidays resolve `MarketState::Closed` for the full local exchange date.
    /// - Early-close windows override regular sessions when configured for a local date.
    /// - Regular sessions apply when neither holiday nor early-close rule matches.
    pub fn resolve_market_state_at_utc(
        &self,
        exchange_code: &str,
        timestamp_utc: DateTime<Utc>,
    ) -> Result<MarketState, ExchangeCalendarResolveError> {
        let (definition, timezone) = self.required_definition_and_timezone(exchange_code)?;

        Ok(resolve_market_state_for_definition_at_utc(
            definition,
            timezone,
            timestamp_utc,
        ))
    }

    /// Preprocess timestamp events for deterministic seasonality workflows.
    pub fn preprocess_timestamp_events(
        &self,
        exchange_code: &str,
        input_records: Vec<TimestampEventInputRecord>,
        session_filter: TimestampEventSessionFilter,
        sparse_hour_event_threshold: usize,
    ) -> Result<TimestampEventPreprocessingOutput, TimestampEventPreprocessingError> {
        let (definition, timezone) = self
            .required_definition_and_timezone(exchange_code)
            .map_err(TimestampEventPreprocessingError::from_calendar_resolve_error)?;

        preprocess_timestamp_events(
            definition,
            timezone,
            exchange_code,
            input_records,
            session_filter,
            sparse_hour_event_threshold,
        )
    }

    fn required_definition_and_timezone(
        &self,
        exchange_code: &str,
    ) -> Result<(&ExchangeCalendarDefinition, Tz), ExchangeCalendarResolveError> {
        let definition = self
            .get_required(exchange_code)
            .map_err(ExchangeCalendarResolveError::from_lookup_error)?;
        let timezone_name = definition.timezone_name();
        let timezone = Tz::from_str(timezone_name).map_err(|_| {
            ExchangeCalendarResolveError::InvalidTimezoneName {
                exchange_code: exchange_code.to_string(),
                timezone_name: timezone_name.to_string(),
            }
        })?;

        Ok((definition, timezone))
    }
}

/// Loader for parsing exchange calendar registry artifacts.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ExchangeCalendarRegistryLoader;

impl ExchangeCalendarRegistryLoader {
    /// Parse and load a registry from JSON artifact content.
    pub fn load_from_json(
        artifact_json: &str,
    ) -> Result<ExchangeCalendarRegistry, ExchangeCalendarRegistryLoadError> {
        let artifact = serde_json::from_str::<ExchangeCalendarRegistryArtifact>(artifact_json)
            .map_err(ExchangeCalendarRegistryLoadError::from_deserialization_error)?;
        ExchangeCalendarRegistry::from_artifact(artifact)
    }
}
