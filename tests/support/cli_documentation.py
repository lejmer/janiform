import json
from datetime import UTC, datetime
from pathlib import Path
from typing import Any

import pyarrow as pa
from pyarrow import parquet

from janiform.calendar import CalendarRegistry
from janiform.calibration.staged import CalibrationExecutionMode
from janiform.hourly_factors import (
    HourlyFactorContract,
    HourlyNewsIntensityMaterialization,
    HourlyNewsIntensityMaterializationSettings,
    HourlyNewsIntensityProviderExplicitSource,
    HourlySeasonalityMaterialization,
    HourlySeasonalityMaterializationSettings,
    HourlySeasonalityProviderExplicitSource,
    materialize_hourly_news_intensity_factors,
    materialize_hourly_seasonality_factors,
    provide_hourly_factor_contract,
)
from janiform.pricing import (
    GammaSeverityMarkConfiguration,
    NigSharedPlusShiftConfiguration,
    VanillaOptionPricingExecutionInput,
)
from janiform.simulation import (
    HawkesExcitationState,
    HawkesKernelLoadings,
    HawkesKernelParameters,
    SeededTickSimulationRequest,
    build_endogenous_seeded_mean_reversion_runtime_inputs,
)
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
from tests.support.calibration import build_valid_execution_input_model
from tests.support.cli_fixtures import (
    build_coarse_simulation_request,
    build_pricing_optional_output_selection,
    build_pricing_request,
    build_stylized_fact_artifact_export_config,
    build_stylized_fact_horizon_comparison_request,
    build_stylized_fact_metric_catalog,
)
from tests.support.hourly_factor_contracts import build_hourly_factor_contract
from tests.support.pricing import build_runtime_interval_schedule
from tests.support.time import REFERENCE_PRICING_TIMESTAMP_UTC_NANOS

CALENDAR_REGISTRY_JSON: str = json.dumps(
    {
        "schema_version": "calendar.registry.v1",
        "artifact_date": "2026-01-31",
        "exchanges": [
            {
                "schema_version": "calendar.v1",
                "exchange_code": "XNYS",
                "exchange_name": "New York Stock Exchange",
                "timezone_name": "America/New_York",
                "rule_metadata": {
                    "holiday_calendar_code": "US_EQUITIES",
                    "early_close_rule_set": "US_EQUITIES_EARLY_CLOSE",
                },
                "regular_sessions": [
                    {
                        "weekday": weekday,
                        "windows": [
                            {
                                "label": "regular",
                                "open_time": "09:30",
                                "close_time": "16:00",
                            }
                        ],
                    }
                    for weekday in ("Monday", "Tuesday", "Wednesday", "Thursday", "Friday")
                ],
                "holiday_dates": ["2026-02-03"],
                "early_close_sessions": [],
            }
        ],
    }
)
SIMULATION_RANGE_START_UTC: datetime = datetime(2026, 2, 2, 14, 0, 0, tzinfo=UTC)
SIMULATION_RANGE_END_UTC: datetime = datetime(2026, 2, 2, 17, 0, 0, tzinfo=UTC)


