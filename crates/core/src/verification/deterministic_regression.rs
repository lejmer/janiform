//! Deterministic fixture-driven regression harness for calendar state and hourly factors.

use std::error::Error;
use std::fmt::{Display, Formatter};

use chrono::{DateTime, Utc};
use serde::Serialize;

use crate::calendar::{ExchangeCalendarRegistry, MarketState};
use crate::hourly_factors::{
    HourlyFactorProviderError, HourlyFactorProviderRequest, HourlyNewsIntensityFactorSource,
    HourlySeasonalityFactorSource, provide_hourly_factor_contract,
};

use super::contracts::RegressionCaseStatus;

pub const DETERMINISTIC_REGRESSION_HARNESS_REPORT_SCHEMA_VERSION: &str =
    "validation.deterministic_regression_harness.report.v1";
pub const DETERMINISTIC_REGRESSION_HARNESS_DETERMINISM_POLICY: &str =
    "stable_input_order_and_first_mismatch_reporting.v1";
pub const VALIDATION_FALLBACK_POLICY_SCHEMA_VERSION: &str = "validation.fallback_policy.v1";
pub const VALIDATION_UNKNOWN_DATE_FALLBACK_POLICY: &str =
    "resolve_with_regular_sessions_when_no_date_override.v1";
pub const VALIDATION_UNSUPPORTED_CALENDAR_SEGMENT_POLICY: &str =
    "fail_case_with_explicit_calendar_resolution_error.v1";
pub const VALIDATION_INCOMPLETE_FIXTURE_COVERAGE_POLICY: &str =
    "fail_fast_on_fixture_contract_validation.v1";

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RegressionHarnessToleranceConfig {
    pub seasonality_absolute_tolerance: f64,
    pub news_intensity_absolute_tolerance: f64,
}

