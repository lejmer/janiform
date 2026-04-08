from __future__ import annotations

from datetime import date, datetime
from pathlib import Path
from typing import TYPE_CHECKING, Any, Optional

from .....pricing import (
    AnchoredVanillaOptionPricingBoundaryInput,
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
from ...pricing import (
    JaniformGroupedVanillaOptionBatch,
    JaniformVanillaOptionGroupedBatchPricingResult,
    JaniformVanillaOptionSurface,
    JaniformVanillaOptionSurfacePricingResult,
)
from ...pricing.internal.maturity import resolve_pricing_maturity_axis, resolve_single_pricing_maturity_input
from ...state import CurrentStateEstimationRequest
from .support import (
    build_model_pricing_hourly_factor_contract,
    native_pricing_interval_factors,
    price_observed_log_price_from_spot,
    pricing_interval_count_from_maturity_interval,
)
from ..payloads import (
    mean_reversion_fitted_parameters_for_model,
    prepare_model_runtime_factors_for_model,
)
from ..mean_reversion import prepare_model_mean_reversion_runtime_inputs
from .native import (
    run_model_vanilla_option_grouped_batch_pricing,
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
    batch: JaniformGroupedVanillaOptionBatch,
    observed_data: Optional[CurrentStateEstimationRequest] = None,
    boundary_state: Optional[HawkesExcitationState] = None,
    include_transform_diagnostics: bool = False,
    include_integration_quality_diagnostics: bool = False,
    cpu_count: Optional[int] = None,
    silent: bool = False,
    progress_mode: WorkflowProgressMode | str = WorkflowProgressMode.AUTO,
) -> JaniformVanillaOptionGroupedBatchPricingResult:
    output_controller = JaniformOutputController(
        suppress_info_output=silent,
        progress_mode=WorkflowProgressMode.NONE if silent else progress_mode,
        default_progress_mode=(WorkflowProgressMode.NONE if silent else WorkflowProgressMode.HUMAN),
    )

    native_execution_input = prepare_model_grouped_batch_pricing_native_input(
        model,
        valuation_time_utc=valuation_time_utc,
        batch=batch,
        observed_data=observed_data,
        boundary_state=boundary_state,
        include_transform_diagnostics=include_transform_diagnostics,
        include_integration_quality_diagnostics=include_integration_quality_diagnostics,
        cpu_count=cpu_count,
    )

    with output_controller.progress("price") as progress:
        progress.update(detail="running grouped batch pricing workflow")
        return run_model_vanilla_option_grouped_batch_pricing(
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
) -> VanillaOptionPricingExecutionInput:
    resolved_maturity_inputs = resolve_single_pricing_maturity_input(
        valuation_time_utc=valuation_time_utc,
        expiry_timestamp_utc=expiry_timestamp_utc,
        expiry_date=expiry_date,
        tenor=tenor,
    )
    maturity_interval = resolved_maturity_inputs.maturity_intervals[0]
    horizon_interval_count = pricing_interval_count_from_maturity_interval(
        maturity_interval,
        field_path="maturity_input",
    )

    runtime_input = prepare_model_pricing_runtime_input(
        model,
        valuation_time_utc=valuation_time_utc,
        spot=spot,
        horizon_interval_count=horizon_interval_count,
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
                interval_delta=runtime_input.runtime_factors.interval_delta,
                observed_boundary_state=runtime_input.effective_observed_boundary_state,
            ),
            model_parameters=runtime_input.model_parameters,
        ),
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
    horizon_interval_count: int,
    observed_data: Optional[CurrentStateEstimationRequest],
    boundary_state: Optional[HawkesExcitationState],
) -> PreparedModelPricingRuntimeInput:
    runtime_factors = prepare_model_runtime_factors_for_model(
        model,
        start_time_utc=valuation_time_utc,
        horizon_interval_count=horizon_interval_count,
        interval="1h",
        prefer_joint_pricing_block=True,
    )
    hourly_factor_contract = build_model_pricing_hourly_factor_contract(
        model,
        runtime_factors=runtime_factors,
        valuation_time_utc=valuation_time_utc,
    )
    mean_reversion_fitted_parameters = mean_reversion_fitted_parameters_for_model(model)

    if boundary_state is not None:
        effective_boundary_state = boundary_state
        (_, _, effective_observed_boundary_state) = prepare_model_mean_reversion_runtime_inputs(
            model,
            runtime_factors=runtime_factors,
            hourly_factor_contract=hourly_factor_contract,
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
                hourly_factor_contract=hourly_factor_contract,
                initial_excitation_state=effective_boundary_state,
                start_anchor_value=price_observed_log_price_from_spot(model, spot=spot),
            )

    return PreparedModelPricingRuntimeInput(
        runtime_factors=runtime_factors,
        hourly_factor_contract=hourly_factor_contract,
        effective_boundary_state=effective_boundary_state,
        effective_observed_boundary_state=effective_observed_boundary_state,
        mean_reversion_fitted_parameters=mean_reversion_fitted_parameters,
        model_parameters=VanillaOptionPricingModelParameters(
            hawkes_kernel_parameters=runtime_factors.kernel_parameters,
            news_process_configuration=runtime_factors.news_process_configuration,
            nig_shared_plus_shift_configuration=runtime_factors.nig_shared_plus_shift_configuration,
        ),
    )