def write_cli_documentation_inputs(output_directory: Path) -> dict[str, str]:
    output_directory.mkdir(parents=True, exist_ok=True)
    market_data_paths = _write_market_data_fixture_files(output_directory)

    calendar_registry_path = output_directory / "calendar_registry.json"
    calendar_registry_path.write_text(
        json.dumps(json.loads(CALENDAR_REGISTRY_JSON), indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    timestamp_event_records_path = output_directory / "timestamp_event_records.json"
    timestamp_event_records_path.write_text(
        json.dumps(
            {
                "event_records": [
                    {"timestamp": "2026-02-02T14:30:00Z"},
                    {"timestamp": "2026-02-02T16:30:00Z"},
                    {"timestamp": "2026-02-02T16:31:00Z"},
                ]
            },
            indent=2,
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
    )
    hourly_factor_seasonality_materialization_input_path = (
        output_directory / "hourly_factor_seasonality_materialization_input.json"
    )
    hourly_factor_seasonality_materialization_input_path.write_text(
        json.dumps(
            {
                "source_timestamps_utc": [
                    "2026-02-02T14:05:00Z",
                    "2026-02-02T14:20:00Z",
                    "2026-02-02T15:10:00Z",
                    "2026-02-02T16:05:00Z",
                ],
                "source_seasonality_factors": [1.0, 1.2, 0.8, 1.4],
                "interpolation_method": "previous_observation_carry_forward.v1",
                "aggregation_method": "mean_of_observations.v1",
                "sparse_fallback_policy": "global_mean.v1",
                "sparse_hour_minimum_observation_count": 1,
            },
            indent=2,
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
    )
    hourly_factor_news_materialization_input_path = (
        output_directory / "hourly_factor_news_materialization_input.json"
    )
    hourly_factor_news_materialization_input_path.write_text(
        json.dumps(
            {
                "known_scheduled_event_timestamps_utc": [
                    "2026-02-02T14:05:00Z",
                    "2026-02-02T15:10:00Z",
                ],
                "surprise_baseline_intensity": 0.1,
                "surprise_deterministic_time_multiplier_by_interval_index": {
                    "0": 1.0,
                    "1": 2.0,
                    "2": 1.0,
                },
            },
            indent=2,
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
    )
    hourly_factor_explicit_seasonality_source_path = output_directory / "hourly_factor_explicit_seasonality_source.json"
    hourly_factor_explicit_seasonality_source_path.write_text(
        json.dumps(
            {
                "hourly_factors": [1.0, 0.9, 1.1],
                "materialization_method": "shared_precomputed_hourly_s_h.v1",
            },
            indent=2,
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
    )
    hourly_factor_explicit_news_source_path = output_directory / "hourly_factor_explicit_news_source.json"
    hourly_factor_explicit_news_source_path.write_text(
        json.dumps(
            {
                "hourly_factors": [0.2, 0.0, 0.3],
                "materialization_method": "shared_precomputed_hourly_nu_h.v1",
            },
            indent=2,
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
    )

    calendar_registry = _build_calendar_registry()
    seasonality_materialization = materialize_hourly_seasonality_factors(
        calendar_registry=calendar_registry,
        exchange_code="XNYS",
        range_start_utc=SIMULATION_RANGE_START_UTC,
        range_end_utc=SIMULATION_RANGE_END_UTC,
        source_timestamps_utc=[
            datetime(2026, 2, 2, 14, 5, 0, tzinfo=UTC),
            datetime(2026, 2, 2, 14, 20, 0, tzinfo=UTC),
            datetime(2026, 2, 2, 15, 10, 0, tzinfo=UTC),
            datetime(2026, 2, 2, 16, 5, 0, tzinfo=UTC),
        ],
        source_seasonality_factors=[1.0, 1.2, 0.8, 1.4],
        settings=HourlySeasonalityMaterializationSettings(
            interpolation_method="previous_observation_carry_forward.v1",
            aggregation_method="mean_of_observations.v1",
            sparse_fallback_policy="global_mean.v1",
            sparse_hour_minimum_observation_count=1,
        ),
    )
    news_materialization = materialize_hourly_news_intensity_factors(
        calendar_registry=calendar_registry,
        exchange_code="XNYS",
        range_start_utc=SIMULATION_RANGE_START_UTC,
        range_end_utc=SIMULATION_RANGE_END_UTC,
        settings=HourlyNewsIntensityMaterializationSettings(
            known_scheduled_event_timestamps_utc=[
                datetime(2026, 2, 2, 14, 5, 0, tzinfo=UTC),
                datetime(2026, 2, 2, 15, 10, 0, tzinfo=UTC),
            ],
            surprise_baseline_intensity=0.1,
            surprise_deterministic_time_multiplier_by_interval_index={0: 1.0, 1: 2.0, 2: 1.0},
        ),
    )
    hourly_factor_materialized_seasonality_path = output_directory / "hourly_factor_materialized_seasonality.json"
    hourly_factor_materialized_seasonality_path.write_text(
        json.dumps(
            _build_hourly_seasonality_materialization_payload(seasonality_materialization),
            indent=2,
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
    )
    hourly_factor_materialized_news_intensity_path = output_directory / "hourly_factor_materialized_news_intensity.json"
    hourly_factor_materialized_news_intensity_path.write_text(
        json.dumps(
            _build_hourly_news_intensity_materialization_payload(news_materialization),
            indent=2,
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
    )

    hourly_factor_contract = provide_hourly_factor_contract(
        calendar_registry=calendar_registry,
        exchange_code="XNYS",
        range_start_utc=SIMULATION_RANGE_START_UTC,
        range_end_utc=SIMULATION_RANGE_END_UTC,
        seasonality_source=_build_simulation_seasonality_source(),
        news_intensity_source=_build_simulation_news_source(),
    )

    (
        mean_reversion_fitted_parameters,
        mean_reversion_runtime_interval_anchors,
        initial_mean_reversion_boundary_state,
    ) = build_endogenous_seeded_mean_reversion_runtime_inputs(
        hourly_factor_contract=hourly_factor_contract
    )

    simulation_request = SeededTickSimulationRequest.from_runtime_inputs(
        seed=17,
        start_time_utc=SIMULATION_RANGE_START_UTC,
        horizon_interval_count=hourly_factor_contract.interval_count,
        kernel_parameters=_build_hawkes_kernel_parameters(),
        nig_shared_plus_shift_configuration=_build_simulation_nig_shared_plus_shift_configuration(),
        news_process_configuration=_build_simulation_news_process_configuration(),
        initial_excitation_state=_build_initial_excitation_state(),
        hourly_factor_contract=hourly_factor_contract,
        mean_reversion_fitted_parameters=mean_reversion_fitted_parameters,
        mean_reversion_runtime_interval_anchors=mean_reversion_runtime_interval_anchors,
        initial_mean_reversion_boundary_state=initial_mean_reversion_boundary_state,
    )
    simulation_request_path = output_directory / "simulation_request.json"
    simulation_request_path.write_text(
        json.dumps(simulation_request.to_payload(), indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    coarse_simulation_request_path = output_directory / "coarse_simulation_request.json"
    coarse_simulation_request_path.write_text(
        json.dumps(
            build_coarse_simulation_request(calendar_registry).to_payload(),
            indent=2,
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
    )

    pricing_request = build_pricing_request()
    pricing_execution_input = VanillaOptionPricingExecutionInput(
        request=pricing_request,
        runtime_interval_schedule=build_runtime_interval_schedule(
            (pricing_request.market_inputs.single_maturity_interval,),
        ),
        hourly_factor_contract=_build_pricing_hourly_factor_contract(),
        optional_output_selection=build_pricing_optional_output_selection(),
    )
    pricing_execution_input_path = output_directory / "pricing_execution_input.json"
    pricing_execution_input_path.write_text(
        json.dumps(pricing_execution_input.to_payload(), indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )

    calibration_execution_input = build_valid_execution_input_model(
        execution_mode=CalibrationExecutionMode.MEMORY,
    )
    calibration_execution_input_path = output_directory / "calibration_execution_input.json"
    calibration_execution_input_path.write_text(
        json.dumps(calibration_execution_input.to_payload(), indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )

    stylized_fact_metric_catalog_path = output_directory / "stylized_fact_metric_catalog.json"
    stylized_fact_metric_catalog_path.write_text(
        json.dumps(build_stylized_fact_metric_catalog(), indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )

    stylized_fact_comparison_request_path = output_directory / "stylized_fact_comparison_request.json"
    stylized_fact_comparison_request_path.write_text(
        json.dumps(build_stylized_fact_horizon_comparison_request(), indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )

    stylized_fact_export_config_path = output_directory / "stylized_fact_artifact_export.json"
    stylized_fact_export_config_path.write_text(
        json.dumps(
            build_stylized_fact_artifact_export_config(
                export_directory=output_directory / "stylized-facts-export",
            ),
            indent=2,
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
    )

    output_paths: dict[str, str] = {
        "calendar_registry_json": str(calendar_registry_path),
        "calibration_execution_input_json": str(calibration_execution_input_path),
        "coarse_simulation_request_json": str(coarse_simulation_request_path),
        "hourly_factor_explicit_news_source_json": str(hourly_factor_explicit_news_source_path),
        "hourly_factor_explicit_seasonality_source_json": str(hourly_factor_explicit_seasonality_source_path),
        "hourly_factor_materialized_news_intensity_json": str(hourly_factor_materialized_news_intensity_path),
        "hourly_factor_materialized_seasonality_json": str(hourly_factor_materialized_seasonality_path),
        "hourly_factor_news_materialization_input_json": str(hourly_factor_news_materialization_input_path),
        "hourly_factor_seasonality_materialization_input_json": str(
            hourly_factor_seasonality_materialization_input_path
        ),
        "known_news_csv": str(market_data_paths["known_news"]),
        "option_surface_parquet": str(market_data_paths["option_surface"]),
        "pricing_execution_input_json": str(pricing_execution_input_path),
        "session_definition_json": str(market_data_paths["session_definition"]),
        "simulation_request_json": str(simulation_request_path),
        "stylized_fact_artifact_export_json": str(stylized_fact_export_config_path),
        "stylized_fact_comparison_request_json": str(stylized_fact_comparison_request_path),
        "stylized_fact_metric_catalog_json": str(stylized_fact_metric_catalog_path),
        "tick_data_csv": str(market_data_paths["tick_csv"]),
        "tick_data_parquet": str(market_data_paths["tick_parquet"]),
        "timestamp_event_records_json": str(timestamp_event_records_path),
    }

    return output_paths


def _build_calendar_registry() -> CalendarRegistry:
    return CalendarRegistry(CALENDAR_REGISTRY_JSON)


def _build_hawkes_kernel_parameters() -> HawkesKernelParameters:
    return HawkesKernelParameters(
        lambda_bar=0.8,
        beta=1.6,
        alpha=HawkesKernelLoadings(
            plus_plus=0.2,
            plus_minus=0.1,
            minus_plus=0.1,
            minus_minus=0.25,
        ),
        rho=HawkesKernelLoadings(
            plus_plus=0.08,
            plus_minus=0.04,
            minus_plus=0.05,
            minus_minus=0.1,
        ),
    )


def _build_hourly_news_intensity_materialization_payload(
    materialization: HourlyNewsIntensityMaterialization,
) -> dict[str, Any]:
    return {
        "schema_version": materialization.schema_version,
        "exchange_code": materialization.exchange_code,
        "exchange_timezone_name": materialization.exchange_timezone_name,
        "request_start_utc_nanos": materialization.request_start_utc_nanos,
        "request_end_utc_nanos": materialization.request_end_utc_nanos,
        "interval_indices": list(materialization.interval_indices),
        "interval_start_utc_nanos": list(materialization.interval_start_utc_nanos),
        "interval_end_utc_nanos": list(materialization.interval_end_utc_nanos),
        "interval_start_exchange_local": list(materialization.interval_start_exchange_local),
        "interval_end_exchange_local": list(materialization.interval_end_exchange_local),
        "known_component_nu_h": list(materialization.known_component_nu_h),
        "surprise_component_nu_h": list(materialization.surprise_component_nu_h),
        "nu_h": list(materialization.nu_h),
        "interval_count": materialization.interval_count,
        "known_component_non_zero_interval_count": materialization.known_component_non_zero_interval_count,
        "surprise_component_non_zero_interval_count": materialization.surprise_component_non_zero_interval_count,
        "combined_component_non_zero_interval_count": materialization.combined_component_non_zero_interval_count,
        "known_scheduled_arrival_count": materialization.known_scheduled_arrival_count,
        "surprise_multiplier_override_interval_count": materialization.surprise_multiplier_override_interval_count,
        "skipped_local_hour_interval_count": materialization.skipped_local_hour_interval_count,
        "repeated_local_hour_interval_count": materialization.repeated_local_hour_interval_count,
        "materialization_method": materialization.materialization_method,
        "component_aggregation_policy": materialization.component_aggregation_policy,
        "arrival_intensity_policy": materialization.arrival_intensity_policy,
        "known_news_calendar_anchor_policy": materialization.known_news_calendar_anchor_policy,
        "surprise_news_intensity_control_policy": materialization.surprise_news_intensity_control_policy,
        "known_news_calendar_anchor_count": materialization.known_news_calendar_anchor_count,
        "surprise_baseline_intensity": materialization.surprise_baseline_intensity,
        "interval_boundary_policy": materialization.interval_boundary_policy,
        "interval_alignment_policy": materialization.interval_alignment_policy,
        "timezone_transition_policy": materialization.timezone_transition_policy,
        "spring_forward_policy": materialization.spring_forward_policy,
        "fall_back_policy": materialization.fall_back_policy,
    }


def _build_hourly_seasonality_materialization_payload(
    materialization: HourlySeasonalityMaterialization,
) -> dict[str, Any]:
    return {
        "schema_version": materialization.schema_version,
        "exchange_code": materialization.exchange_code,
        "exchange_timezone_name": materialization.exchange_timezone_name,
        "request_start_utc_nanos": materialization.request_start_utc_nanos,
        "request_end_utc_nanos": materialization.request_end_utc_nanos,
        "interval_indices": list(materialization.interval_indices),
        "interval_start_utc_nanos": list(materialization.interval_start_utc_nanos),
        "interval_end_utc_nanos": list(materialization.interval_end_utc_nanos),
        "interval_start_exchange_local": list(materialization.interval_start_exchange_local),
        "interval_end_exchange_local": list(materialization.interval_end_exchange_local),
        "s_h": list(materialization.s_h),
        "source_point_count": materialization.source_point_count,
        "interval_count": materialization.interval_count,
        "sparse_interval_count": materialization.sparse_interval_count,
        "interpolated_interval_count": materialization.interpolated_interval_count,
        "fallback_interval_count": materialization.fallback_interval_count,
        "skipped_local_hour_interval_count": materialization.skipped_local_hour_interval_count,
        "repeated_local_hour_interval_count": materialization.repeated_local_hour_interval_count,
        "interpolation_method": materialization.interpolation_method,
        "aggregation_method": materialization.aggregation_method,
        "sparse_fallback_policy": materialization.sparse_fallback_policy,
        "sparse_hour_minimum_observation_count": materialization.sparse_hour_minimum_observation_count,
        "interval_boundary_policy": materialization.interval_boundary_policy,
        "interval_alignment_policy": materialization.interval_alignment_policy,
        "timezone_transition_policy": materialization.timezone_transition_policy,
        "spring_forward_policy": materialization.spring_forward_policy,
        "fall_back_policy": materialization.fall_back_policy,
    }


def _build_initial_excitation_state() -> HawkesExcitationState:
    return HawkesExcitationState(
        y_plus_plus=0.0,
        y_plus_minus=0.0,
        y_minus_plus=0.0,
        y_minus_minus=0.0,
    )


def _build_pricing_hourly_factor_contract() -> HourlyFactorContract:
    return build_hourly_factor_contract(
        request_start_utc_nanos=REFERENCE_PRICING_TIMESTAMP_UTC_NANOS,
        s_h=(1.0,),
        known_component_nu_h=(0.01,),
        surprise_component_nu_h=(0.02,),
    )


def _build_simulation_news_process_configuration() -> NewsProcessConfiguration:
    return NewsProcessConfiguration(
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
            coupled_gap_marks=CoupledGapMarkConfiguration(
                sigma_g=0.75,
            ),
        ),
        known_component_mark_scales=NewsComponentMarkScaleConfiguration(
            activity_coupling_scale=0.6,
            return_jump_scale=1.2,
        ),
        surprise_component_mark_scales=NewsComponentMarkScaleConfiguration(
            activity_coupling_scale=0.6,
            return_jump_scale=1.2,
        ),
    )


def _build_simulation_news_source() -> HourlyNewsIntensityProviderExplicitSource:
    return HourlyNewsIntensityProviderExplicitSource(
        hourly_factors=[0.2, 0.0, 0.3],
        materialization_method="shared_precomputed_hourly_nu_h.v1",
    )


def _build_simulation_nig_shared_plus_shift_configuration() -> NigSharedPlusShiftConfiguration:
    return NigSharedPlusShiftConfiguration(
        alpha=2.3,
        beta_0=-0.8,
        delta=0.7,
        mu=0.1,
        delta_beta=-0.2,
    )


def _build_simulation_seasonality_source() -> HourlySeasonalityProviderExplicitSource:
    return HourlySeasonalityProviderExplicitSource(
        hourly_factors=[1.0, 0.9, 1.1],
        materialization_method="shared_precomputed_hourly_s_h.v1",
    )


def _write_market_data_fixture_files(working_directory: Path) -> dict[str, Path]:
    data_directory = working_directory / "data"
    config_directory = working_directory / "config"
    data_directory.mkdir(parents=True, exist_ok=True)
    config_directory.mkdir(parents=True, exist_ok=True)

    session_definition_path = config_directory / "sessions.json"
    session_definition_path.write_text(CALENDAR_REGISTRY_JSON, encoding="utf-8")

    tick_csv_path = data_directory / "ticks.csv"
    tick_csv_path.write_text(
        "\n".join(
            [
                "timestamp_utc,trade_price,trade_sign",
                "2026-02-02T14:30:00Z,100.0,buy",
                "2026-02-02T15:00:00Z,101.0,buy",
                "2026-02-02T15:10:00Z,104.0,sell",
                "2026-02-02T20:59:00Z,102.0,sell",
                "2026-02-04T14:30:00Z,90.0,buy",
                "2026-02-04T14:40:00Z,100.0,sell",
            ]
        ),
        encoding="utf-8",
    )

    tick_parquet_path = data_directory / "ticks.parquet"
    parquet.write_table(
        pa.table(
            {
                "timestamp_utc": [
                    datetime(2026, 2, 2, 14, 30, tzinfo=UTC),
                    datetime(2026, 2, 2, 15, 0, tzinfo=UTC),
                    datetime(2026, 2, 2, 15, 10, tzinfo=UTC),
                    datetime(2026, 2, 2, 20, 59, tzinfo=UTC),
                    datetime(2026, 2, 4, 14, 30, tzinfo=UTC),
                    datetime(2026, 2, 4, 14, 40, tzinfo=UTC),
                ],
                "trade_price": [100.0, 101.0, 104.0, 102.0, 90.0, 100.0],
                "trade_sign": ["buy", "buy", "sell", "sell", "buy", "sell"],
            }
        ),
        tick_parquet_path,
    )

    known_news_path = config_directory / "known-news.csv"
    known_news_path.write_text(
        "\n".join(
            [
                "event_timestamp_utc,event_code",
                "2026-02-02T15:05:00Z,open_macro",
                "2026-02-02T22:00:00Z,closed_macro",
                "2026-02-10T15:00:00Z,outside_window",
            ]
        ),
        encoding="utf-8",
    )

    option_surface_path = data_directory / "options.parquet"
    parquet.write_table(
        pa.table(
            {
                "quote_timestamp_utc": [datetime(2026, 2, 2, 20, 0, tzinfo=UTC)],
                "expiry_utc": [datetime(2026, 3, 20, 20, 0, tzinfo=UTC)],
                "strike": [100.0],
                "option_type": ["C"],
                "bid": [4.0],
                "ask": [5.0],
            }
        ),
        option_surface_path,
    )

    return {
        "known_news": known_news_path,
        "option_surface": option_surface_path,
        "session_definition": session_definition_path,
        "tick_csv": tick_csv_path,
        "tick_parquet": tick_parquet_path,
    }
