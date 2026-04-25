//! Exchange calendar domain model, resolver semantics, and artifact validation.
//!
//! The model is designed for immutable construction from versioned artifacts.
//! It covers timezone context, exchange metadata, and regular session windows
//! that can be consumed by downstream calendar rule resolution.
//!
//! # Examples
//!
//! A single regular session per day:
//! ```rust
//! use janiform_market_time::calendar::{
//!     CalendarRuleMetadataArtifact, ExchangeCalendarArtifact, ExchangeCalendarDefinition,
//!     RegularSessionArtifact, SessionWindowArtifact, TradingWeekday,
//! };
//!
//! let artifact = ExchangeCalendarArtifact {
//!     schema_version: "calendar.v1".to_string(),
//!     exchange_code: "XNYS".to_string(),
//!     exchange_name: "New York Stock Exchange".to_string(),
//!     timezone_name: Some("America/New_York".to_string()),
//!     rule_metadata: CalendarRuleMetadataArtifact {
//!         holiday_calendar_code: "US_EQUITIES".to_string(),
//!         early_close_rule_set: Some("US_EQUITIES_EARLY_CLOSE".to_string()),
//!     },
//!     regular_sessions: vec![RegularSessionArtifact {
//!         weekday: TradingWeekday::Monday,
//!         windows: vec![SessionWindowArtifact {
//!             label: "regular".to_string(),
//!             open_time: "09:30".to_string(),
//!             close_time: "16:00".to_string(),
//!         }],
//!     }],
//!     holiday_dates: vec![],
//!     early_close_sessions: vec![],
//! };
//!
//! let definition = ExchangeCalendarDefinition::from_artifact(artifact)?;
//! assert_eq!(definition.timezone_name(), "America/New_York");
//! # Ok::<(), janiform_market_time::calendar::ExchangeCalendarValidationError>(())
//! ```
//!
//! A split session on a single weekday:
//! ```rust
//! use janiform_market_time::calendar::{
//!     CalendarRuleMetadataArtifact, ExchangeCalendarArtifact, ExchangeCalendarDefinition,
//!     RegularSessionArtifact, SessionWindowArtifact, TradingWeekday,
//! };
//!
//! let artifact = ExchangeCalendarArtifact {
//!     schema_version: "calendar.v1".to_string(),
//!     exchange_code: "XTKS".to_string(),
//!     exchange_name: "Tokyo Stock Exchange".to_string(),
//!     timezone_name: Some("Asia/Tokyo".to_string()),
//!     rule_metadata: CalendarRuleMetadataArtifact {
//!         holiday_calendar_code: "JP_EQUITIES".to_string(),
//!         early_close_rule_set: None,
//!     },
//!     regular_sessions: vec![RegularSessionArtifact {
//!         weekday: TradingWeekday::Monday,
//!         windows: vec![
//!             SessionWindowArtifact {
//!                 label: "morning".to_string(),
//!                 open_time: "09:00".to_string(),
//!                 close_time: "11:30".to_string(),
//!             },
//!             SessionWindowArtifact {
//!                 label: "afternoon".to_string(),
//!                 open_time: "12:30".to_string(),
//!                 close_time: "15:00".to_string(),
//!             },
//!         ],
//!     }],
//!     holiday_dates: vec![],
//!     early_close_sessions: vec![],
//! };
//!
//! let definition = ExchangeCalendarDefinition::from_artifact(artifact)?;
//! assert_eq!(definition.regular_sessions().len(), 1);
//! # Ok::<(), janiform_market_time::calendar::ExchangeCalendarValidationError>(())
//! ```

mod constants;
mod definition;
mod errors;
mod news_market_state_resolver;
mod preprocessing;
mod registry;
mod types;
mod validation;

pub use constants::{
    PREPROCESSED_TIMESTAMP_EVENT_SCHEMA_VERSION, TIMESTAMP_EVENT_INPUT_SCHEMA_VERSION,
};
pub use janiform_foundations::market_state::{
    MARKET_STATE_RESOLUTION_POLICY, SESSION_WINDOW_BOUNDARY_POLICY,
};

pub use crate::seasonality::{
    DAY_OF_WEEK_SEASONALITY_ESTIMATE_SCHEMA_VERSION,
    DAY_OF_YEAR_SEASONALITY_ESTIMATE_SCHEMA_VERSION,
    DETERMINISTIC_SEASONALITY_COMPOSITION_SCHEMA_VERSION, DayOfWeekSeasonalityDiagnostics,
    DayOfWeekSeasonalityEstimateOutput, DayOfWeekSeasonalityEstimationError,
    DayOfWeekSeasonalityEstimatorSettings, DayOfYearSeasonalityDiagnostics,
    DayOfYearSeasonalityEstimateOutput, DayOfYearSeasonalityEstimationError,
    DayOfYearSeasonalityEstimatorSettings, DeterministicSeasonalityCompositionDiagnostics,
    DeterministicSeasonalityCompositionError, DeterministicSeasonalityCompositionOutput,
    INTRADAY_SEASONALITY_ESTIMATE_SCHEMA_VERSION, IntradaySeasonalityDiagnostics,
    IntradaySeasonalityEstimateOutput, IntradaySeasonalityEstimationError,
    IntradaySeasonalityEstimatorSettings, IntradaySmoothingKernel,
    SEASONALITY_DIAGNOSTICS_BATCH_REPORT_SCHEMA_VERSION,
    SEASONALITY_DIAGNOSTICS_REPORT_SCHEMA_VERSION, ScheduledEventOverlayDefinition,
    ScheduledEventOverlayProvenanceMetadata, SeasonalityDiagnosticsBatchReportOutput,
    SeasonalityDiagnosticsReportError, SeasonalityDiagnosticsReportInput,
    SeasonalityDiagnosticsReportOutput, compose_deterministic_seasonality_signal,
    estimate_day_of_week_seasonality, estimate_day_of_year_seasonality,
    estimate_intraday_seasonality, generate_seasonality_diagnostics_batch_report,
    generate_seasonality_diagnostics_report,
};

pub use errors::{
    ExchangeCalendarRegistryLoadError, ExchangeCalendarRegistryLookupError,
    ExchangeCalendarResolveError, ExchangeCalendarValidationError,
    TimestampEventPreprocessingError,
};
pub use registry::{ExchangeCalendarRegistry, ExchangeCalendarRegistryLoader};
pub use types::{
    CalendarDate, CalendarRuleMetadata, CalendarRuleMetadataArtifact, EarlyCloseSessionArtifact,
    ExchangeCalendarArtifact, ExchangeCalendarDefinition, ExchangeCalendarRegistryArtifact,
    ExchangeMetadata, MarketState, RegularSessionArtifact, RegularSessionDefinition, SessionTime,
    SessionWindowArtifact, SessionWindowDefinition, TimestampEventInputRecord,
    TimestampEventPreprocessingDiagnostics, TimestampEventPreprocessingOutput,
    TimestampEventSessionFilter, TradingWeekday,
};

#[cfg(test)]
mod tests;
