import json
from typing import Any

from janiform.hourly_factors import provide_hourly_factor_contract
from janiform.simulation.seeded import (
    SeededTickSimulationRequest,
    build_disabled_seeded_mean_reversion_runtime_inputs,
    run_seeded_tick_event_simulation,
)

from .support._shared import (
    SIMULATION_RANGE_END_UTC,
    SIMULATION_RANGE_START_UTC,
    build_calendar_registry,
    build_hawkes_kernel_parameters,
    build_initial_excitation_state,
    build_simulation_news_process_configuration,
    build_simulation_news_source,
    build_simulation_nig_shared_plus_shift_configuration,
    build_simulation_seasonality_source,
)


def run_example() -> dict[str, Any]:
    calendar_registry = build_calendar_registry()
    hourly_factor_contract = provide_hourly_factor_contract(
        calendar_registry=calendar_registry,
        exchange_code="XNYS",
        range_start_utc=SIMULATION_RANGE_START_UTC,
        range_end_utc=SIMULATION_RANGE_END_UTC,
        seasonality_source=build_simulation_seasonality_source(),
        news_intensity_source=build_simulation_news_source(),
    )

    (
        mean_reversion_fitted_parameters,
        mean_reversion_runtime_interval_anchors,
        initial_mean_reversion_boundary_state,
    ) = build_disabled_seeded_mean_reversion_runtime_inputs(
        hourly_factor_contract=hourly_factor_contract
    )

    request = SeededTickSimulationRequest.from_runtime_inputs(
        seed=17,
        start_time_utc=SIMULATION_RANGE_START_UTC,
        horizon_interval_count=hourly_factor_contract.interval_count,
        kernel_parameters=build_hawkes_kernel_parameters(),
        nig_shared_plus_shift_configuration=build_simulation_nig_shared_plus_shift_configuration(),
        news_process_configuration=build_simulation_news_process_configuration(),
        initial_excitation_state=build_initial_excitation_state(),
        hourly_factor_contract=hourly_factor_contract,
        mean_reversion_fitted_parameters=mean_reversion_fitted_parameters,
        mean_reversion_runtime_interval_anchors=mean_reversion_runtime_interval_anchors,
        initial_mean_reversion_boundary_state=initial_mean_reversion_boundary_state,
    )

    response = run_seeded_tick_event_simulation(request)

    first_transition = response.fine_path[0].to_payload() if response.fine_path else None
    last_transition = response.fine_path[-1].to_payload() if response.fine_path else None

    return {
        "request_summary": {
            "seed": request.seed,
            "horizon_interval_count": request.horizon_interval_count,
            "hourly_factor_interval_count": request.hourly_factor_summary.interval_count,
        },
        "response_summary": {
            "schema_version": response.schema_version,
            "transition_count": len(response.fine_path),
            "event_projection_count": len(response.event_tape),
            "first_transition": first_transition,
            "last_transition": last_transition,
        },
    }


def main() -> None:
    print(json.dumps(run_example(), indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
