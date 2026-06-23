use chrono::{DateTime, Utc};
use janiform::model::hawkes::HawkesExcitationState;
use janiform::model::long_variance::LongVarianceState;
use janiform::model::market_pressure::MarketPressureState;
use janiform::pricing::{
    AnchoredVanillaOptionPricingBoundaryInput, VanillaOptionPayoffConfiguration,
    VanillaOptionPayoffType, VanillaOptionPricingIntervalFactors, VanillaOptionPricingMarketInputs,
    VanillaOptionPricingModelParameters, VanillaOptionPricingRequest,
};
use janiform_foundations::time::checked_ceil_interval_count_between_utc_nanos;
use serde_json::json;

use crate::error::WorkflowError;
use crate::model::{
    FittedModelContext, build_interval_factor_grid, build_physical_vanilla_option_model_parameters,
    build_risk_neutral_vanilla_option_model_parameters, fitted_model_context,
    model_reference_price,
};

use super::initial_state::{resolve_forecast_initial_state, validate_forecast_initial_state};
use super::numeric::usize_to_f64;
use super::output::{ForecastWarning, ForecastWarningSeverity};
use super::request::{ForecastMeasure, ForecastWorkflowRequest, validation_error};

pub(super) struct ResolvedForecastRequest {
    pub(super) horizon_interval_count: usize,
    pub(super) end_date_utc: DateTime<Utc>,
    pub(super) warnings: Vec<ForecastWarning>,
}

pub(super) struct ForecastRuntime {
    pub(super) context: FittedModelContext,
    pub(super) reference_price: f64,
    pub(super) initial_price: f64,
    pub(super) initial_log_price: f64,
    pub(super) initial_state_policy: &'static str,
    pub(super) pricing_request: VanillaOptionPricingRequest,
    pub(super) interval_factor_grids: Vec<VanillaOptionPricingIntervalFactors>,
    pub(super) interval_deltas: Vec<f64>,
}

pub(super) fn validate_request(request: &ForecastWorkflowRequest) -> Result<(), WorkflowError> {
    if matches!(
        request.initial_price,
        Some(initial_price) if initial_price <= 0.0 || !initial_price.is_finite()
    ) {
        return Err(validation_error(
            "initial_price",
            "must be finite and strictly positive",
        ));
    }

    if request.initial_price.is_some() && request.state.is_some() {
        return Err(validation_error(
            "initial_price",
            "must be omitted when state is supplied because the forecast derives price from state",
        ));
    }

    if let Some(state) = request.state.as_ref() {
        validate_forecast_initial_state(state)?;
    }

    if request.interval_duration_nanos <= 0 {
        return Err(validation_error(
            "interval",
            "must be a positive integer number of nanoseconds",
        ));
    }

    match (request.horizon_interval_count, request.end_date_utc) {
        (Some(0), _) => {
            return Err(validation_error(
                "horizon_interval_count",
                "must be a positive integer",
            ));
        }
        (Some(_), Some(_)) => {
            return Err(validation_error(
                "horizon",
                "exactly one of horizon_interval_count or end_date_utc is required",
            ));
        }
        (None, None) => {
            return Err(validation_error(
                "horizon",
                "exactly one of horizon_interval_count or end_date_utc is required",
            ));
        }
        _ => {}
    }

    if !request.risk_free_rate.is_finite() {
        return Err(validation_error("risk_free_rate", "must be finite"));
    }

    if !request.dividend_yield.is_finite() {
        return Err(validation_error("dividend_yield", "must be finite"));
    }

    if request.distribution_node_count < 2 {
        return Err(validation_error(
            "distribution_node_count",
            "must be at least 2",
        ));
    }

    match (
        request.distribution_log_return_lower,
        request.distribution_log_return_upper,
    ) {
        (Some(lower), Some(upper)) if lower.is_finite() && upper.is_finite() && upper > lower => {}
        (Some(_), Some(_)) => {
            return Err(validation_error(
                "distribution_log_return_bounds",
                "explicit support bounds must be finite and increasing",
            ));
        }
        (None, None) => {}
        _ => {
            return Err(validation_error(
                "distribution_log_return_bounds",
                "explicit support override requires both lower and upper bounds",
            ));
        }
    }

    Ok(())
}

