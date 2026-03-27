from datetime import UTC, datetime, timedelta
from math import log
from typing import Any, TypedDict, cast

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
    SeededTickSimulationResponse,
    run_seeded_tick_event_simulation,
)
from janiform.simulation.shared import (
    HawkesExcitationState,
    HawkesKernelParameters,
)
from janiform.simulation.seeded.contracts import build_disabled_seeded_mean_reversion_runtime_inputs

ONE_HOUR_NANOS: int = 3_600_000_000_000
START_TIME_UTC: datetime = datetime(2026, 2, 2, tzinfo=UTC)


class ClusteringMetrics(TypedDict):
    mean_fano_factor: float
    mean_lag_one_autocorrelation: float
    non_grid_run_ratio: float


class AsymmetryMetrics(TypedDict):
    mean_sell_share: float
    mean_log_sell_to_buy_ratio: float


def test_seeded_behavior_validation_is_deterministic_for_repeated_seeded_runs(
    calendar_registry,
) -> None:
    first_payload = _run_seeded_case(
        calendar_registry=calendar_registry,
        seed=91_006,
        kernel_parameters=_symmetric_kernel_parameters(),
        horizon_interval_count=24,
        seasonality_factor=1.0,
        news_intensity=0.01,
    )
    second_payload = _run_seeded_case(
        calendar_registry=calendar_registry,
        seed=91_006,
        kernel_parameters=_symmetric_kernel_parameters(),
        horizon_interval_count=24,
        seasonality_factor=1.0,
        news_intensity=0.01,
    )

    assert first_payload == second_payload


def test_seeded_behavior_validation_suppresses_surprise_news_fine_path_records_while_market_is_closed(
    calendar_registry,
) -> None:
    start_time_utc = datetime(2026, 2, 7, tzinfo=UTC)
    payload = _run_seeded_case(
        calendar_registry=calendar_registry,
        seed=91_007,
        kernel_parameters=_symmetric_kernel_parameters(),
        horizon_interval_count=48,
        seasonality_factor=1.0,
        news_intensity=1.0,
        start_time_utc=start_time_utc,
    )

    assert len(payload.event_tape) == 0
    assert all(
        not (
            record.transition_kind.value == "exogenous_news"
            and record.market_state.value == "closed"
        )
        for record in payload.fine_path
    )


def test_seeded_behavior_validation_exhibits_clustering_and_non_grid_signatures(
    calendar_registry,
) -> None:
    seed_values = list(range(20_000, 20_024))
    thresholds: dict[str, float] = {
        "minimum_fano_factor_gap": 0.35,
        "minimum_lag_one_autocorrelation_gap": 0.07,
        "minimum_cluster_lag_one_autocorrelation": 0.60,
        "minimum_non_grid_run_ratio": 0.95,
    }

    baseline_metrics = _compute_batch_clustering_metrics(
        calendar_registry=calendar_registry,
        seed_values=seed_values,
        kernel_parameters=_symmetric_kernel_parameters(),
        horizon_interval_count=72,
        seasonality_factor=1.0,
        news_intensity=0.02,
    )
    clustered_metrics = _compute_batch_clustering_metrics(
        calendar_registry=calendar_registry,
        seed_values=seed_values,
        kernel_parameters=_clustered_kernel_parameters(),
        horizon_interval_count=72,
        seasonality_factor=1.0,
        news_intensity=0.02,
    )

    fano_factor_gap = clustered_metrics["mean_fano_factor"] - baseline_metrics["mean_fano_factor"]
    lag_one_autocorrelation_gap = (
        clustered_metrics["mean_lag_one_autocorrelation"]
        - baseline_metrics["mean_lag_one_autocorrelation"]
    )

    assert fano_factor_gap >= thresholds["minimum_fano_factor_gap"], {
        "fano_factor_gap": fano_factor_gap,
        "thresholds": thresholds,
        "baseline_metrics": baseline_metrics,
        "clustered_metrics": clustered_metrics,
    }
    assert lag_one_autocorrelation_gap >= thresholds["minimum_lag_one_autocorrelation_gap"], {
        "lag_one_autocorrelation_gap": lag_one_autocorrelation_gap,
        "thresholds": thresholds,
        "baseline_metrics": baseline_metrics,
        "clustered_metrics": clustered_metrics,
    }
    assert (
        clustered_metrics["mean_lag_one_autocorrelation"]
        >= thresholds["minimum_cluster_lag_one_autocorrelation"]
    ), {
        "thresholds": thresholds,
        "clustered_metrics": clustered_metrics,
    }
    assert clustered_metrics["non_grid_run_ratio"] >= thresholds["minimum_non_grid_run_ratio"], {
        "thresholds": thresholds,
        "clustered_metrics": clustered_metrics,
    }


