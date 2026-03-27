from datetime import datetime
from typing import Any, Optional, cast

from janiform.hourly_factors import (
    HourlyNewsIntensityProviderExplicitSource,
    HourlySeasonalityProviderExplicitSource,
    provide_hourly_factor_contract,
)
from janiform.pricing import GammaSeverityMarkConfiguration, NigSharedPlusShiftConfiguration
from janiform.simulation.coarse import (
    CoupledGapMarkConfiguration,
    DoubleExponentialBaseShockConfiguration,
    KnownNewsArrivalConfiguration,
    NewsArrivalConfiguration,
    NewsComponentMarkScaleConfiguration,
    NewsProcessConfiguration,
    SharedNewsMarkLawConfiguration,
    SurpriseNewsArrivalConfiguration,
)
from janiform.simulation.seeded import (
    SeededTickSimulationRequest,
    run_seeded_tick_event_simulation,
)
from janiform.simulation.shared import (
    HawkesExcitationState,
    HawkesKernelParameters,
)
from janiform.simulation.seeded.contracts import build_disabled_seeded_mean_reversion_runtime_inputs
from tests.fixture_validation import SeededTickSimulationParityCase


def test_seeded_tick_simulation_parity_fixture_matches_python_surface(
    calendar_registry,
    seeded_tick_simulation_parity_cases: list[SeededTickSimulationParityCase],
) -> None:
    for fixture_case in seeded_tick_simulation_parity_cases:
        payload = _run_seeded_case(calendar_registry, fixture_case)
        mismatch_diagnostic = _first_mismatch_diagnostic(payload, fixture_case)

        assert mismatch_diagnostic is None, mismatch_diagnostic


def test_seeded_tick_simulation_parity_fixture_covers_short_and_medium_horizons(
    seeded_tick_simulation_parity_cases: list[SeededTickSimulationParityCase],
) -> None:
    observed_tags: set[str] = {
        tag
        for fixture_case in seeded_tick_simulation_parity_cases
        for tag in fixture_case["tags"]
    }

    assert "short_horizon" in observed_tags
    assert "medium_horizon" in observed_tags


def test_seeded_tick_simulation_mismatch_diagnostics_are_actionable(
    calendar_registry,
    seeded_tick_simulation_parity_cases: list[SeededTickSimulationParityCase],
) -> None:
    fixture_case = seeded_tick_simulation_parity_cases[0]
    payload = _run_seeded_case(calendar_registry, fixture_case)
    observed_first_side = payload.event_tape[0].side.value
    mismatched_first_side = "sell" if observed_first_side == "buy" else "buy"
    mutated_fixture_case = cast(
        SeededTickSimulationParityCase,
        {
            **fixture_case,
            "expected_side_sequence": [
                mismatched_first_side,
                *fixture_case["expected_side_sequence"][1:],
            ],
        },
    )

    mismatch_diagnostic = _first_mismatch_diagnostic(payload, mutated_fixture_case)
    assert mismatch_diagnostic is not None
    assert f"fixture_id={fixture_case['fixture_id']}" in mismatch_diagnostic
    assert "side_sequence mismatch at event_index=0" in mismatch_diagnostic