def prepare_model_grouped_batch_pricing_native_input(
    model: JaniformModel,
    *,
    valuation_time_utc: datetime,
    batch: JaniformGroupedVanillaOptionBatch,
    observed_data: Optional[CurrentStateEstimationRequest],
    boundary_state: Optional[HawkesExcitationState],
    include_transform_diagnostics: bool,
    include_integration_quality_diagnostics: bool,
    cpu_count: Optional[int],
) -> dict[str, Any]:
    resolved_group_maturity_inputs = tuple(
        resolve_pricing_maturity_axis(
            valuation_time_utc=valuation_time_utc,
            expiry_timestamps_utc=group.expiry_timestamps_utc,
            expiry_dates=group.expiry_dates,
            tenors=group.tenors,
            field_path=f"batch.groups[{group_index}]",
        )
        for group_index, group in enumerate(batch.groups)
    )
    interval_counts = tuple(
        pricing_interval_count_from_maturity_interval(
            maturity_interval,
            field_path=f"batch.groups[{group_index}].maturity_inputs[{maturity_index}]",
        )
        for group_index, resolved_group in enumerate(resolved_group_maturity_inputs)
        for maturity_index, maturity_interval in enumerate(resolved_group.maturity_intervals)
    )
    max_interval_count = max(interval_counts)
    runtime_input = prepare_model_pricing_runtime_input(
        model,
        valuation_time_utc=valuation_time_utc,
        spot=batch.spot,
        horizon_interval_count=max_interval_count,
        observed_data=observed_data,
        boundary_state=boundary_state,
    )

    return {
        "request": {
            "spot": batch.spot,
            "groups": [
                {
                    "payoff_type": group.payoff_type.value,
                    "risk_free_rate": group.risk_free_rate,
                    "dividend_yield": group.dividend_yield,
                    "strikes": list(group.strikes),
                    "maturity_intervals": list(resolved_group.maturity_intervals),
                }
                for group, resolved_group in zip(batch.groups, resolved_group_maturity_inputs, strict=True)
            ],
        },
        "boundary_input": AnchoredVanillaOptionPricingBoundaryInput(
            interval_index=0,
            hawkes_boundary_state=runtime_input.effective_boundary_state,
            mean_reversion_fitted_parameters=runtime_input.mean_reversion_fitted_parameters,
            interval_delta=runtime_input.runtime_factors.interval_delta,
            observed_boundary_state=runtime_input.effective_observed_boundary_state,
        ).to_payload(),
        "model_parameters": runtime_input.model_parameters.to_payload(),
        "interval_delta": runtime_input.runtime_factors.interval_delta,
        "interval_factors": native_pricing_interval_factors(
            runtime_input.hourly_factor_contract,
            horizon_interval_count=max_interval_count,
        ),
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
    cpu_count: Optional[int],
) -> dict[str, Any]:
    resolved_surface_maturity_inputs = resolve_pricing_maturity_axis(
        valuation_time_utc=valuation_time_utc,
        expiry_timestamps_utc=surface.expiry_timestamps_utc,
        expiry_dates=surface.expiry_dates,
        tenors=surface.tenors,
        field_path="surface",
    )
    interval_counts = tuple(
        pricing_interval_count_from_maturity_interval(
            maturity_interval,
            field_path=f"surface.maturity_inputs[{maturity_index}]",
        )
        for maturity_index, maturity_interval in enumerate(resolved_surface_maturity_inputs.maturity_intervals)
    )
    max_interval_count = max(interval_counts)
    runtime_input = prepare_model_pricing_runtime_input(
        model,
        valuation_time_utc=valuation_time_utc,
        spot=surface.spot,
        horizon_interval_count=max_interval_count,
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
            interval_delta=runtime_input.runtime_factors.interval_delta,
            observed_boundary_state=runtime_input.effective_observed_boundary_state,
        ).to_payload(),
        "model_parameters": runtime_input.model_parameters.to_payload(),
        "interval_delta": runtime_input.runtime_factors.interval_delta,
        "interval_factors": native_pricing_interval_factors(
            runtime_input.hourly_factor_contract,
            horizon_interval_count=max_interval_count,
        ),
        "optional_output_selection": VanillaOptionPricingOptionalOutputSelection(
            include_transform_diagnostics=include_transform_diagnostics,
            include_integration_quality_diagnostics=include_integration_quality_diagnostics,
            include_calibration_hooks=False,
        ).to_payload(),
        "cpu_count": cpu_count,
    }
