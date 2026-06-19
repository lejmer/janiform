use std::path::PathBuf;

use chrono::{DateTime, Utc};
use janiform::model::hawkes::HawkesExcitationState;
use janiform::model::long_variance::LongVarianceState;
use janiform::simulation::MeanReversionBoundaryState;
use janiform_foundations::time::{
    deserialize_optional_utc_datetime, deserialize_utc_datetime, serialize_utc_datetime,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::WorkflowError;

/// Default model-only forecast starting price.
pub const DEFAULT_FORECAST_INITIAL_PRICE: f64 = 100.0;
/// Default forecast interval duration in nanoseconds.
pub const DEFAULT_FORECAST_INTERVAL_DURATION_NANOS: i64 =
    janiform_foundations::time::NANOS_PER_HOUR;
/// Default number of terminal distribution nodes.
pub const DEFAULT_FORECAST_DISTRIBUTION_NODE_COUNT: usize = 513;

/// Forecast measure selected by the user.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ForecastMeasure {
    /// Use the fitted physical dynamics.
    #[default]
    Physical,
    /// Use risk-neutral martingale dynamics with explicit carry inputs.
    RiskNeutral,
}

impl ForecastMeasure {
    /// Return the persisted measure value.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Physical => "physical",
            Self::RiskNeutral => "risk_neutral",
        }
    }
}

/// Forecast table file format.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ForecastTableFormat {
    /// Apache Parquet table files.
    #[default]
    Parquet,
    /// CSV table files.
    Csv,
    /// Streaming row-array JSON table files.
    Json,
}

impl ForecastTableFormat {
    /// Return the artifact file extension for this table format.
    pub const fn extension(self) -> &'static str {
        match self {
            Self::Parquet => "parquet",
            Self::Csv => "csv",
            Self::Json => "json",
        }
    }

    /// Return the persisted table format value.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Parquet => "parquet",
            Self::Csv => "csv",
            Self::Json => "json",
        }
    }
}

/// User-facing forecast workflow metadata.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ForecastWorkflowMetadata {
    /// Optional stable run id.
    pub run_id: Option<String>,
    /// Optional source model path.
    pub model_path: Option<PathBuf>,
    /// Optional output bundle target path.
    pub output_path: Option<PathBuf>,
}

impl ForecastWorkflowMetadata {
    /// Return the explicit run id or derive one from wall-clock time.
    pub fn run_id(&self) -> String {
        self.run_id
            .clone()
            .unwrap_or_else(|| format!("forecast_{}", chrono::Utc::now().format("%Y%m%dT%H%M%SZ")))
    }
}

/// Explicit fitted-model runtime state used as the forecast boundary condition.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ForecastInitialState {
    /// Current Hawkes excitation state.
    pub hawkes_excitation_state: HawkesExcitationState,
    /// Current long-variance state.
    pub long_variance_state: LongVarianceState,
    /// Current mean-reversion boundary state in model log-price coordinates.
    pub mean_reversion_boundary_state: MeanReversionBoundaryState,
}

/// Forecast request for a persisted model artifact payload.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ForecastWorkflowRequest {
    /// User-facing metadata and artifact paths.
    pub metadata: ForecastWorkflowMetadata,
    /// Persisted model artifact payload.
    pub model_payload: Value,
    /// Forecast start timestamp.
    #[serde(deserialize_with = "deserialize_utc_datetime")]
    #[serde(serialize_with = "serialize_utc_datetime")]
    pub start_date_utc: DateTime<Utc>,
    /// Initial observed price.
    #[serde(default)]
    pub initial_price: Option<f64>,
    /// Optional explicit fitted-model runtime state.
    #[serde(default)]
    pub state: Option<ForecastInitialState>,
    /// Runtime interval duration in nanoseconds.
    pub interval_duration_nanos: i64,
    /// Explicit horizon interval count.
    pub horizon_interval_count: Option<usize>,
    /// Requested terminal timestamp.
    #[serde(default, deserialize_with = "deserialize_optional_utc_datetime")]
    #[serde(serialize_with = "serialize_optional_utc_datetime")]
    pub end_date_utc: Option<DateTime<Utc>>,
    /// Forecast measure.
    #[serde(default)]
    pub measure: ForecastMeasure,
    /// Risk-free rate used only by risk-neutral forecasts.
    #[serde(default)]
    pub risk_free_rate: f64,
    /// Dividend yield used only by risk-neutral forecasts.
    #[serde(default)]
    pub dividend_yield: f64,
    /// Output table format.
    #[serde(default)]
    pub table_format: ForecastTableFormat,
    /// Distribution node count.
    pub distribution_node_count: usize,
    /// Optional explicit terminal log-return lower support.
    pub distribution_log_return_lower: Option<f64>,
    /// Optional explicit terminal log-return upper support.
    pub distribution_log_return_upper: Option<f64>,
    /// Optional CPU count for parallel forecast projection.
    #[serde(default)]
    pub cpu_count: Option<usize>,
    /// Fail before artifact writing when warnings are emitted.
    #[serde(default)]
    pub treat_forecast_warnings_as_errors: bool,
}

impl ForecastWorkflowRequest {
    /// Return a copy of the request with defaultable optional fields resolved.
    pub fn with_boundary_defaults(mut self) -> Self {
        if self.interval_duration_nanos == 0 {
            self.interval_duration_nanos = DEFAULT_FORECAST_INTERVAL_DURATION_NANOS;
        }

        if self.distribution_node_count == 0 {
            self.distribution_node_count = DEFAULT_FORECAST_DISTRIBUTION_NODE_COUNT;
        }

        self
    }
}

pub(super) fn validation_error(field: &'static str, message: impl Into<String>) -> WorkflowError {
    WorkflowError::ForecastValidationFailed {
        field,
        message: message.into(),
    }
}

fn serialize_optional_utc_datetime<S>(
    value: &Option<DateTime<Utc>>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    match value {
        Some(value) => serializer.serialize_some(&value.to_rfc3339()),
        None => serializer.serialize_none(),
    }
}