pub(super) fn resolve_horizon(
    request: &ForecastWorkflowRequest,
) -> Result<ResolvedForecastRequest, WorkflowError> {
    let start_nanos = timestamp_nanos(request.start_date_utc, "start_date_utc")?;

    if let Some(horizon_interval_count) = request.horizon_interval_count {
        let end_date_utc = timestamp_after_intervals(
            request.start_date_utc,
            request.interval_duration_nanos,
            horizon_interval_count,
            "horizon_interval_count",
        )?;

        return Ok(ResolvedForecastRequest {
            horizon_interval_count,
            end_date_utc,
            warnings: Vec::new(),
        });
    }

    let end_date_utc = request
        .end_date_utc
        .ok_or_else(|| validation_error("end_date_utc", "missing end date"))?;

    let end_nanos = timestamp_nanos(end_date_utc, "end_date_utc")?;

    let horizon_interval_count = checked_ceil_interval_count_between_utc_nanos(
        start_nanos,
        end_nanos,
        request.interval_duration_nanos,
    )
    .ok_or_else(|| {
        validation_error(
            "end_date_utc",
            "must be after start_date_utc and representable as interval coverage",
        )
    })?;

    if horizon_interval_count == 0 {
        return Err(validation_error(
            "end_date_utc",
            "must be after start_date_utc",
        ));
    }

    let resolved_end_date_utc = timestamp_after_intervals(
        request.start_date_utc,
        request.interval_duration_nanos,
        horizon_interval_count,
        "end_date_utc",
    )?;

    let warnings = if resolved_end_date_utc != end_date_utc {
        vec![ForecastWarning {
            code: "end_date_utc_resolved_to_interval_boundary".to_string(),
            severity: ForecastWarningSeverity::Warning,
            message: "end_date_utc is not aligned to the forecast interval grid".to_string(),
            fields: json!({
                "requested_end_date_utc": end_date_utc.to_rfc3339(),
                "resolved_end_date_utc": resolved_end_date_utc.to_rfc3339(),
                "horizon_interval_count": horizon_interval_count,
            }),
        }]
    } else {
        Vec::new()
    };

    Ok(ResolvedForecastRequest {
        horizon_interval_count,
        end_date_utc: resolved_end_date_utc,
        warnings,
    })
}

pub(super) fn build_runtime(
    request: &ForecastWorkflowRequest,
    resolved: &ResolvedForecastRequest,
) -> Result<ForecastRuntime, WorkflowError> {
    let context = fitted_model_context(
        &request.model_payload,
        request.start_date_utc,
        request.interval_duration_nanos,
        resolved.horizon_interval_count,
    )?;

    let reference_price = model_reference_price(&context.model)?;

    let interval_factor_grid = build_interval_factor_grid(
        &context,
        request.start_date_utc,
        context.range_end_utc,
        request.interval_duration_nanos,
    )?;

    let initial_state =
        resolve_forecast_initial_state(request, &context, &interval_factor_grid, reference_price)?;

    let model_parameters = match request.measure {
        ForecastMeasure::Physical => build_physical_vanilla_option_model_parameters(&context)?,
        ForecastMeasure::RiskNeutral => {
            build_risk_neutral_vanilla_option_model_parameters(&context)?
        }
    };

    let boundary_input = build_boundary_input(
        &context,
        initial_state.excitation_state,
        initial_state.long_variance_state,
        initial_state.mean_reversion_boundary_state,
        initial_state.market_pressure_state,
    )?;

    let pricing_request = build_pricing_request(
        request,
        resolved,
        initial_state.initial_price,
        boundary_input,
        model_parameters,
        context.interval_delta,
    )?;

    let interval_factor_grids =
        pricing_interval_factors_from_grid(&interval_factor_grid, resolved.horizon_interval_count)?;

    let interval_deltas = vec![context.interval_delta; resolved.horizon_interval_count];

    Ok(ForecastRuntime {
        context,
        reference_price,
        initial_price: initial_state.initial_price,
        initial_log_price: initial_state.initial_log_price,
        initial_state_policy: initial_state.policy,
        pricing_request,
        interval_factor_grids,
        interval_deltas,
    })
}

pub(super) fn engine_forecast_measure(
    measure: ForecastMeasure,
) -> janiform_engine_pricing::AffineForecastMeasure {
    match measure {
        ForecastMeasure::Physical => janiform_engine_pricing::AffineForecastMeasure::Physical,
        ForecastMeasure::RiskNeutral => janiform_engine_pricing::AffineForecastMeasure::RiskNeutral,
    }
}

