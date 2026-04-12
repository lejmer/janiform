from __future__ import annotations

from datetime import date, datetime
from pathlib import Path
from typing import TYPE_CHECKING, Any, Optional

from .....pricing import (
    AnchoredVanillaOptionPricingBoundaryInput,
    PricingIntervalSchedule,
    PricingIntervalSchedulePolicy,
    VanillaOptionPayoffConfiguration,
    VanillaOptionPayoffType,
    VanillaOptionPricingExecutionInput,
    VanillaOptionPricingMarketInputs,
    VanillaOptionPricingModelParameters,
    VanillaOptionPricingOptionalOutputSelection,
    VanillaOptionPricingRequest,
    VanillaOptionPricingRunResult,
    run_vanilla_option_pricing,
)
from .....simulation.shared import HawkesExcitationState
from ....._workflow_outputs import generate_workflow_run_id, write_pricing_artifacts
from ....._workflow_progress import JaniformOutputController, WorkflowProgressMode
from ....errors import calibration_error
from ...pricing.grids import (
    JaniformVanillaOptionGridBatch,
    JaniformVanillaOptionGridBatchPricingResult,
)
from ...pricing.surface import (
    JaniformVanillaOptionSurface,
    JaniformVanillaOptionSurfacePricingResult,
)
from ...pricing.internal.maturity import resolve_pricing_maturity_axis, resolve_single_pricing_maturity_input
from ...state import CurrentStateEstimationRequest
from .support import (
    build_model_pricing_hourly_factor_contract,
    price_observed_log_price_from_spot,
    runtime_schedule_hourly_factor_contract_coverage,
)
from ..payloads import (
    mean_reversion_fitted_parameters_for_model,
    prepare_model_runtime_factors_for_model,
)
from ..mean_reversion import prepare_model_mean_reversion_runtime_inputs
from .native import (
    run_model_vanilla_option_grid_batch_pricing,
    run_model_vanilla_option_surface_pricing,
)
from ..state import estimate_model_state_from_observed_data
from ..shared import PreparedModelPricingRuntimeInput

if TYPE_CHECKING:
    from ...model import JaniformModel


def price_option_for_model(
    model: JaniformModel,
    *,
    valuation_time_utc: datetime,
    spot: float,
    strike: float,
    expiry_timestamp_utc: Optional[datetime] = None,
    expiry_date: Optional[date] = None,
    tenor: Optional[str] = None,
    payoff_type: VanillaOptionPayoffType | str = VanillaOptionPayoffType.CALL,
    risk_free_rate: float = 0.0,
    dividend_yield: float = 0.0,
    observed_data: Optional[CurrentStateEstimationRequest] = None,
    boundary_state: Optional[HawkesExcitationState] = None,
    include_transform_diagnostics: bool = False,
    include_integration_quality_diagnostics: bool = False,
    include_calibration_hooks: bool = False,
    pricing_interval_schedule_policy: Optional[PricingIntervalSchedulePolicy] = None,
    output_path: Optional[str | Path] = None,
    silent: bool = False,
    progress_mode: WorkflowProgressMode | str = WorkflowProgressMode.AUTO,
) -> VanillaOptionPricingRunResult:
    output_controller = JaniformOutputController(
        suppress_info_output=silent,
        progress_mode=WorkflowProgressMode.NONE if silent else progress_mode,
        default_progress_mode=(WorkflowProgressMode.NONE if silent else WorkflowProgressMode.HUMAN),
    )

    execution_input = prepare_model_pricing_execution_input(
        model,
        valuation_time_utc=valuation_time_utc,
        spot=spot,
        strike=strike,
        expiry_timestamp_utc=expiry_timestamp_utc,
        expiry_date=expiry_date,
        tenor=tenor,
        payoff_type=payoff_type,
        risk_free_rate=risk_free_rate,
        dividend_yield=dividend_yield,
        observed_data=observed_data,
        boundary_state=boundary_state,
        include_transform_diagnostics=include_transform_diagnostics,
        include_integration_quality_diagnostics=include_integration_quality_diagnostics,
        include_calibration_hooks=include_calibration_hooks,
        pricing_interval_schedule_policy=pricing_interval_schedule_policy,
    )

    with output_controller.progress("price") as progress:
        progress.update(detail="running pricing workflow")
        pricing_result = run_vanilla_option_pricing(execution_input)

    if output_path is not None:
        pricing_run_id = generate_workflow_run_id(f"{model.run_id}_price")
        write_pricing_artifacts(
            model=model,
            pricing_result=pricing_result,
            output_path=output_path,
            pricing_run_id=pricing_run_id,
        )

    return pricing_result


