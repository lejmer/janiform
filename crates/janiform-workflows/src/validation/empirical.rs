use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::path::Path;

use janiform::calibration::market_data::{
    DEFAULT_MARKET_DATA_TIMESTAMP_TIMEZONE, MarketDataInputFileFormat,
    MarketDataInputFileReference, MarketDataNormalizationPolicy, MarketDataTickDataColumnMapping,
    infer_market_data_input_file_format_from_path, infer_market_data_tick_price_input_policy,
    normalize_market_data_source, resolve_market_data_tick_data_column_mapping_for_source,
};
use janiform_foundations::identifiers::sanitize_ascii_identifier_component;
use janiform_foundations::paths::format_path;
use janiform_foundations::time::parse_duration_spec_nanos;

use super::request::StylizedFactsValidationWorkflowRequest;
use crate::error::WorkflowError;

pub(super) fn parsed_intervals(
    request: &StylizedFactsValidationWorkflowRequest,
) -> Result<Vec<ParsedInterval>, WorkflowError> {
    let mut intervals = Vec::with_capacity(request.intervals.len());
    let mut seen = BTreeMap::<String, ()>::new();

    for label in &request.intervals {
        let label = label.trim();

        if label.is_empty() {
            return Err(WorkflowError::ValidationInputFailed {
                field: "--intervals",
                message: "must not contain empty intervals".to_string(),
            });
        }

        if seen.insert(label.to_string(), ()).is_some() {
            return Err(WorkflowError::ValidationInputFailed {
                field: "--intervals",
                message: format!("duplicate interval `{label}`"),
            });
        }

        let duration_nanos = parse_duration_spec_nanos(label).map_err(|source| {
            WorkflowError::ValidationInputFailed {
                field: "--intervals",
                message: source.to_string(),
            }
        })?;

        intervals.push(ParsedInterval {
            label: label.to_string(),
            duration_nanos,
        });
    }

    Ok(intervals)
}

pub(super) fn prepare_empirical_plan_summary(
    request: &StylizedFactsValidationWorkflowRequest,
    empirical_data_path: &Path,
) -> Result<EmpiricalPlanSummary, WorkflowError> {
    let empirical_input = prepare_empirical_input(request, empirical_data_path)?;

    let empirical_ticks = normalize_market_data_source(
        &empirical_input.source,
        &empirical_input.column_mapping,
        &empirical_input.normalization_policy,
        false,
    )
    .map_err(|source| WorkflowError::ValidationInputFailed {
        field: "--empirical-data-path",
        message: source.to_string(),
    })?;

    let empirical_returns = tick_returns(&empirical_ticks.tick_records)?;

    Ok(EmpiricalPlanSummary {
        timestamp_column: empirical_input.column_mapping.timestamp_column,
        price_column: empirical_input.column_mapping.price_column,
        analysis_window: analysis_window(&empirical_returns)?,
        return_observations: empirical_returns,
    })
}

pub(super) fn prepare_empirical_input(
    request: &StylizedFactsValidationWorkflowRequest,
    empirical_data_path: &Path,
) -> Result<PreparedEmpiricalInput, WorkflowError> {
    let source = market_data_input_file_reference(
        empirical_data_path,
        request.empirical_data_format,
        None,
        "empirical",
    )?;

    let unresolved_mapping = MarketDataTickDataColumnMapping {
        timestamp_column: request.empirical_timestamp_column.clone(),
        price_column: request.empirical_price_column.clone(),
        trade_sign_column: request.empirical_trade_sign_column.clone(),
        bid_column: request.empirical_bid_column.clone(),
        ask_column: request.empirical_ask_column.clone(),
    };

    let tick_price_policy = match request.empirical_price_input_policy {
        Some(policy) => Some(policy),
        None => infer_market_data_tick_price_input_policy(&source, &unresolved_mapping).map_err(
            |source| WorkflowError::ValidationInputFailed {
                field: "--empirical-data-path",
                message: source.to_string(),
            },
        )?,
    };

    let normalization_policy = MarketDataNormalizationPolicy {
        timestamp_timezone: request
            .empirical_timestamp_timezone
            .clone()
            .unwrap_or_else(|| DEFAULT_MARKET_DATA_TIMESTAMP_TIMEZONE.to_string()),
        sort_records_by_timestamp: !request.keep_empirical_source_order,
        duplicate_record_policy: request.empirical_duplicate_record_policy,
        tick_price_input_policy: tick_price_policy,
        option_quote_input_policy: None,
    };

    let column_mapping = resolve_market_data_tick_data_column_mapping_for_source(
        &source,
        &unresolved_mapping,
        &normalization_policy,
    )
    .map_err(|source| WorkflowError::ValidationInputFailed {
        field: "--empirical-data-path",
        message: source.to_string(),
    })?;

    Ok(PreparedEmpiricalInput {
        source,
        column_mapping,
        normalization_policy,
    })
}