def _run_seeded_case(
    calendar_registry,
    fixture_case: SeededTickSimulationParityCase,
) -> Any:
    range_start_utc = datetime.fromisoformat(fixture_case["range_start_utc"].replace("Z", "+00:00"))
    range_end_utc = datetime.fromisoformat(fixture_case["range_end_utc"].replace("Z", "+00:00"))

    hourly_factor_contract = provide_hourly_factor_contract(
        calendar_registry=calendar_registry,
        exchange_code=fixture_case["exchange_code"],
        range_start_utc=range_start_utc,
        range_end_utc=range_end_utc,
        seasonality_source=HourlySeasonalityProviderExplicitSource(
            hourly_factors=fixture_case["seasonality_hourly_factors"],
            materialization_method="explicit_hourly_input.v1",
        ),
        news_intensity_source=HourlyNewsIntensityProviderExplicitSource(
            hourly_factors=fixture_case["news_intensity_hourly_factors"],
            materialization_method="explicit_hourly_input.v1",
        ),
    )

    (
        mean_reversion_fitted_parameters,
        mean_reversion_runtime_interval_anchors,
        initial_mean_reversion_boundary_state,
    ) = build_disabled_seeded_mean_reversion_runtime_inputs(
        hourly_factor_contract=hourly_factor_contract
    )

    request = SeededTickSimulationRequest.from_runtime_inputs(
        seed=fixture_case["seed"],
        start_time_utc=range_start_utc,
        horizon_interval_count=fixture_case["horizon_interval_count"],
        kernel_parameters=HawkesKernelParameters.from_mapping(fixture_case["kernel_parameters"]),
        nig_shared_plus_shift_configuration=NigSharedPlusShiftConfiguration(
            alpha=2.3,
            beta_0=-0.8,
            delta=0.7,
            mu=0.1,
            delta_beta=-0.2,
        ),
        news_process_configuration=NewsProcessConfiguration(
            schema_version="news.process.configuration.v1",
            arrivals=NewsArrivalConfiguration(
                known=KnownNewsArrivalConfiguration(
                    scheduled_event_timestamps_utc_nanos=(),
                ),
                surprise=SurpriseNewsArrivalConfiguration(
                    baseline_intensity=0.0,
                ),
            ),
            shared_mark_law=SharedNewsMarkLawConfiguration(
                gamma_severity_marks=GammaSeverityMarkConfiguration(
                    shape_k=1.5,
                    scale_theta=0.9,
                ),
                double_exponential_base_shock=DoubleExponentialBaseShockConfiguration(
                    probability_positive=0.4,
                    eta_plus=1.3,
                    eta_minus=1.1,
                ),
                coupled_gap_marks=CoupledGapMarkConfiguration(sigma_g=0.75),
            ),
            known_component_mark_scales=NewsComponentMarkScaleConfiguration(
                activity_coupling_scale=0.6,
                return_jump_scale=1.2,
            ),
            surprise_component_mark_scales=NewsComponentMarkScaleConfiguration(
                activity_coupling_scale=0.6,
                return_jump_scale=1.2,
            ),
        ),
        initial_excitation_state=HawkesExcitationState.from_mapping(fixture_case["initial_excitation_state"]),
        hourly_factor_contract=hourly_factor_contract,
        mean_reversion_fitted_parameters=mean_reversion_fitted_parameters,
        mean_reversion_runtime_interval_anchors=mean_reversion_runtime_interval_anchors,
        initial_mean_reversion_boundary_state=initial_mean_reversion_boundary_state,
    )

    return run_seeded_tick_event_simulation(request)


def _first_mismatch_diagnostic(
    payload: Any,
    fixture_case: SeededTickSimulationParityCase,
) -> Optional[str]:
    fixture_id = fixture_case["fixture_id"]
    event_tape = list(payload.event_tape)

    observed_timestamps = [event_record.timestamp_utc_nanos for event_record in event_tape]
    observed_side_sequence = [event_record.side.value for event_record in event_tape]

    if len(observed_timestamps) != fixture_case["expected_total_event_count"]:
        return (
            f"fixture_id={fixture_id} total_event_count mismatch "
            f"expected={fixture_case['expected_total_event_count']} observed={len(observed_timestamps)}"
        )

    if observed_timestamps != fixture_case["expected_timestamps_utc_nanos"]:
        for event_index, (observed_timestamp, expected_timestamp) in enumerate(
            zip(observed_timestamps, fixture_case["expected_timestamps_utc_nanos"]),
        ):
            if observed_timestamp != expected_timestamp:
                return (
                    f"fixture_id={fixture_id} timestamp mismatch at event_index={event_index} "
                    f"expected={expected_timestamp} observed={observed_timestamp}"
                )

        return f"fixture_id={fixture_id} timestamp length mismatch after comparison"

    if observed_side_sequence != fixture_case["expected_side_sequence"]:
        for event_index, (observed_side, expected_side) in enumerate(
            zip(observed_side_sequence, fixture_case["expected_side_sequence"]),
        ):
            if observed_side != expected_side:
                return (
                    f"fixture_id={fixture_id} side_sequence mismatch at event_index={event_index} "
                    f"expected={expected_side} observed={observed_side}"
                )

        return f"fixture_id={fixture_id} side sequence length mismatch after comparison"

    if any(
        observed_timestamp < previous_timestamp
        for previous_timestamp, observed_timestamp in zip(observed_timestamps, observed_timestamps[1:])
    ):
        return f"fixture_id={fixture_id} event timestamps are not non-decreasing"

    interval_event_counts, interval_buy_counts, interval_sell_counts = _summarize_counts_by_interval(
        observed_timestamps,
        observed_side_sequence,
        fixture_case["range_start_utc"],
        fixture_case["horizon_interval_count"],
    )

    if interval_event_counts != fixture_case["expected_interval_event_counts"]:
        return (
            f"fixture_id={fixture_id} interval_event_counts mismatch "
            f"expected={fixture_case['expected_interval_event_counts']} observed={interval_event_counts}"
        )

    if interval_buy_counts != fixture_case["expected_interval_buy_counts"]:
        return (
            f"fixture_id={fixture_id} interval_buy_counts mismatch "
            f"expected={fixture_case['expected_interval_buy_counts']} observed={interval_buy_counts}"
        )

    if interval_sell_counts != fixture_case["expected_interval_sell_counts"]:
        return (
            f"fixture_id={fixture_id} interval_sell_counts mismatch "
            f"expected={fixture_case['expected_interval_sell_counts']} observed={interval_sell_counts}"
        )

    min_positive_gap = _minimum_positive_gap_nanos(observed_timestamps)
    if min_positive_gap != fixture_case["expected_min_positive_timestamp_gap_nanos"]:
        return (
            f"fixture_id={fixture_id} minimum positive timestamp gap mismatch "
            f"expected={fixture_case['expected_min_positive_timestamp_gap_nanos']} observed={min_positive_gap}"
        )

    has_non_grid_arrival = _has_non_grid_arrival(
        observed_timestamps,
        fixture_case["range_start_utc"],
        fixture_case["horizon_interval_count"],
    )
    if has_non_grid_arrival != fixture_case["expected_has_non_grid_arrival"]:
        return (
            f"fixture_id={fixture_id} non-grid arrival mismatch "
            f"expected={fixture_case['expected_has_non_grid_arrival']} observed={has_non_grid_arrival}"
        )

    return None


