use std::path::PathBuf;
use std::time::Duration;

use chrono::{DateTime, Utc};
use janiform_foundations::paths::format_path;
use janiform_foundations::time::{deserialize_utc_datetime, serialize_utc_datetime};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::request::{ForecastMeasure, ForecastTableFormat};

/// Persisted forecast bundle schema version.
pub const FORECAST_SCHEMA_VERSION: &str = "janiform.forecast.v1";

/// Expected path table row.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ForecastExpectedPathRow {
    pub interval_index: usize,
    #[serde(deserialize_with = "deserialize_utc_datetime")]
    #[serde(serialize_with = "serialize_utc_datetime")]
    pub timestamp_utc: DateTime<Utc>,
    pub expected_log_return: f64,
    pub expected_log_price: f64,
    pub geometric_mean_price_ratio: f64,
    pub geometric_mean_price: f64,
    pub log_expected_price_ratio: f64,
    pub expected_price_ratio: f64,
    pub expected_price: f64,
}

/// Terminal distribution table row.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ForecastTerminalDistributionRow {
    pub node_index: usize,
    pub terminal_log_return: f64,
    pub terminal_price_ratio: f64,
    pub terminal_price: f64,
    pub log_return_density: f64,
    pub price_density: f64,
    pub probability_mass: f64,
    pub cumulative_probability: f64,
}

/// Forecast warning severity.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ForecastWarningSeverity {
    Warning,
}

/// Structured forecast warning persisted to the manifest.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ForecastWarning {
    pub code: String,
    pub severity: ForecastWarningSeverity,
    pub message: String,
    pub fields: Value,
}

/// Table column schema metadata persisted to the manifest.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ForecastTableColumnSchema {
    pub name: String,
    pub data_type: String,
}

/// Table schema metadata persisted to the manifest.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ForecastTableSchema {
    pub file_name: String,
    pub table_format: ForecastTableFormat,
    pub row_count: usize,
    pub columns: Vec<ForecastTableColumnSchema>,
}

/// Forecast quantile summary row.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ForecastQuantile {
    pub probability: f64,
    pub terminal_log_return: f64,
    pub terminal_price_ratio: f64,
    pub terminal_price: f64,
}

/// Scalar summary fields persisted to the forecast manifest.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ForecastScalarSummary {
    pub initial_price: f64,
    pub expected_terminal_log_return: f64,
    pub expected_terminal_log_price: f64,
    pub geometric_mean_terminal_price_ratio: f64,
    pub geometric_mean_terminal_price: f64,
    pub log_expected_terminal_price_ratio: f64,
    pub expected_terminal_price_ratio: f64,
    pub expected_terminal_price: f64,
    #[serde(deserialize_with = "deserialize_utc_datetime")]
    #[serde(serialize_with = "serialize_utc_datetime")]
    pub start_timestamp_utc: DateTime<Utc>,
    #[serde(deserialize_with = "deserialize_utc_datetime")]
    #[serde(serialize_with = "serialize_utc_datetime")]
    pub end_timestamp_utc: DateTime<Utc>,
}

/// Resolved distribution support metadata.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ForecastDistributionSupport {
    pub policy: String,
    pub grid_policy: String,
    pub lower_probability: Option<f64>,
    pub upper_probability: Option<f64>,
    pub log_return_lower: f64,
    pub log_return_upper: f64,
    pub node_count: usize,
}

/// Persisted forecast manifest.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ForecastBundleManifest {
    pub schema_version: String,
    pub run_id: String,
    pub model_run_id: String,
    #[serde(deserialize_with = "deserialize_utc_datetime")]
    #[serde(serialize_with = "serialize_utc_datetime")]
    pub created_at_utc: DateTime<Utc>,
    pub model_path: Option<PathBuf>,
    pub original_request: Value,
    pub resolved_request: Value,
    pub measure: ForecastMeasure,
    pub initial_state_policy: String,
    pub reference_price: f64,
    pub interval_duration_nanos: i64,
    pub resolved_horizon_interval_count: usize,
    #[serde(deserialize_with = "deserialize_utc_datetime")]
    #[serde(serialize_with = "serialize_utc_datetime")]
    pub start_timestamp_utc: DateTime<Utc>,
    #[serde(deserialize_with = "deserialize_utc_datetime")]
    #[serde(serialize_with = "serialize_utc_datetime")]
    pub end_timestamp_utc: DateTime<Utc>,
    pub warnings: Vec<ForecastWarning>,
    pub expected_path_table: ForecastTableSchema,
    pub terminal_distribution_table: ForecastTableSchema,
    pub distribution_support: ForecastDistributionSupport,
    pub quantiles: Vec<ForecastQuantile>,
    pub scalar_summary: ForecastScalarSummary,
}

/// Forecast workflow output held by native and CLI entrypoints.
#[derive(Clone, Debug)]
pub struct ForecastOutput {
    pub manifest: ForecastBundleManifest,
    pub expected_path: Vec<ForecastExpectedPathRow>,
    pub terminal_distribution: Vec<ForecastTerminalDistributionRow>,
    pub bundle_path: Option<PathBuf>,
    pub execution_duration: Duration,
}