def test_seeded_behavior_validation_exhibits_expected_buy_sell_asymmetry(
    calendar_registry,
) -> None:
    seed_values = list(range(30_000, 30_024))
    thresholds: dict[str, float] = {
        "symmetric_sell_share_tolerance": 0.04,
        "minimum_sell_share_gap": 0.07,
        "minimum_sell_to_buy_log_ratio_gap": 0.20,
    }

    symmetric_metrics = _compute_batch_asymmetry_metrics(
        calendar_registry=calendar_registry,
        seed_values=seed_values,
        kernel_parameters=_symmetric_kernel_parameters(),
        horizon_interval_count=72,
        seasonality_factor=1.0,
        news_intensity=0.01,
    )

    sell_asymmetric_metrics = _compute_batch_asymmetry_metrics(
        calendar_registry=calendar_registry,
        seed_values=seed_values,
        kernel_parameters=_sell_asymmetric_kernel_parameters(),
        horizon_interval_count=72,
        seasonality_factor=1.0,
        news_intensity=0.01,
    )

    symmetric_sell_share_distance = abs(symmetric_metrics["mean_sell_share"] - 0.5)
    sell_share_gap = sell_asymmetric_metrics["mean_sell_share"] - symmetric_metrics["mean_sell_share"]
    log_ratio_gap = (
        sell_asymmetric_metrics["mean_log_sell_to_buy_ratio"]
        - symmetric_metrics["mean_log_sell_to_buy_ratio"]
    )

    assert symmetric_sell_share_distance <= thresholds["symmetric_sell_share_tolerance"], {
        "symmetric_sell_share_distance": symmetric_sell_share_distance,
        "thresholds": thresholds,
        "symmetric_metrics": symmetric_metrics,
    }
    assert sell_share_gap >= thresholds["minimum_sell_share_gap"], {
        "sell_share_gap": sell_share_gap,
        "thresholds": thresholds,
        "symmetric_metrics": symmetric_metrics,
        "sell_asymmetric_metrics": sell_asymmetric_metrics,
    }
    assert log_ratio_gap >= thresholds["minimum_sell_to_buy_log_ratio_gap"], {
        "log_ratio_gap": log_ratio_gap,
        "thresholds": thresholds,
        "symmetric_metrics": symmetric_metrics,
        "sell_asymmetric_metrics": sell_asymmetric_metrics,
    }


def _compute_batch_clustering_metrics(
    *,
    calendar_registry,
    seed_values: list[int],
    kernel_parameters: dict[str, Any],
    horizon_interval_count: int,
    seasonality_factor: float,
    news_intensity: float,
) -> ClusteringMetrics:
    fano_factors: list[float] = []
    lag_one_autocorrelations: list[float] = []
    non_grid_run_count = 0

    for seed in seed_values:
        payload = _run_seeded_case(
            calendar_registry=calendar_registry,
            seed=seed,
            kernel_parameters=kernel_parameters,
            horizon_interval_count=horizon_interval_count,
            seasonality_factor=seasonality_factor,
            news_intensity=news_intensity,
        )

        event_tape = list(payload.event_tape)

        interval_event_counts = _collect_interval_event_counts(
            event_tape=event_tape,
            horizon_interval_count=horizon_interval_count,
        )

        fano_factors.append(_calculate_fano_factor(interval_event_counts))
        lag_one_autocorrelations.append(_calculate_lag_one_autocorrelation(interval_event_counts))

        if _has_non_grid_arrival(event_tape):
            non_grid_run_count += 1

    return {
        "mean_fano_factor": _mean(fano_factors),
        "mean_lag_one_autocorrelation": _mean(lag_one_autocorrelations),
        "non_grid_run_ratio": non_grid_run_count / len(seed_values),
    }