def price_option_grids_for_model(
    model: JaniformModel,
    *,
    valuation_time_utc: datetime,
    batch: JaniformVanillaOptionGridBatch,
    observed_data: Optional[CurrentStateEstimationRequest] = None,
    boundary_state: Optional[HawkesExcitationState] = None,
    include_transform_diagnostics: bool = False,
    include_integration_quality_diagnostics: bool = False,
    pricing_interval_schedule_policy: Optional[PricingIntervalSchedulePolicy] = None,
    cpu_count: Optional[int] = None,
    silent: bool = False,
    progress_mode: WorkflowProgressMode | str = WorkflowProgressMode.AUTO,
) -> JaniformVanillaOptionGridBatchPricingResult:
    output_controller = JaniformOutputController(
        suppress_info_output=silent,
        progress_mode=WorkflowProgressMode.NONE if silent else progress_mode,
        default_progress_mode=(WorkflowProgressMode.NONE if silent else WorkflowProgressMode.HUMAN),
    )

    native_execution_input = prepare_model_grid_batch_pricing_native_input(
        model,
        valuation_time_utc=valuation_time_utc,
        batch=batch,
        observed_data=observed_data,
        boundary_state=boundary_state,
        include_transform_diagnostics=include_transform_diagnostics,
        include_integration_quality_diagnostics=include_integration_quality_diagnostics,
        pricing_interval_schedule_policy=pricing_interval_schedule_policy,
        cpu_count=cpu_count,
    )

    with output_controller.progress("price") as progress:
        progress.update(detail="running grid batch pricing workflow")
        return run_model_vanilla_option_grid_batch_pricing(
            native_execution_input,
            progress=progress,
        )


def price_option_surface_for_model(
    model: JaniformModel,
    *,
    valuation_time_utc: datetime,
    surface: JaniformVanillaOptionSurface,
    observed_data: Optional[CurrentStateEstimationRequest] = None,
    boundary_state: Optional[HawkesExcitationState] = None,
    include_transform_diagnostics: bool = False,
    include_integration_quality_diagnostics: bool = False,
    pricing_interval_schedule_policy: Optional[PricingIntervalSchedulePolicy] = None,
    cpu_count: Optional[int] = None,
    silent: bool = False,
    progress_mode: WorkflowProgressMode | str = WorkflowProgressMode.AUTO,
) -> JaniformVanillaOptionSurfacePricingResult:
    output_controller = JaniformOutputController(
        suppress_info_output=silent,
        progress_mode=WorkflowProgressMode.NONE if silent else progress_mode,
        default_progress_mode=(WorkflowProgressMode.NONE if silent else WorkflowProgressMode.HUMAN),
    )

    native_execution_input = prepare_model_surface_pricing_native_input(
        model,
        valuation_time_utc=valuation_time_utc,
        surface=surface,
        observed_data=observed_data,
        boundary_state=boundary_state,
        include_transform_diagnostics=include_transform_diagnostics,
        include_integration_quality_diagnostics=include_integration_quality_diagnostics,
        pricing_interval_schedule_policy=pricing_interval_schedule_policy,
        cpu_count=cpu_count,
    )

    with output_controller.progress("price") as progress:
        progress.update(detail="running surface pricing workflow")
        return run_model_vanilla_option_surface_pricing(
            native_execution_input,
            progress=progress,
        )