impl ForecastOutput {
    /// Serialize the compact completion payload.
    pub fn response_payload(&self) -> Value {
        serde_json::json!({
            "schema_version": self.manifest.schema_version,
            "run_id": self.manifest.run_id,
            "model_run_id": self.manifest.model_run_id,
            "bundle_path": self.bundle_path.as_ref().map(|path| format_path(path)),
            "measure": self.manifest.measure,
            "expected_path": {
                "file_name": self.manifest.expected_path_table.file_name,
                "row_count": self.manifest.expected_path_table.row_count,
            },
            "terminal_distribution": {
                "file_name": self.manifest.terminal_distribution_table.file_name,
                "row_count": self.manifest.terminal_distribution_table.row_count,
            },
            "quantiles": self.manifest.quantiles,
            "warnings": self.manifest.warnings,
            "scalar_summary": self.manifest.scalar_summary,
            "execution_duration_seconds": self.execution_duration.as_secs_f64(),
        })
    }
}

pub(super) fn expected_path_table_schema(
    file_name: String,
    table_format: ForecastTableFormat,
    row_count: usize,
) -> ForecastTableSchema {
    ForecastTableSchema {
        file_name,
        table_format,
        row_count,
        columns: vec![
            column("interval_index", "uint64"),
            column("timestamp_utc", "timestamp[ns, UTC]"),
            column("expected_log_return", "float64"),
            column("expected_log_price", "float64"),
            column("geometric_mean_price_ratio", "float64"),
            column("geometric_mean_price", "float64"),
            column("log_expected_price_ratio", "float64"),
            column("expected_price_ratio", "float64"),
            column("expected_price", "float64"),
        ],
    }
}

pub(super) fn forecast_start_summary(
    run_id: &str,
    measure: ForecastMeasure,
    horizon_interval_count: Option<usize>,
    output_path: Option<&std::path::Path>,
) -> janiform_terminal_ui::StartSummary {
    let mut rows = vec![
        janiform_terminal_ui::KeyValueRow::new("Run", run_id),
        janiform_terminal_ui::KeyValueRow::new("Measure", measure.as_str()),
    ];

    if let Some(horizon_interval_count) = horizon_interval_count {
        rows.push(janiform_terminal_ui::KeyValueRow::new(
            "Intervals",
            horizon_interval_count.to_string(),
        ));
    }

    if let Some(output_path) = output_path {
        rows.push(janiform_terminal_ui::KeyValueRow::new(
            "Output",
            format_path(output_path),
        ));
    }

    janiform_terminal_ui::StartSummary {
        title: "Janiform forecast".to_string(),
        rows,
    }
}

pub(super) fn forecast_completion_summary(
    output: &ForecastOutput,
) -> janiform_terminal_ui::CompletionSummary {
    let quantile = |probability: f64| {
        output
            .manifest
            .quantiles
            .iter()
            .find(|quantile| quantile.probability == probability)
            .map(|quantile| quantile.terminal_price.to_string())
            .unwrap_or_else(|| "-".to_string())
    };

    let mut rows = vec![
        janiform_terminal_ui::KeyValueRow::new("p05", quantile(0.05)),
        janiform_terminal_ui::KeyValueRow::new("p50", quantile(0.50)),
        janiform_terminal_ui::KeyValueRow::new("p95", quantile(0.95)),
        janiform_terminal_ui::KeyValueRow::new(
            "Expected Path",
            output.manifest.expected_path_table.file_name.as_str(),
        ),
        janiform_terminal_ui::KeyValueRow::new(
            "Distribution",
            output
                .manifest
                .terminal_distribution_table
                .file_name
                .as_str(),
        ),
    ];

    if let Some(bundle_path) = output.bundle_path.as_deref() {
        rows.push(janiform_terminal_ui::KeyValueRow::new(
            "Bundle",
            format_path(bundle_path),
        ));
    }

    let warning_messages = forecast_warning_messages(output);
    if !warning_messages.is_empty() {
        rows.push(janiform_terminal_ui::KeyValueRow::block(
            "Warnings",
            warning_messages.join("\n"),
        ));
    }

    let status = if output.manifest.warnings.is_empty() {
        janiform_terminal_ui::CompletionSummaryStatus::Succeeded
    } else {
        janiform_terminal_ui::CompletionSummaryStatus::SucceededWithWarnings
    };

    janiform_terminal_ui::CompletionSummary {
        status,
        title: format!(
            "Forecast completed in {}",
            janiform_terminal_ui::format_elapsed_label(output.execution_duration)
        ),
        rows,
    }
}

fn forecast_warning_messages(output: &ForecastOutput) -> Vec<String> {
    output
        .manifest
        .warnings
        .iter()
        .map(|warning| warning.message.clone())
        .collect()
}

pub(super) fn terminal_distribution_table_schema(
    file_name: String,
    table_format: ForecastTableFormat,
    row_count: usize,
) -> ForecastTableSchema {
    ForecastTableSchema {
        file_name,
        table_format,
        row_count,
        columns: vec![
            column("node_index", "uint64"),
            column("terminal_log_return", "float64"),
            column("terminal_price_ratio", "float64"),
            column("terminal_price", "float64"),
            column("log_return_density", "float64"),
            column("price_density", "float64"),
            column("probability_mass", "float64"),
            column("cumulative_probability", "float64"),
        ],
    }
}

fn column(name: &str, data_type: &str) -> ForecastTableColumnSchema {
    ForecastTableColumnSchema {
        name: name.to_string(),
        data_type: data_type.to_string(),
    }
}
