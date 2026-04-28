use chrono::{DateTime, Utc};
use janiform::calendar::MarketState;
use janiform::hourly_factors::{HourlyFactorContractOutput, ONE_HOUR_NANOS};

use crate::error::WorkflowError;

use super::ModelArtifactSimulationKind;
use super::ModelArtifactSimulationWorkflowRequest;
use super::context::ModelRuntimeContext;
use super::support::validation_error;

const CLOSED_INTERVAL_SEASONALITY_FACTOR: f64 = 0.0;

pub(super) fn build_hourly_factor_contract(
    request: &ModelArtifactSimulationWorkflowRequest,
    context: &ModelRuntimeContext,
) -> Result<HourlyFactorContractOutput, WorkflowError> {
    let mut hourly_factor_contract = crate::model::build_hourly_factor_contract(
        context,
        request.start_time_utc,
        context.range_end_utc,
        request.interval_duration_nanos,
    )?;

    if request.simulation_kind == ModelArtifactSimulationKind::CoarseInterval {
        zero_closed_coarse_interval_seasonality_factors(&mut hourly_factor_contract, context)?;
    }

    Ok(hourly_factor_contract)
}

fn zero_closed_coarse_interval_seasonality_factors(
    hourly_factor_contract: &mut HourlyFactorContractOutput,
    context: &ModelRuntimeContext,
) -> Result<(), WorkflowError> {
    if hourly_factor_contract.interval_duration_nanos >= ONE_HOUR_NANOS {
        return Ok(());
    }

    for (interval_start_utc_nanos, seasonality_factor) in hourly_factor_contract
        .interval_start_utc_nanos
        .iter()
        .zip(hourly_factor_contract.s_h.iter_mut())
    {
        let timestamp_utc = DateTime::<Utc>::from_timestamp_nanos(*interval_start_utc_nanos);

        let market_state = context
            .calendar_registry
            .resolve_market_state_at_utc(
                context.model.session_resolution.exchange_code.as_str(),
                timestamp_utc,
            )
            .map_err(|source| validation_error("calendar_registry", source.to_string()))?;

        if market_state == MarketState::Closed {
            *seasonality_factor = CLOSED_INTERVAL_SEASONALITY_FACTOR;
        }
    }

    Ok(())
}
