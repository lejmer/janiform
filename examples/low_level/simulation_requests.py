from datetime import UTC, datetime, timedelta
from typing import Final

from janiform.calendar import CalendarRegistry
from janiform.interval_factor_grids import IntervalFactorGridContract, build_interval_factor_grid
from janiform.return_clock import ReturnClockExposureParameters
from janiform.simulation import (
    CoarseSimulationModelInputs,
    CoarseSimulationPath,
    CoarseSimulationRunRequest,
    LongVarianceParameters,
    LongVarianceState,
    SeededTickSimulationIntervalFactorGridSummary,
    SeededTickSimulationRequest,
    SeededTickSimulationResponse,
    run_coarse_simulation,
    run_seeded_tick_event_simulation,
)
from janiform.simulation.coarse import (
    COARSE_SIMULATION_RUN_REQUEST_SCHEMA_VERSION,
)
from janiform.sufficient_statistics import SufficientStatisticsModelInputs

from examples._contracts import (
    build_excitation_state,
    build_kernel_parameters,
    build_mean_reversion_runtime_contracts,
    build_news_process_configuration,
    build_nig_shared_plus_shift_configuration,
    build_sufficient_statistics_model_inputs,
)
from examples._shared import (
    calendar_registry_json,
    calendar_registry_payload,
    datetime_to_utc_nanos,
)


SIMULATION_START_UTC: Final[datetime] = datetime(2026, 2, 2, 14, 0, tzinfo=UTC)
SIMULATION_INTERVAL_COUNT: Final[int] = 2
SIMULATION_INTERVAL_DELTA: Final[float] = 1.0


def main() -> None:
    calendar_registry: CalendarRegistry = CalendarRegistry(calendar_registry_json())

    interval_factor_grid: IntervalFactorGridContract = build_interval_factor_grid(
        calendar_registry=calendar_registry,
        exchange_code="XNYS",
        range_start_utc=SIMULATION_START_UTC,
        range_end_utc=SIMULATION_START_UTC + timedelta(hours=SIMULATION_INTERVAL_COUNT),
        interval="1h",
        seasonality_factors=[1.0, 1.1],
        news_intensity_factors=[0.2, 0.1],
    )

    sufficient_statistics_model_inputs: SufficientStatisticsModelInputs = build_sufficient_statistics_model_inputs(
        interval_count=SIMULATION_INTERVAL_COUNT,
        interval_delta_hours=SIMULATION_INTERVAL_DELTA,
    )

    coarse_request: CoarseSimulationRunRequest = CoarseSimulationRunRequest(
        schema_version=COARSE_SIMULATION_RUN_REQUEST_SCHEMA_VERSION,
        seed=17,
        start_time_utc_nanos=datetime_to_utc_nanos(SIMULATION_START_UTC),
        horizon_interval_count=SIMULATION_INTERVAL_COUNT,
        interval_delta=SIMULATION_INTERVAL_DELTA,
        model_inputs=CoarseSimulationModelInputs(
            sufficient_statistics_model_inputs=sufficient_statistics_model_inputs,
            news_process_configuration=build_news_process_configuration(),
            return_clock_exposure_parameters=ReturnClockExposureParameters.neutral(),
            long_variance_parameters=LongVarianceParameters.neutral(),
            initial_long_variance_state=LongVarianceState.unit(),
            observed_price_tick_size=None,
            calendar_registry_artifact=calendar_registry_payload(),
            exchange_code="XNYS",
        ),
        interval_factor_grid=interval_factor_grid,
    )

    (
        mean_reversion_fitted_parameters,
        mean_reversion_runtime_interval_anchors,
        initial_mean_reversion_boundary_state,
    ) = build_mean_reversion_runtime_contracts(
        interval_count=SIMULATION_INTERVAL_COUNT,
        interval_delta_hours=SIMULATION_INTERVAL_DELTA,
        start_anchor_value=0.0,
        observed_log_price=0.0,
    )

    direct_seeded_request: SeededTickSimulationRequest = SeededTickSimulationRequest(
        seed=17,
        start_time_utc=SIMULATION_START_UTC,
        horizon_interval_count=SIMULATION_INTERVAL_COUNT,
        kernel_parameters=build_kernel_parameters(),
        nig_shared_plus_shift_configuration=build_nig_shared_plus_shift_configuration(),
        news_process_configuration=build_news_process_configuration(),
        initial_excitation_state=build_excitation_state(),
        mean_reversion_fitted_parameters=mean_reversion_fitted_parameters,
        mean_reversion_runtime_interval_anchors=mean_reversion_runtime_interval_anchors,
        initial_mean_reversion_boundary_state=initial_mean_reversion_boundary_state,
        interval_factor_grid_summary=SeededTickSimulationIntervalFactorGridSummary.from_contract(
            interval_factor_grid,
        ),
        return_clock_exposure_parameters=ReturnClockExposureParameters.neutral(),
        long_variance_parameters=LongVarianceParameters.neutral(),
        initial_long_variance_state=LongVarianceState.unit(),
    )

    # The runtime helper attaches the non-serializable interval-grid handle needed by execution.
    runtime_seeded_request: SeededTickSimulationRequest = SeededTickSimulationRequest.from_runtime_inputs(
        seed=17,
        start_time_utc=SIMULATION_START_UTC,
        horizon_interval_count=SIMULATION_INTERVAL_COUNT,
        kernel_parameters=build_kernel_parameters(),
        nig_shared_plus_shift_configuration=build_nig_shared_plus_shift_configuration(),
        news_process_configuration=build_news_process_configuration(),
        initial_excitation_state=build_excitation_state(),
        mean_reversion_fitted_parameters=mean_reversion_fitted_parameters,
        mean_reversion_runtime_interval_anchors=mean_reversion_runtime_interval_anchors,
        initial_mean_reversion_boundary_state=initial_mean_reversion_boundary_state,
        interval_factor_grid=interval_factor_grid,
        return_clock_exposure_parameters=ReturnClockExposureParameters.neutral(),
        long_variance_parameters=LongVarianceParameters.neutral(),
        initial_long_variance_state=LongVarianceState.unit(),
    )

    coarse_path: CoarseSimulationPath = run_coarse_simulation(coarse_request)

    seeded_response: SeededTickSimulationResponse = run_seeded_tick_event_simulation(
        runtime_seeded_request,
    )

    print(
        "request summary:",
        {
            "coarse_horizon_interval_count": coarse_request.horizon_interval_count,
            "seeded_transport_interval_count": direct_seeded_request.interval_factor_grid_summary.interval_count,
        },
    )
    print(f"coarse intervals simulated: {len(coarse_path.records)}")
    print(f"seeded events simulated: {len(seeded_response.event_tape)}")


if __name__ == "__main__":
    main()