def _compute_batch_asymmetry_metrics(
    *,
    calendar_registry,
    seed_values: list[int],
    kernel_parameters: dict[str, Any],
    horizon_interval_count: int,
    seasonality_factor: float,
    news_intensity: float,
) -> AsymmetryMetrics:
    sell_shares: list[float] = []
    log_sell_to_buy_ratios: list[float] = []

    for seed in seed_values:
        payload = _run_seeded_case(
            calendar_registry=calendar_registry,
            seed=seed,
            kernel_parameters=kernel_parameters,
            horizon_interval_count=horizon_interval_count,
            seasonality_factor=seasonality_factor,
            news_intensity=news_intensity,
        )

        event_tape = list(payload.event_tape)

        buy_event_count = sum(1 for event_record in event_tape if event_record.side.value == "buy")
        sell_event_count = sum(1 for event_record in event_tape if event_record.side.value == "sell")

        total_event_count = buy_event_count + sell_event_count

        if total_event_count == 0:
            continue

        sell_shares.append(sell_event_count / total_event_count)
        log_sell_to_buy_ratios.append(log((sell_event_count + 1.0) / (buy_event_count + 1.0)))

    return {
        "mean_sell_share": _mean(sell_shares),
        "mean_log_sell_to_buy_ratio": _mean(log_sell_to_buy_ratios),
    }