def prepare_model_pricing_execution_input(
    model: JaniformModel,
    *,
    valuation_time_utc: datetime,
    spot: float,
    strike: float,
    expiry_timestamp_utc: Optional[datetime],
    expiry_date: Optional[date],
    tenor: Optional[str],
    payoff_type: VanillaOptionPayoffType | str,
    risk_free_rate: float,
    dividend_yield: float,
    observed_data: Optional[CurrentStateEstimationRequest] = None,
    boundary_state: Optional[HawkesExcitationState],
    include_transform_diagnostics: bool,
    include_integration_quality_diagnostics: bool,
    include_calibration_hooks: bool,
    pricing_interval_schedule_policy: Optional[PricingIntervalSchedulePolicy],
) -> VanillaOptionPricingExecutionInput:
    resolved_maturity_inputs = resolve_single_pricing_maturity_input(
        valuation_time_utc=valuation_time_utc,
        expiry_timestamp_utc=expiry_timestamp_utc,
        expiry_date=expiry_date,
        tenor=tenor,
        pricing_interval_schedule_policy=pricing_interval_schedule_policy,
    )
    maturity_interval = resolved_maturity_inputs.maturity_intervals[0]

    runtime_input = prepare_model_pricing_runtime_input(
        model,
        valuation_time_utc=valuation_time_utc,
        spot=spot,
        runtime_interval_schedule=resolved_maturity_inputs.pricing_interval_schedule,
        observed_data=observed_data,
        boundary_state=boundary_state,
    )

    try:
        normalized_payoff_type = (
            payoff_type if isinstance(payoff_type, VanillaOptionPayoffType) else VanillaOptionPayoffType(payoff_type)
        )
    except ValueError as error:
        raise calibration_error("payoff_type", "must be one of: call, put") from error

    return VanillaOptionPricingExecutionInput(
        request=VanillaOptionPricingRequest(
            market_inputs=VanillaOptionPricingMarketInputs(
                spot=spot,
                strikes=(strike,),
                maturity_intervals=(maturity_interval,),
                risk_free_rate=risk_free_rate,
                dividend_yield=dividend_yield,
            ),
            payoff=VanillaOptionPayoffConfiguration(payoff_type=normalized_payoff_type),
            boundary_input=AnchoredVanillaOptionPricingBoundaryInput(
                interval_index=0,
                hawkes_boundary_state=runtime_input.effective_boundary_state,
                mean_reversion_fitted_parameters=runtime_input.mean_reversion_fitted_parameters,
                interval_delta=resolved_maturity_inputs.pricing_interval_schedule.first_interval_delta_hours,
                observed_boundary_state=runtime_input.effective_observed_boundary_state,
            ),
            model_parameters=runtime_input.model_parameters,
            interval_delta=resolved_maturity_inputs.pricing_interval_schedule.first_interval_delta_hours,
        ),
        runtime_interval_schedule=resolved_maturity_inputs.pricing_interval_schedule,
        hourly_factor_contract=runtime_input.hourly_factor_contract,
        optional_output_selection=VanillaOptionPricingOptionalOutputSelection(
            include_transform_diagnostics=include_transform_diagnostics,
            include_integration_quality_diagnostics=include_integration_quality_diagnostics,
            include_calibration_hooks=include_calibration_hooks,
        ),
    )