pub(super) fn timestamp_after_intervals(
    start_date_utc: DateTime<Utc>,
    interval_duration_nanos: i64,
    interval_count: usize,
    field: &'static str,
) -> Result<DateTime<Utc>, WorkflowError> {
    let start_nanos = timestamp_nanos(start_date_utc, field)?;

    let interval_count = i64::try_from(interval_count).map_err(|_| {
        validation_error(
            field,
            "interval count is too large for timestamp nanosecond resolution",
        )
    })?;

    let horizon_nanos = interval_duration_nanos
        .checked_mul(interval_count)
        .ok_or_else(|| validation_error(field, "forecast horizon duration overflowed"))?;

    let end_nanos = start_nanos
        .checked_add(horizon_nanos)
        .ok_or_else(|| validation_error(field, "forecast end timestamp overflowed"))?;

    Ok(DateTime::<Utc>::from_timestamp_nanos(end_nanos))
}

pub(super) fn timestamp_nanos(
    timestamp: DateTime<Utc>,
    field: &'static str,
) -> Result<i64, WorkflowError> {
    timestamp
        .timestamp_nanos_opt()
        .ok_or_else(|| validation_error(field, "timestamp is outside nanosecond range"))
}

fn build_boundary_input(
    context: &FittedModelContext,
    initial_excitation_state: HawkesExcitationState,
    initial_long_variance_state: LongVarianceState,
    mean_reversion_boundary_state: janiform::simulation::MeanReversionBoundaryState,
    market_pressure_state: MarketPressureState,
) -> Result<AnchoredVanillaOptionPricingBoundaryInput, WorkflowError> {
    AnchoredVanillaOptionPricingBoundaryInput::try_new(
        0,
        initial_excitation_state,
        initial_long_variance_state,
        market_pressure_state,
        context
            .fitted_parameters
            .return_components_fitted_parameters
            .mean_reversion_fitted_parameters,
        context.interval_delta,
        mean_reversion_boundary_state,
    )
    .map_err(|source| validation_error("boundary_input", source.to_string()))
}

fn build_pricing_request(
    request: &ForecastWorkflowRequest,
    resolved: &ResolvedForecastRequest,
    initial_price: f64,
    boundary_input: AnchoredVanillaOptionPricingBoundaryInput,
    model_parameters: VanillaOptionPricingModelParameters,
    interval_delta: f64,
) -> Result<VanillaOptionPricingRequest, WorkflowError> {
    let maturity_interval =
        interval_delta * usize_to_f64(resolved.horizon_interval_count, "horizon_interval_count")?;

    VanillaOptionPricingRequest::try_new(
        VanillaOptionPricingMarketInputs::try_new_single_node(
            initial_price,
            initial_price,
            maturity_interval,
            request.risk_free_rate,
            request.dividend_yield,
        )
        .map_err(|source| validation_error("market_inputs", source.to_string()))?,
        VanillaOptionPayoffConfiguration {
            payoff_type: VanillaOptionPayoffType::Call,
        },
        boundary_input,
        model_parameters,
        interval_delta,
    )
    .map_err(|source| validation_error("pricing_request", source.to_string()))
}

fn pricing_interval_factors_from_grid(
    interval_factor_grid: &janiform::model::factors::interval_factor_grid::IntervalFactorGrid,
    horizon_interval_count: usize,
) -> Result<Vec<VanillaOptionPricingIntervalFactors>, WorkflowError> {
    if interval_factor_grid.seasonality_factor.len() < horizon_interval_count
        || interval_factor_grid.known_news_intensity.len() < horizon_interval_count
        || interval_factor_grid.surprise_news_intensity.len() < horizon_interval_count
    {
        return Err(validation_error(
            "interval_factor_grid",
            "materialized interval factors do not cover the forecast horizon",
        ));
    }

    (0..horizon_interval_count)
        .map(|interval_position| {
            VanillaOptionPricingIntervalFactors::try_new(
                interval_factor_grid.seasonality_factor[interval_position],
                interval_factor_grid.known_news_intensity[interval_position],
                interval_factor_grid.surprise_news_intensity[interval_position],
            )
            .map_err(|source| validation_error("interval_factor_grid", source.to_string()))
        })
        .collect()
}