def _summarize_counts_by_interval(
    observed_timestamps: list[int],
    observed_side_sequence: list[str],
    range_start_utc: str,
    horizon_interval_count: int,
) -> tuple[list[int], list[int], list[int]]:
    interval_event_counts = [0 for _ in range(horizon_interval_count)]
    interval_buy_counts = [0 for _ in range(horizon_interval_count)]
    interval_sell_counts = [0 for _ in range(horizon_interval_count)]

    range_start_utc_seconds = int(datetime.fromisoformat(range_start_utc.replace("Z", "+00:00")).timestamp())
    range_start_utc_nanos = range_start_utc_seconds * 1_000_000_000

    one_hour_nanos = 3_600_000_000_000

    for timestamp_utc_nanos, side in zip(observed_timestamps, observed_side_sequence):
        interval_index = (timestamp_utc_nanos - range_start_utc_nanos) // one_hour_nanos
        if interval_index < 0 or interval_index >= horizon_interval_count:
            raise AssertionError(
                f"event timestamp {timestamp_utc_nanos} outside [{range_start_utc}, horizon={horizon_interval_count}]",
            )

        interval_index_int = int(interval_index)
        interval_event_counts[interval_index_int] += 1

        if side == "buy":
            interval_buy_counts[interval_index_int] += 1
        else:
            interval_sell_counts[interval_index_int] += 1

    return interval_event_counts, interval_buy_counts, interval_sell_counts


def _minimum_positive_gap_nanos(observed_timestamps: list[int]) -> int:
    positive_gaps = [
        current_timestamp - previous_timestamp
        for previous_timestamp, current_timestamp in zip(observed_timestamps, observed_timestamps[1:])
        if current_timestamp > previous_timestamp
    ]

    return min(positive_gaps) if positive_gaps else 0


def _has_non_grid_arrival(
    observed_timestamps: list[int],
    range_start_utc: str,
    horizon_interval_count: int,
) -> bool:
    range_start_utc_seconds = int(datetime.fromisoformat(range_start_utc.replace("Z", "+00:00")).timestamp())
    range_start_utc_nanos = range_start_utc_seconds * 1_000_000_000
    one_hour_nanos = 3_600_000_000_000

    interval_boundaries = {
        range_start_utc_nanos + interval_index * one_hour_nanos
        for interval_index in range(horizon_interval_count + 1)
    }

    return any(timestamp_utc_nanos not in interval_boundaries for timestamp_utc_nanos in observed_timestamps)