def prepare_model_pricing_runtime_input(
    model: JaniformModel,
    *,
    valuation_time_utc: datetime,
    spot: float,
    runtime_interval_schedule: PricingIntervalSchedule,
    observed_data: Optional[CurrentStateEstimationRequest],
    boundary_state: Optional[HawkesExcitationState],
) -> PreparedModelPricingRuntimeInput:
    runtime_factors = prepare_model_runtime_factors_for_model(
        model,
        start_time_utc=valuation_time_utc,
        horizon_interval_count=1,
        interval="1h",
        prefer_joint_pricing_block=True,
    )

    boundary_hourly_factor_contract = build_model_pricing_hourly_factor_contract(
        model,
        runtime_factors=runtime_factors,
        range_start_utc=valuation_time_utc,
        range_end_utc=runtime_factors.range_end_utc,
    )

    pricing_hourly_coverage_start_utc, pricing_hourly_coverage_end_utc = (
        runtime_schedule_hourly_factor_contract_coverage(
            valuation_time_utc,
            runtime_interval_schedule,
        )
    )

    pricing_hourly_factor_contract = build_model_pricing_hourly_factor_contract(
        model,
        runtime_factors=runtime_factors,
        range_start_utc=pricing_hourly_coverage_start_utc,
        range_end_utc=pricing_hourly_coverage_end_utc,
    )

    mean_reversion_fitted_parameters = mean_reversion_fitted_parameters_for_model(model)

    if boundary_state is not None:
        effective_boundary_state = boundary_state

        (_, _, effective_observed_boundary_state) = prepare_model_mean_reversion_runtime_inputs(
            model,
            runtime_factors=runtime_factors,
            hourly_factor_contract=boundary_hourly_factor_contract,
            initial_excitation_state=effective_boundary_state,
            start_anchor_value=price_observed_log_price_from_spot(model, spot=spot),
        )
    else:
        pricing_state = estimate_model_state_from_observed_data(
            model,
            state_time_utc=valuation_time_utc,
            request=observed_data,
            observed_price=spot,
        )
        if pricing_state is not None:
            effective_boundary_state = pricing_state.hawkes_excitation_state
            effective_observed_boundary_state = pricing_state.mean_reversion_boundary_state
        else:
            effective_boundary_state = runtime_factors.kernel_parameters.stationary_excitation_state()

            (_, _, effective_observed_boundary_state) = prepare_model_mean_reversion_runtime_inputs(
                model,
                runtime_factors=runtime_factors,
                hourly_factor_contract=boundary_hourly_factor_contract,
                initial_excitation_state=effective_boundary_state,
                start_anchor_value=price_observed_log_price_from_spot(model, spot=spot),
            )

    return PreparedModelPricingRuntimeInput(
        runtime_factors=runtime_factors,
        hourly_factor_contract=pricing_hourly_factor_contract,
        effective_boundary_state=effective_boundary_state,
        effective_observed_boundary_state=effective_observed_boundary_state,
        mean_reversion_fitted_parameters=mean_reversion_fitted_parameters,
        model_parameters=VanillaOptionPricingModelParameters(
            hawkes_kernel_parameters=runtime_factors.kernel_parameters,
            news_process_configuration=runtime_factors.news_process_configuration,
            nig_shared_plus_shift_configuration=runtime_factors.nig_shared_plus_shift_configuration,
        ),
    )


def prepare_model_grid_batch_pricing_native_input(
    model: JaniformModel,
    *,
    valuation_time_utc: datetime,
    batch: JaniformVanillaOptionGridBatch,
    observed_data: Optional[CurrentStateEstimationRequest],
    boundary_state: Optional[HawkesExcitationState],
    include_transform_diagnostics: bool,
    include_integration_quality_diagnostics: bool,
    pricing_interval_schedule_policy: Optional[PricingIntervalSchedulePolicy] = None,
    cpu_count: Optional[int],
) -> dict[str, Any]:
    resolved_grid_maturity_inputs = tuple(
        resolve_pricing_maturity_axis(
            valuation_time_utc=valuation_time_utc,
            expiry_timestamps_utc=grid.expiry_timestamps_utc,
            expiry_dates=grid.expiry_dates,
            tenors=grid.tenors,
            field_path=f"batch.grids[{grid_index}]",
            pricing_interval_schedule_policy=pricing_interval_schedule_policy,
        )
        for grid_index, grid in enumerate(batch.grids)
    )

    runtime_schedule_resolution = resolve_pricing_maturity_axis(
        valuation_time_utc=valuation_time_utc,
        expiry_timestamps_utc=tuple(
            resolved_expiry_timestamp_utc
            for resolved_grid in resolved_grid_maturity_inputs
            for resolved_expiry_timestamp_utc in resolved_grid.resolved_expiry_timestamps_utc
        ),
        expiry_dates=None,
        tenors=None,
        field_path="batch.runtime_interval_schedule",
        pricing_interval_schedule_policy=pricing_interval_schedule_policy,
    )

    runtime_input = prepare_model_pricing_runtime_input(
        model,
        valuation_time_utc=valuation_time_utc,
        spot=batch.spot,
        runtime_interval_schedule=runtime_schedule_resolution.pricing_interval_schedule,
        observed_data=observed_data,
        boundary_state=boundary_state,
    )

    return {
        "request": {
            "spot": batch.spot,
            "grids": [
                {
                    "payoff_type": grid.payoff_type.value,
                    "risk_free_rate": grid.risk_free_rate,
                    "dividend_yield": grid.dividend_yield,
                    "strikes": list(grid.strikes),
                    "maturity_intervals": list(resolved_grid.maturity_intervals),
                }
                for grid, resolved_grid in zip(batch.grids, resolved_grid_maturity_inputs, strict=True)
            ],
        },
        "boundary_input": AnchoredVanillaOptionPricingBoundaryInput(
            interval_index=0,
            hawkes_boundary_state=runtime_input.effective_boundary_state,
            mean_reversion_fitted_parameters=runtime_input.mean_reversion_fitted_parameters,
            interval_delta=runtime_schedule_resolution.pricing_interval_schedule.first_interval_delta_hours,
            observed_boundary_state=runtime_input.effective_observed_boundary_state,
        ).to_payload(),
        "model_parameters": runtime_input.model_parameters.to_payload(),
        "runtime_interval_schedule": runtime_schedule_resolution.pricing_interval_schedule.to_payload(),
        "interval_delta": runtime_schedule_resolution.pricing_interval_schedule.first_interval_delta_hours,
        "hourly_factor_contract": runtime_input.hourly_factor_contract.to_payload(),
        "optional_output_selection": VanillaOptionPricingOptionalOutputSelection(
            include_transform_diagnostics=include_transform_diagnostics,
            include_integration_quality_diagnostics=include_integration_quality_diagnostics,
            include_calibration_hooks=False,
        ).to_payload(),
        "cpu_count": cpu_count,
    }