def _run_seeded_case(
    *,
    calendar_registry,
    seed: int,
    kernel_parameters: dict[str, Any],
    horizon_interval_count: int,
    seasonality_factor: float,
    news_intensity: float,
    start_time_utc: datetime = START_TIME_UTC,
) -> SeededTickSimulationResponse:
    range_end_utc = start_time_utc + timedelta(hours=horizon_interval_count)
    hourly_factor_contract = provide_hourly_factor_contract(
        calendar_registry=calendar_registry,
        exchange_code="XNYS",
        range_start_utc=start_time_utc,
        range_end_utc=range_end_utc,
        seasonality_source=HourlySeasonalityProviderExplicitSource(
            hourly_factors=[seasonality_factor] * horizon_interval_count,
            materialization_method="explicit_hourly_input.v1",
        ),
        news_intensity_source=HourlyNewsIntensityProviderExplicitSource(
            hourly_factors=[news_intensity] * horizon_interval_count,
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
        seed=seed,
        start_time_utc=start_time_utc,
        horizon_interval_count=horizon_interval_count,
        kernel_parameters=HawkesKernelParameters.from_mapping(kernel_parameters),
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
        initial_excitation_state=HawkesExcitationState.from_mapping({
            "y_plus_plus": 0.0,
            "y_plus_minus": 0.0,
            "y_minus_plus": 0.0,
            "y_minus_minus": 0.0,
        }),
        hourly_factor_contract=hourly_factor_contract,
        mean_reversion_fitted_parameters=mean_reversion_fitted_parameters,
        mean_reversion_runtime_interval_anchors=mean_reversion_runtime_interval_anchors,
        initial_mean_reversion_boundary_state=initial_mean_reversion_boundary_state,
    )

    return run_seeded_tick_event_simulation(request)


def _collect_interval_event_counts(
    *,
    event_tape: list[Any],
    horizon_interval_count: int,
) -> list[int]:
    interval_event_counts = [0] * horizon_interval_count
    start_time_utc_seconds = int(START_TIME_UTC.timestamp())
    start_time_utc_nanos = start_time_utc_seconds * 1_000_000_000

    for event_record in event_tape:
        timestamp_utc_nanos = event_record.timestamp_utc_nanos
        interval_position = (timestamp_utc_nanos - start_time_utc_nanos) // ONE_HOUR_NANOS

        if 0 <= interval_position < horizon_interval_count:
            interval_event_counts[int(interval_position)] += 1

    return interval_event_counts


def _calculate_fano_factor(interval_event_counts: list[int]) -> float:
    if not interval_event_counts:
        return 0.0

    mean_event_count = sum(interval_event_counts) / len(interval_event_counts)
    if mean_event_count <= 0.0:
        return 0.0

    variance = (
        sum((event_count - mean_event_count) ** 2 for event_count in interval_event_counts)
        / len(interval_event_counts)
    )
    return variance / mean_event_count


def _calculate_lag_one_autocorrelation(interval_event_counts: list[int]) -> float:
    if len(interval_event_counts) < 2:
        return 0.0

    left_interval_counts = [float(event_count) for event_count in interval_event_counts[:-1]]
    right_interval_counts = [float(event_count) for event_count in interval_event_counts[1:]]

    left_mean = _mean(left_interval_counts)
    right_mean = _mean(right_interval_counts)

    numerator = sum(
        (left_count - left_mean) * (right_count - right_mean)
        for left_count, right_count in zip(left_interval_counts, right_interval_counts)
    )

    left_variance = sum((left_count - left_mean) ** 2 for left_count in left_interval_counts)
    right_variance = sum((right_count - right_mean) ** 2 for right_count in right_interval_counts)

    denominator = (left_variance * right_variance) ** 0.5

    if denominator == 0.0:
        return 0.0

    return numerator / denominator


def _has_non_grid_arrival(event_tape: list[Any]) -> bool:
    start_time_utc_seconds = int(START_TIME_UTC.timestamp())
    start_time_utc_nanos = start_time_utc_seconds * 1_000_000_000

    for event_record in event_tape:
        timestamp_utc_nanos = event_record.timestamp_utc_nanos
        if (timestamp_utc_nanos - start_time_utc_nanos) % ONE_HOUR_NANOS != 0:
            return True

    return False


def _mean(values: list[float]) -> float:
    if not values:
        return 0.0

    return sum(values) / len(values)


def _symmetric_kernel_parameters() -> dict[str, Any]:
    return {
        "lambda_bar": 0.9,
        "beta": 1.6,
        "alpha": {
            "plus_plus": 0.35,
            "plus_minus": 0.20,
            "minus_plus": 0.20,
            "minus_minus": 0.35,
        },
        "rho": {
            "plus_plus": 0.02,
            "plus_minus": 0.01,
            "minus_plus": 0.01,
            "minus_minus": 0.02,
        },
    }


def _clustered_kernel_parameters() -> dict[str, Any]:
    return {
        "lambda_bar": 0.8,
        "beta": 1.2,
        "alpha": {
            "plus_plus": 0.75,
            "plus_minus": 0.35,
            "minus_plus": 0.32,
            "minus_minus": 0.70,
        },
        "rho": {
            "plus_plus": 0.12,
            "plus_minus": 0.05,
            "minus_plus": 0.04,
            "minus_minus": 0.10,
        },
    }


def _sell_asymmetric_kernel_parameters() -> dict[str, Any]:
    return {
        "lambda_bar": 0.8,
        "beta": 1.4,
        "alpha": {
            "plus_plus": 0.25,
            "plus_minus": 0.10,
            "minus_plus": 0.30,
            "minus_minus": 0.55,
        },
        "rho": {
            "plus_plus": 0.02,
            "plus_minus": 0.01,
            "minus_plus": 0.07,
            "minus_minus": 0.10,
        },
    }