impl Default for RegressionHarnessToleranceConfig {
    fn default() -> Self {
        Self {
            seasonality_absolute_tolerance: 1e-12,
            news_intensity_absolute_tolerance: 1e-12,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CalendarStateRegressionCase {
    pub fixture_id: String,
    pub exchange_code: String,
    pub timestamp_utc: DateTime<Utc>,
    pub expected_state: MarketState,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HourlyFactorRegressionCase {
    pub fixture_id: String,
    pub exchange_code: String,
    pub range_start_utc: DateTime<Utc>,
    pub range_end_utc: DateTime<Utc>,
    pub seasonality_source: HourlySeasonalityRegressionSource,
    pub news_intensity_source: HourlyNewsIntensityRegressionSource,
    pub expected_s_h: Vec<f64>,
    pub expected_nu_h: Vec<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum HourlySeasonalityRegressionSource {
    Explicit {
        hourly_factors: Vec<f64>,
        materialization_method: String,
    },
    Materialized {
        source_timestamp_utc_nanos: Vec<i64>,
        source_seasonality_factors: Vec<f64>,
        interpolation_method: String,
        aggregation_method: String,
        sparse_fallback_policy: String,
        sparse_hour_minimum_observation_count: usize,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum HourlyNewsIntensityRegressionSource {
    Explicit {
        hourly_factors: Vec<f64>,
        materialization_method: String,
    },
    Materialized {
        known_scheduled_event_timestamps_utc_nanos: Vec<i64>,
        surprise_baseline_intensity: f64,
        surprise_deterministic_time_multiplier_by_interval_index: Vec<(u64, f64)>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RegressionCaseKind {
    CalendarMarketState,
    HourlyFactorOutput,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "failure_kind", rename_all = "snake_case")]
pub enum RegressionCaseFailure {
    CalendarResolutionError {
        exchange_code: String,
        timestamp_utc: String,
        error_message: String,
    },
    MarketStateMismatch {
        exchange_code: String,
        timestamp_utc: String,
        expected_state: String,
        observed_state: String,
    },
    HourlyFactorResolutionError {
        error_message: String,
    },
    FactorLengthMismatch {
        field_name: &'static str,
        expected_length: usize,
        observed_length: usize,
    },
    FactorToleranceExceeded {
        field_name: &'static str,
        value_index: usize,
        expected_value: f64,
        observed_value: f64,
        absolute_difference: f64,
        absolute_tolerance: f64,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RegressionCaseReport {
    pub fixture_id: String,
    pub case_kind: RegressionCaseKind,
    pub status: RegressionCaseStatus,
    pub summary: String,
    pub failure: Option<RegressionCaseFailure>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RegressionHarnessSummary {
    pub total_case_count: usize,
    pub passed_case_count: usize,
    pub failed_case_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RegressionHarnessFallbackPolicy {
    pub schema_version: &'static str,
    pub unknown_date_policy: &'static str,
    pub unsupported_exchange_calendar_segment_policy: &'static str,
    pub incomplete_fixture_coverage_policy: &'static str,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RegressionHarnessReport {
    pub schema_version: &'static str,
    pub determinism_policy: &'static str,
    pub fallback_policy: RegressionHarnessFallbackPolicy,
    pub tolerance_config: RegressionHarnessToleranceConfig,
    pub summary: RegressionHarnessSummary,
    pub case_reports: Vec<RegressionCaseReport>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum RegressionHarnessError {
    InvalidSeasonalityAbsoluteTolerance { value: f64 },
    InvalidNewsIntensityAbsoluteTolerance { value: f64 },
}

impl Display for RegressionHarnessError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidSeasonalityAbsoluteTolerance { value } => write!(
                formatter,
                "seasonality_absolute_tolerance must be a finite non-negative value, found {value}"
            ),
            Self::InvalidNewsIntensityAbsoluteTolerance { value } => write!(
                formatter,
                "news_intensity_absolute_tolerance must be a finite non-negative value, found {value}"
            ),
        }
    }
}

impl Error for RegressionHarnessError {}

pub fn run_deterministic_regression_harness(
    calendar_registry: &ExchangeCalendarRegistry,
    calendar_cases: &[CalendarStateRegressionCase],
    hourly_factor_cases: &[HourlyFactorRegressionCase],
    tolerance_config: RegressionHarnessToleranceConfig,
) -> Result<RegressionHarnessReport, RegressionHarnessError> {
    validate_tolerance_config(&tolerance_config)?;

    let mut case_reports: Vec<RegressionCaseReport> =
        Vec::with_capacity(calendar_cases.len() + hourly_factor_cases.len());

    for calendar_case in calendar_cases {
        case_reports.push(evaluate_calendar_case(calendar_registry, calendar_case));
    }

    for hourly_factor_case in hourly_factor_cases {
        case_reports.push(evaluate_hourly_factor_case(
            calendar_registry,
            hourly_factor_case,
            &tolerance_config,
        ));
    }

    let passed_case_count = case_reports
        .iter()
        .filter(|case_report| case_report.status == RegressionCaseStatus::Pass)
        .count();
    let total_case_count = case_reports.len();
    let failed_case_count = total_case_count - passed_case_count;

    Ok(RegressionHarnessReport {
        schema_version: DETERMINISTIC_REGRESSION_HARNESS_REPORT_SCHEMA_VERSION,
        determinism_policy: DETERMINISTIC_REGRESSION_HARNESS_DETERMINISM_POLICY,
        fallback_policy: RegressionHarnessFallbackPolicy {
            schema_version: VALIDATION_FALLBACK_POLICY_SCHEMA_VERSION,
            unknown_date_policy: VALIDATION_UNKNOWN_DATE_FALLBACK_POLICY,
            unsupported_exchange_calendar_segment_policy:
                VALIDATION_UNSUPPORTED_CALENDAR_SEGMENT_POLICY,
            incomplete_fixture_coverage_policy: VALIDATION_INCOMPLETE_FIXTURE_COVERAGE_POLICY,
        },
        tolerance_config,
        summary: RegressionHarnessSummary {
            total_case_count,
            passed_case_count,
            failed_case_count,
        },
        case_reports,
    })
}
fn validate_tolerance_config(
    tolerance_config: &RegressionHarnessToleranceConfig,
) -> Result<(), RegressionHarnessError> {
    if !tolerance_config.seasonality_absolute_tolerance.is_finite()
        || tolerance_config.seasonality_absolute_tolerance < 0.0
    {
        return Err(
            RegressionHarnessError::InvalidSeasonalityAbsoluteTolerance {
                value: tolerance_config.seasonality_absolute_tolerance,
            },
        );
    }

    if !tolerance_config
        .news_intensity_absolute_tolerance
        .is_finite()
        || tolerance_config.news_intensity_absolute_tolerance < 0.0
    {
        return Err(
            RegressionHarnessError::InvalidNewsIntensityAbsoluteTolerance {
                value: tolerance_config.news_intensity_absolute_tolerance,
            },
        );
    }

    Ok(())
}
fn evaluate_calendar_case(
    calendar_registry: &ExchangeCalendarRegistry,
    calendar_case: &CalendarStateRegressionCase,
) -> RegressionCaseReport {
    let timestamp_utc = calendar_case.timestamp_utc.to_rfc3339();

    match calendar_registry
        .resolve_market_state_at_utc(&calendar_case.exchange_code, calendar_case.timestamp_utc)
    {
        Ok(observed_state) => {
            if observed_state == calendar_case.expected_state {
                return RegressionCaseReport {
                    fixture_id: calendar_case.fixture_id.clone(),
                    case_kind: RegressionCaseKind::CalendarMarketState,
                    status: RegressionCaseStatus::Pass,
                    summary: format!(
                        "resolved expected market state `{}`",
                        calendar_case.expected_state
                    ),
                    failure: None,
                };
            }

            RegressionCaseReport {
                fixture_id: calendar_case.fixture_id.clone(),
                case_kind: RegressionCaseKind::CalendarMarketState,
                status: RegressionCaseStatus::Fail,
                summary: format!(
                    "expected market state `{}` but observed `{}`",
                    calendar_case.expected_state, observed_state
                ),
                failure: Some(RegressionCaseFailure::MarketStateMismatch {
                    exchange_code: calendar_case.exchange_code.clone(),
                    timestamp_utc,
                    expected_state: calendar_case.expected_state.to_string(),
                    observed_state: observed_state.to_string(),
                }),
            }
        }
        Err(source) => RegressionCaseReport {
            fixture_id: calendar_case.fixture_id.clone(),
            case_kind: RegressionCaseKind::CalendarMarketState,
            status: RegressionCaseStatus::Fail,
            summary: format!("failed to resolve market state: {source}"),
            failure: Some(RegressionCaseFailure::CalendarResolutionError {
                exchange_code: calendar_case.exchange_code.clone(),
                timestamp_utc,
                error_message: source.to_string(),
            }),
        },
    }
}

fn evaluate_hourly_factor_case(
    calendar_registry: &ExchangeCalendarRegistry,
    hourly_factor_case: &HourlyFactorRegressionCase,
    tolerance_config: &RegressionHarnessToleranceConfig,
) -> RegressionCaseReport {
    let contract_output = resolve_hourly_factor_contract(calendar_registry, hourly_factor_case);
    let contract_output = match contract_output {
        Ok(output) => output,
        Err(error) => {
            return RegressionCaseReport {
                fixture_id: hourly_factor_case.fixture_id.clone(),
                case_kind: RegressionCaseKind::HourlyFactorOutput,
                status: RegressionCaseStatus::Fail,
                summary: format!("failed to resolve hourly factor output: {error}"),
                failure: Some(RegressionCaseFailure::HourlyFactorResolutionError {
                    error_message: error.to_string(),
                }),
            };
        }
    };

    if let Some(failure) = compare_factor_vectors(
        "s_h",
        &hourly_factor_case.expected_s_h,
        &contract_output.s_h,
        tolerance_config.seasonality_absolute_tolerance,
    ) {
        return RegressionCaseReport {
            fixture_id: hourly_factor_case.fixture_id.clone(),
            case_kind: RegressionCaseKind::HourlyFactorOutput,
            status: RegressionCaseStatus::Fail,
            summary: "seasonality factor output mismatch".to_string(),
            failure: Some(failure),
        };
    }

    if let Some(failure) = compare_factor_vectors(
        "nu_h",
        &hourly_factor_case.expected_nu_h,
        &contract_output.nu_h,
        tolerance_config.news_intensity_absolute_tolerance,
    ) {
        return RegressionCaseReport {
            fixture_id: hourly_factor_case.fixture_id.clone(),
            case_kind: RegressionCaseKind::HourlyFactorOutput,
            status: RegressionCaseStatus::Fail,
            summary: "news intensity factor output mismatch".to_string(),
            failure: Some(failure),
        };
    }

    RegressionCaseReport {
        fixture_id: hourly_factor_case.fixture_id.clone(),
        case_kind: RegressionCaseKind::HourlyFactorOutput,
        status: RegressionCaseStatus::Pass,
        summary: "hourly factor outputs matched expected vectors".to_string(),
        failure: None,
    }
}
fn resolve_hourly_factor_contract(
    calendar_registry: &ExchangeCalendarRegistry,
    hourly_factor_case: &HourlyFactorRegressionCase,
) -> Result<crate::hourly_factors::HourlyFactorContractOutput, HourlyFactorProviderError> {
    let seasonality_source = match &hourly_factor_case.seasonality_source {
        HourlySeasonalityRegressionSource::Explicit {
            hourly_factors,
            materialization_method,
        } => HourlySeasonalityFactorSource::Explicit {
            hourly_factors,
            materialization_method,
        },
        HourlySeasonalityRegressionSource::Materialized {
            source_timestamp_utc_nanos,
            source_seasonality_factors,
            interpolation_method,
            aggregation_method,
            sparse_fallback_policy,
            sparse_hour_minimum_observation_count,
        } => HourlySeasonalityFactorSource::Materialized {
            source_timestamp_utc_nanos,
            source_seasonality_factors,
            interpolation_method,
            aggregation_method,
            sparse_fallback_policy,
            sparse_hour_minimum_observation_count: *sparse_hour_minimum_observation_count,
        },
    };

    let news_intensity_source = match &hourly_factor_case.news_intensity_source {
        HourlyNewsIntensityRegressionSource::Explicit {
            hourly_factors,
            materialization_method,
        } => HourlyNewsIntensityFactorSource::Explicit {
            hourly_factors,
            materialization_method,
        },
        HourlyNewsIntensityRegressionSource::Materialized {
            known_scheduled_event_timestamps_utc_nanos,
            surprise_baseline_intensity,
            surprise_deterministic_time_multiplier_by_interval_index,
        } => HourlyNewsIntensityFactorSource::Materialized {
            known_scheduled_event_timestamps_utc_nanos,
            surprise_baseline_intensity: *surprise_baseline_intensity,
            surprise_deterministic_time_multiplier_by_interval_index,
        },
    };

    provide_hourly_factor_contract(
        calendar_registry,
        HourlyFactorProviderRequest {
            exchange_code: &hourly_factor_case.exchange_code,
            range_start_utc: hourly_factor_case.range_start_utc,
            range_end_utc: hourly_factor_case.range_end_utc,
            interval_duration_nanos: 3_600_000_000_000,
            seasonality_source: Some(seasonality_source),
            news_intensity_source: Some(news_intensity_source),
        },
    )
}

fn compare_factor_vectors(
    field_name: &'static str,
    expected_values: &[f64],
    observed_values: &[f64],
    absolute_tolerance: f64,
) -> Option<RegressionCaseFailure> {
    if expected_values.len() != observed_values.len() {
        return Some(RegressionCaseFailure::FactorLengthMismatch {
            field_name,
            expected_length: expected_values.len(),
            observed_length: observed_values.len(),
        });
    }

    for value_index in 0..expected_values.len() {
        let expected_value = expected_values[value_index];
        let observed_value = observed_values[value_index];

        let absolute_difference = (expected_value - observed_value).abs();
        if !absolute_difference.is_finite() || absolute_difference > absolute_tolerance {
            return Some(RegressionCaseFailure::FactorToleranceExceeded {
                field_name,
                value_index,
                expected_value,
                observed_value,
                absolute_difference,
                absolute_tolerance,
            });
        }
    }

    None
}