def prepare_model_surface_pricing_native_input(
    model: JaniformModel,
    *,
    valuation_time_utc: datetime,
    surface: JaniformVanillaOptionSurface,
    observed_data: Optional[CurrentStateEstimationRequest],
    boundary_state: Optional[HawkesExcitationState],
    include_transform_diagnostics: bool,
    include_integration_quality_diagnostics: bool,
    pricing_interval_schedule_policy: Optional[PricingIntervalSchedulePolicy] = None,
    cpu_count: Optional[int],
) -> dict[str, Any]:
    resolved_surface_maturity_inputs = resolve_pricing_maturity_axis(
        valuation_time_utc=valuation_time_utc,
        expiry_timestamps_utc=surface.expiry_timestamps_utc,
        expiry_dates=surface.expiry_dates,
        tenors=surface.tenors,
        field_path="surface",
        pricing_interval_schedule_policy=pricing_interval_schedule_policy,
    )

    runtime_input = prepare_model_pricing_runtime_input(
        model,
        valuation_time_utc=valuation_time_utc,
        spot=surface.spot,
        runtime_interval_schedule=resolved_surface_maturity_inputs.pricing_interval_schedule,
        observed_data=observed_data,
        boundary_state=boundary_state,
    )

    return {
        "request": {
            "market_inputs": {
                "spot": surface.spot,
                "strikes": list(surface.strikes),
                "maturity_intervals": list(resolved_surface_maturity_inputs.maturity_intervals),
                "risk_free_rate": surface.risk_free_rate,
                "dividend_yield": surface.dividend_yield,
            },
            "payoff": {
                "payoff_type": surface.payoff_type.value,
            },
        },
        "boundary_input": AnchoredVanillaOptionPricingBoundaryInput(
            interval_index=0,
            hawkes_boundary_state=runtime_input.effective_boundary_state,
            mean_reversion_fitted_parameters=runtime_input.mean_reversion_fitted_parameters,
            interval_delta=resolved_surface_maturity_inputs.pricing_interval_schedule.first_interval_delta_hours,
            observed_boundary_state=runtime_input.effective_observed_boundary_state,
        ).to_payload(),
        "model_parameters": runtime_input.model_parameters.to_payload(),
        "runtime_interval_schedule": resolved_surface_maturity_inputs.pricing_interval_schedule.to_payload(),
        "interval_delta": resolved_surface_maturity_inputs.pricing_interval_schedule.first_interval_delta_hours,
        "hourly_factor_contract": runtime_input.hourly_factor_contract.to_payload(),
        "optional_output_selection": VanillaOptionPricingOptionalOutputSelection(
            include_transform_diagnostics=include_transform_diagnostics,
            include_integration_quality_diagnostics=include_integration_quality_diagnostics,
            include_calibration_hooks=False,
        ).to_payload(),
        "cpu_count": cpu_count,
    }