pub(super) fn tick_returns(
    ticks: &[janiform::calibration::market_data::MarketDataTranslationTickRecord],
) -> Result<Vec<ReturnObservation>, WorkflowError> {
    let mut returns = Vec::new();
    let mut previous_price = None::<f64>;

    for tick in ticks {
        let Some(price) = tick.price else {
            continue;
        };
        if !price.is_finite() || price <= 0.0 {
            continue;
        }

        if let Some(previous_price) = previous_price {
            returns.push(ReturnObservation {
                timestamp_utc_nanos: tick.timestamp_utc_nanos,
                return_increment: (price / previous_price).ln(),
            });
        }

        previous_price = Some(price);
    }

    if returns.is_empty() {
        return Err(WorkflowError::ValidationInputFailed {
            field: "--empirical-data-path",
            message: "must contain at least two positive finite price observations".to_string(),
        });
    }

    Ok(returns)
}

pub(super) fn analysis_window(
    returns: &[ReturnObservation],
) -> Result<AnalysisWindow, WorkflowError> {
    let start_utc_nanos = returns
        .first()
        .map(|observation| observation.timestamp_utc_nanos)
        .ok_or(WorkflowError::ValidationInputFailed {
            field: "--empirical-data-path",
            message: "must contain return observations".to_string(),
        })?;

    let end_utc_nanos = returns
        .last()
        .map(|observation| observation.timestamp_utc_nanos)
        .ok_or(WorkflowError::ValidationInputFailed {
            field: "--empirical-data-path",
            message: "must contain return observations".to_string(),
        })?;

    if end_utc_nanos <= start_utc_nanos {
        return Err(WorkflowError::ValidationInputFailed {
            field: "--empirical-data-path",
            message: "empirical timestamps must span a positive time range".to_string(),
        });
    }

    Ok(AnalysisWindow {
        start_utc_nanos,
        end_utc_nanos,
    })
}

pub(super) fn interval_count(
    start_utc_nanos: i64,
    end_utc_nanos: i64,
    duration_nanos: i64,
) -> Result<usize, WorkflowError> {
    if duration_nanos <= 0 {
        return Err(WorkflowError::ValidationInputFailed {
            field: "--intervals",
            message: "interval duration must be positive".to_string(),
        });
    }

    let span = i128::from(end_utc_nanos) - i128::from(start_utc_nanos);
    let duration = i128::from(duration_nanos);
    let count = (span + duration - 1) / duration;

    usize::try_from(count).map_err(|_| WorkflowError::ValidationInputFailed {
        field: "--intervals",
        message: "derived interval count does not fit in usize".to_string(),
    })
}

pub(super) fn observed_interval_count(
    returns: &[ReturnObservation],
    start_utc_nanos: i64,
    duration_nanos: i64,
) -> Result<usize, WorkflowError> {
    if duration_nanos <= 0 {
        return Err(WorkflowError::ValidationInputFailed {
            field: "--intervals",
            message: "interval duration must be positive".to_string(),
        });
    }

    let mut observed_intervals = BTreeSet::new();
    let duration = i128::from(duration_nanos);

    for observation in returns {
        let offset = i128::from(observation.timestamp_utc_nanos) - i128::from(start_utc_nanos);
        if offset < 0 {
            continue;
        }

        observed_intervals.insert(offset / duration);
    }

    Ok(observed_intervals.len())
}

fn market_data_input_file_reference(
    path: &Path,
    explicit_format: Option<MarketDataInputFileFormat>,
    explicit_provenance_label: Option<&str>,
    default_provenance_role: &str,
) -> Result<MarketDataInputFileReference, WorkflowError> {
    Ok(MarketDataInputFileReference {
        path: format_path(path),
        file_format: explicit_format.unwrap_or(infer_input_file_format(path)?),
        provenance_label: explicit_provenance_label
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| provenance_label_from_path(path, default_provenance_role)),
    })
}

pub(super) fn infer_input_file_format(
    path: &Path,
) -> Result<MarketDataInputFileFormat, WorkflowError> {
    infer_market_data_input_file_format_from_path(path).map_err(|source| {
        WorkflowError::ValidationInputFailed {
            field: "--empirical-data-path",
            message: source.to_string(),
        }
    })
}

fn provenance_label_from_path(path: &Path, default_role: &str) -> String {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .and_then(sanitize_ascii_identifier_component)
        .unwrap_or_else(|| default_role.to_string())
}

#[derive(Debug, Clone)]
pub(super) struct ParsedInterval {
    pub(super) label: String,
    pub(super) duration_nanos: i64,
}

#[derive(Debug)]
pub(super) struct PreparedEmpiricalInput {
    pub(super) source: MarketDataInputFileReference,
    pub(super) column_mapping: MarketDataTickDataColumnMapping,
    pub(super) normalization_policy: MarketDataNormalizationPolicy,
}

#[derive(Debug)]
pub(super) struct EmpiricalPlanSummary {
    pub(super) timestamp_column: Option<String>,
    pub(super) price_column: Option<String>,
    pub(super) analysis_window: AnalysisWindow,
    pub(super) return_observations: Vec<ReturnObservation>,
}

#[derive(Debug, Clone, Copy, serde::Serialize)]
pub(super) struct ReturnObservation {
    pub(super) timestamp_utc_nanos: i64,
    pub(super) return_increment: f64,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct AnalysisWindow {
    pub(super) start_utc_nanos: i64,
    pub(super) end_utc_nanos: i64,
}
