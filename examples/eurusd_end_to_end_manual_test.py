import argparse
import csv
import json
import math
from collections.abc import Mapping, Sequence
from dataclasses import dataclass
from datetime import UTC, datetime, timedelta
from pathlib import Path
from tempfile import TemporaryDirectory
from time import perf_counter
from typing import Any, Optional
from zoneinfo import ZoneInfo

from janiform.calendar import CalendarRegistry
from janiform.calibration.market_data import (
    CalibrationDuplicateRecordPolicy,
    CalibrationInputFileFormat,
    CalibrationInputFileReference,
    CalibrationMarketDataColumnMapping,
    CalibrationMarketDataNormalizationPolicy,
    CalibrationMarketDataOutputPreferences,
    CalibrationMarketDataWorkflowScope,
    CalibrationSessionConfigurationInput,
    CalibrationTickDataColumnMapping,
    CalibrationTickPriceInputPolicy,
    MarketDataCalibrationRequest,
    prepare_market_data_calibration,
)
from janiform.calibration.staged import (
    CalibrationExecutionMode,
    CalibrationStageExecutionStatus,
    run_staged_calibration,
)
from janiform.hourly_factors import (
    HourlyNewsIntensityMaterializationSettings,
    HourlyNewsIntensityProviderExplicitSource,
    HourlySeasonalityProviderExplicitSource,
    _parse_interval_duration_nanos,
    build_hourly_factor_contract,
    build_runtime_hourly_seasonality_source_from_calibration_payload,
    materialize_hourly_news_intensity_factors,
)
from janiform.pricing import NigSharedPlusShiftConfiguration
from janiform.risk import (
    SufficientStatisticsRiskIntervalFactors,
    SufficientStatisticsRiskModelInputs,
)
from janiform.simulation import (
    CoarseSimulationRunRequest,
    HawkesExcitationState,
    HawkesKernelParameters,
    NewsProcessConfiguration,
    SeededTickSimulationRequest,
    run_coarse_simulation,
    run_seeded_tick_event_simulation,
)

DEFAULT_EXCHANGE_CODE: str = "FX_EURUSD"
DEFAULT_SESSION_TIMEZONE: str = "UTC"
DEFAULT_MAX_INTERVAL_COUNT: int = 168
ONE_HOUR_NANOS: int = 3_600_000_000_000
_COMMON_TIMESTAMP_FORMATS: tuple[str, ...] = (
    "%d.%m.%Y %H:%M:%S.%f%z",
    "%d.%m.%Y %H:%M:%S.%f %z",
    "%d.%m.%Y %H:%M:%S%z",
    "%d.%m.%Y %H:%M:%S %z",
    "%d.%m.%Y %H:%M%z",
    "%d.%m.%Y %H:%M %z",
    "%d.%m.%Y %H:%M:%S.%f",
    "%d.%m.%Y %H:%M:%S",
    "%d.%m.%Y %H:%M",
    "%Y/%m/%d %H:%M:%S.%f%z",
    "%Y/%m/%d %H:%M:%S.%f %z",
    "%Y/%m/%d %H:%M:%S%z",
    "%Y/%m/%d %H:%M:%S %z",
    "%Y/%m/%d %H:%M%z",
    "%Y/%m/%d %H:%M %z",
    "%Y/%m/%d %H:%M:%S.%f",
    "%Y/%m/%d %H:%M:%S",
    "%Y/%m/%d %H:%M",
    "%Y%m%d %H:%M:%S.%f",
    "%Y%m%d %H:%M:%S",
    "%Y%m%d %H%M%S.%f",
    "%Y%m%d %H%M%S",
    "%Y%m%dT%H:%M:%S.%f",
    "%Y%m%dT%H:%M:%S",
    "%Y%m%dT%H%M%S.%f",
    "%Y%m%dT%H%M%S",
    "%Y%m%d%H%M%S.%f",
    "%Y%m%d%H%M%S",
)
_UNIX_EPOCH_UTC = datetime(1970, 1, 1, tzinfo=UTC)

RETURN_METRIC_TOLERANCES: dict[str, float] = {
    "mean_return": 0.005,
    "return_volatility": 0.50,
    "return_skewness": 1.00,
    "return_excess_kurtosis": 5.00,
    "abs_return_lag1_autocorrelation": 0.35,
    "abs_return_tail_ratio_q99_q95": 0.60,
}
RETURN_METRIC_RELATIVE_BASIS: frozenset[str] = frozenset(
    {
        "return_volatility",
        "abs_return_tail_ratio_q99_q95",
    }
)


@dataclass(frozen=True, slots=True)
class TickRecord:
    timestamp: datetime
    close: float
    sign: str


@dataclass(frozen=True, slots=True)
class TickRecordLoadResult:
    tick_records: tuple[TickRecord, ...]
    sign_column_present: bool
    inferred_sign_count: int


def run_example(
    *,
    csv_path: Path,
    artifact_directory: Path,
    exchange_code: str = DEFAULT_EXCHANGE_CODE,
    timestamp_timezone: str = "UTC",
    session_timezone: str = DEFAULT_SESSION_TIMEZONE,
    max_interval_count: int = DEFAULT_MAX_INTERVAL_COUNT,
    simulation_interval_count: Optional[int] = None,
    simulation_interval: str = "1h",
    coarse_seed: int = 17,
    seeded_tick_seed: int = 23,
    session_definition_path: Optional[Path] = None,
    plot_path: Optional[Path] = None,
    plot_data_csv_path: Optional[Path] = None,
    event_count_isolation_runs: bool = False,
) -> dict[str, Any]:
    tick_record_load_result = _load_tick_records(
        csv_path=csv_path,
        timestamp_timezone=timestamp_timezone,
    )
    tick_records = tick_record_load_result.tick_records
    run_id = _build_run_id(csv_path)

    artifact_directory.mkdir(parents=True, exist_ok=True)
    simulation_interval_duration_nanos = _parse_interval_duration_nanos(simulation_interval)
    simulation_interval_delta = (
        simulation_interval_duration_nanos / ONE_HOUR_NANOS
    )

    simulation_start_utc, horizon_interval_count = _resolve_simulation_window(
        timestamps=[record.timestamp for record in tick_records],
        interval_duration_nanos=simulation_interval_duration_nanos,
        requested_interval_count=simulation_interval_count,
        max_interval_count=max_interval_count,
    )
    simulation_end_utc = simulation_start_utc + (
        horizon_interval_count * _timedelta_from_nanos(simulation_interval_duration_nanos)
    )
    normalized_tick_csv_path = (
        artifact_directory / "artifacts" / "runs" / run_id / "normalized_tick_input.csv"
    )
    _write_normalized_tick_csv(
        csv_path=normalized_tick_csv_path,
        tick_records=tick_records,
    )

    observed_hourly_series = _build_observed_hourly_series(
        tick_records=tick_records,
        simulation_start_utc=simulation_start_utc,
        horizon_interval_count=horizon_interval_count,
        interval_duration_nanos=simulation_interval_duration_nanos,
    )

    market_data_request = _build_market_data_request(
        csv_path=normalized_tick_csv_path,
        disk_checkpoint_directory=artifact_directory,
        exchange_code=exchange_code,
        run_id=run_id,
        session_definition_path=session_definition_path,
        session_timezone=session_timezone,
        timestamp_timezone=timestamp_timezone,
    )

    preparation_start = perf_counter()
    prepared = prepare_market_data_calibration(market_data_request)
    preparation_seconds = perf_counter() - preparation_start
    calendar_registry = CalendarRegistry(
        prepared.session_resolution.resolved_session_definition_json,
    )
    resolved_session_timezone = prepared.session_resolution.resolved_timezone_name

    calibration_start = perf_counter()
    calibration_response = run_staged_calibration(
        prepared.execution_input,
        calendar_registry=calendar_registry,
    )
    calibration_seconds = perf_counter() - calibration_start

    if calibration_response.status != CalibrationStageExecutionStatus.SUCCESS:
        raise RuntimeError(
            _format_staged_calibration_failure(calibration_response)
        )

    stage_artifact_paths = _stage_parameter_artifact_paths(
        artifact_directory=artifact_directory,
        run_id=run_id,
    )
    stage_artifacts = _load_stage_parameter_artifacts(
        artifact_paths=stage_artifact_paths,
    )
    calibrated_model = _extract_calibrated_model(
        calendar_registry=calendar_registry,
        stage_artifacts=stage_artifacts,
        exchange_code=exchange_code,
        exchange_timezone=resolved_session_timezone,
        interval_starts=observed_hourly_series["interval_starts"],
        interval_duration_nanos=simulation_interval_duration_nanos,
        simulation_interval_delta=simulation_interval_delta,
    )
    runtime_hourly_factor_contract = _build_runtime_hourly_factor_contract(
        calendar_registry=calendar_registry,
        exchange_code=exchange_code,
        simulation_start_utc=simulation_start_utc,
        simulation_end_utc=simulation_end_utc,
        simulation_interval=simulation_interval,
        calibrated_model=calibrated_model,
    )
    runtime_interval_factors = _build_runtime_interval_factors(
        calibrated_interval_factors=calibrated_model["interval_factors"],
        runtime_hourly_seasonality_factors=runtime_hourly_factor_contract.s_h,
    )

    coarse_request = CoarseSimulationRunRequest.from_runtime_inputs(
        seed=coarse_seed,
        start_time_utc=simulation_start_utc,
        horizon_interval_count=horizon_interval_count,
        interval_delta=simulation_interval_delta,
        risk_model_inputs=calibrated_model["risk_model_inputs"],
        interval_factors=runtime_interval_factors,
        news_process_configuration=calibrated_model["news_process_configuration"],
        calendar_registry=calendar_registry,
        exchange_code=exchange_code,
    )

    coarse_start = perf_counter()
    coarse_path = run_coarse_simulation(coarse_request)
    coarse_generation_seconds = perf_counter() - coarse_start

    hourly_factor_contract = runtime_hourly_factor_contract

    seeded_request = SeededTickSimulationRequest.from_runtime_inputs(
        seed=seeded_tick_seed,
        start_time_utc=simulation_start_utc,
        horizon_interval_count=horizon_interval_count,
        kernel_parameters=calibrated_model["hawkes_kernel_parameters"],
        nig_shared_plus_shift_configuration=calibrated_model["nig_shared_plus_shift_configuration"],
        news_process_configuration=calibrated_model["news_process_configuration"],
        initial_excitation_state=calibrated_model["initial_excitation_state"],
        hourly_factor_contract=hourly_factor_contract,
    )

    seeded_start = perf_counter()
    seeded_path = run_seeded_tick_event_simulation(seeded_request)
    seeded_generation_seconds = perf_counter() - seeded_start
    event_count_isolation_summary = (
        _run_event_count_isolation_experiments(
            calendar_registry=calendar_registry,
            exchange_code=exchange_code,
            simulation_start_utc=simulation_start_utc,
            simulation_end_utc=simulation_end_utc,
            simulation_interval=simulation_interval,
            horizon_interval_count=horizon_interval_count,
            seeded_tick_seed=seeded_tick_seed,
            calibrated_model=calibrated_model,
            default_seeded_path=seeded_path,
        )
        if event_count_isolation_runs
        else None
    )

    coarse_returns = [
        float(record.return_components.total_return)
        for record in coarse_path.records
    ]
    seeded_hourly_returns = _aggregate_seeded_hourly_returns(
        seeded_path=seeded_path,
        horizon_interval_count=horizon_interval_count,
    )

    observed_return_metrics = _compute_return_stylized_facts(
        observed_hourly_series["hourly_returns"],
    )
    coarse_return_metrics = _compute_return_stylized_facts(coarse_returns)
    seeded_return_metrics = _compute_return_stylized_facts(seeded_hourly_returns)

    observed_vs_coarse_return_alignment = _compare_metric_maps(
        reference_label="observed_hourly_returns",
        candidate_label="coarse_interval_returns",
        reference_metrics=observed_return_metrics,
        candidate_metrics=coarse_return_metrics,
        tolerances=RETURN_METRIC_TOLERANCES,
        relative_metric_names=RETURN_METRIC_RELATIVE_BASIS,
    )
    observed_vs_seeded_return_alignment = _compare_metric_maps(
        reference_label="observed_hourly_returns",
        candidate_label="seeded_hourly_returns",
        reference_metrics=observed_return_metrics,
        candidate_metrics=seeded_return_metrics,
        tolerances=RETURN_METRIC_TOLERANCES,
        relative_metric_names=RETURN_METRIC_RELATIVE_BASIS,
    )
    coarse_vs_seeded_return_alignment = _compare_metric_maps(
        reference_label="coarse_interval_returns",
        candidate_label="seeded_hourly_returns",
        reference_metrics=coarse_return_metrics,
        candidate_metrics=seeded_return_metrics,
        tolerances=RETURN_METRIC_TOLERANCES,
        relative_metric_names=RETURN_METRIC_RELATIVE_BASIS,
    )

    first_seeded_event_timestamp = (
        _utc_datetime_from_nanos(seeded_path.event_tape[0].timestamp_utc_nanos).isoformat()
        if seeded_path.event_tape
        else None
    )
    last_seeded_event_timestamp = (
        _utc_datetime_from_nanos(seeded_path.event_tape[-1].timestamp_utc_nanos).isoformat()
        if seeded_path.event_tape
        else None
    )

    if plot_path is not None:
        _write_manual_test_plot(
            plot_path=plot_path,
            tick_records=tick_records,
            observed_hourly_returns=observed_hourly_series["hourly_returns"],
            coarse_hourly_returns=coarse_returns,
            seeded_hourly_returns=seeded_hourly_returns,
            simulation_start_utc=simulation_start_utc,
            interval_duration_nanos=simulation_interval_duration_nanos,
            starting_price=tick_records[-1].close,
        )

    resolved_plot_data_csv_path = (
        plot_data_csv_path
        if plot_data_csv_path is not None
        else artifact_directory / "artifacts" / "runs" / run_id / "hourly_series.csv"
    )
    _write_manual_test_series_csv(
        csv_path=resolved_plot_data_csv_path,
        hourly_boundaries=_interval_boundaries(
            simulation_start_utc=simulation_start_utc,
            horizon_interval_count=horizon_interval_count,
            interval_duration_nanos=simulation_interval_duration_nanos,
        ),
        observed_hourly_returns=observed_hourly_series["hourly_returns"],
        coarse_hourly_returns=coarse_returns,
        seeded_hourly_returns=seeded_hourly_returns,
        starting_price=tick_records[-1].close,
    )

    return {
        "input_summary": {
            "csv_path": str(csv_path),
            "normalized_csv_path": str(normalized_tick_csv_path),
            "tick_row_count": len(tick_records),
            "sign_column_present": tick_record_load_result.sign_column_present,
            "inferred_sign_count": tick_record_load_result.inferred_sign_count,
            "simulation_start_utc": simulation_start_utc.isoformat(),
            "simulation_end_utc": simulation_end_utc.isoformat(),
            "simulation_interval": simulation_interval,
            "simulation_interval_duration_nanos": simulation_interval_duration_nanos,
            "horizon_interval_count": horizon_interval_count,
            "artifact_directory": str(artifact_directory),
            "exchange_code": exchange_code,
            "session_definition_path": (
                str(session_definition_path) if session_definition_path is not None else None
            ),
            "session_resolution_kind": prepared.session_resolution.resolution_kind,
            "session_timezone": resolved_session_timezone,
            "timestamp_timezone": timestamp_timezone,
        },
        "timing_seconds": {
            "market_data_preparation": round(preparation_seconds, 6),
            "staged_calibration": round(calibration_seconds, 6),
            "coarse_generation": round(coarse_generation_seconds, 6),
            "seeded_tick_generation": round(seeded_generation_seconds, 6),
        },
        "plot_summary": {
            "plot_path": str(plot_path) if plot_path is not None else None,
            "plot_data_csv_path": str(resolved_plot_data_csv_path),
        },
        "calibration_summary": {
            "status": calibration_response.status.value,
            "execution_mode": calibration_response.execution_mode.value,
            "selected_stages": [
                stage.value for stage in prepared.execution_input.request.selected_stages
            ],
            "prepared_diagnostics": {
                "tick_record_count": prepared.diagnostics.tick_record_count,
                "session_open_tick_count": prepared.diagnostics.session_open_tick_count,
                "session_closed_tick_count": prepared.diagnostics.session_closed_tick_count,
                "hawkes_candidate_count": prepared.diagnostics.hawkes_candidate_count,
                "intraday_jump_candidate_count": prepared.diagnostics.intraday_jump_candidate_count,
                "close_open_boundary_observation_count": (
                    prepared.diagnostics.close_open_boundary_observation_count
                ),
                "material_close_open_gap_count": prepared.diagnostics.material_close_open_gap_count,
            },
            "artifact_paths": {
                artifact_name: str(artifact_path)
                for artifact_name, artifact_path in stage_artifact_paths.items()
            },
        },
        "coarse_path_summary": {
            "schema_version": coarse_path.schema_version,
            "simulated_interval_count": coarse_path.simulated_interval_count,
            "cumulative_total_return": round(
                coarse_path.diagnostics.cumulative_total_return,
                8,
            ),
            "cumulative_n_plus": coarse_path.diagnostics.cumulative_n_plus,
            "cumulative_n_minus": coarse_path.diagnostics.cumulative_n_minus,
            "max_abs_total_return": round(coarse_path.diagnostics.max_abs_total_return, 8),
        },
        "seeded_tick_summary": {
            "schema_version": seeded_path.schema_version,
            "fine_path_transition_count": len(seeded_path.fine_path),
            "event_count": len(seeded_path.event_tape),
            "first_event_timestamp_utc": first_seeded_event_timestamp,
            "last_event_timestamp_utc": last_seeded_event_timestamp,
            "cumulative_total_return": round(
                sum(record.return_components.total_return for record in seeded_path.fine_path),
                8,
            ),
            "terminal_cumulative_log_return": round(
                seeded_path.fine_path[-1].cumulative_log_return if seeded_path.fine_path else 0.0,
                8,
            ),
        },
        "event_count_diagnostics": {
            "observed_total_tick_count": prepared.diagnostics.tick_record_count,
            "observed_open_session_tick_count": prepared.diagnostics.session_open_tick_count,
            "simulated_seeded_event_count": len(seeded_path.event_tape),
            "simulated_to_observed_total_ratio": round(
                len(seeded_path.event_tape)
                / max(prepared.diagnostics.tick_record_count, 1),
                6,
            ),
            "simulated_to_observed_open_session_ratio": round(
                len(seeded_path.event_tape)
                / max(prepared.diagnostics.session_open_tick_count, 1),
                6,
            ),
            "hourly_factor_summary": {
                "seasonality_materialization_method": (
                    runtime_hourly_factor_contract.seasonality_materialization_method
                ),
                "seasonality_factor_mean": _round_optional_float(
                    _mean(runtime_hourly_factor_contract.s_h)
                ),
                "seasonality_factor_min": _round_optional_float(
                    min(runtime_hourly_factor_contract.s_h)
                    if runtime_hourly_factor_contract.s_h
                    else None
                ),
                "seasonality_factor_max": _round_optional_float(
                    max(runtime_hourly_factor_contract.s_h)
                    if runtime_hourly_factor_contract.s_h
                    else None
                ),
                "calibration_recomposed_factor_mean": _round_optional_float(
                    _mean(calibrated_model["interval_factors"].s_h)
                ),
                "calibration_recomposed_factor_min": _round_optional_float(
                    min(calibrated_model["interval_factors"].s_h)
                    if calibrated_model["interval_factors"].s_h
                    else None
                ),
                "calibration_recomposed_factor_max": _round_optional_float(
                    max(calibrated_model["interval_factors"].s_h)
                    if calibrated_model["interval_factors"].s_h
                    else None
                ),
                "news_intensity_mean": _round_optional_float(
                    _mean(calibrated_model["hourly_total_news_intensity"])
                ),
                "news_intensity_max": _round_optional_float(
                    max(calibrated_model["hourly_total_news_intensity"])
                    if calibrated_model["hourly_total_news_intensity"]
                    else None
                ),
            },
            "seasonality_normalization": calibrated_model["seasonality_normalization"],
            "hawkes_selected_candidate": calibrated_model[
                "hawkes_selected_candidate_summary"
            ],
            "return_components_selected_candidate": calibrated_model[
                "return_components_selected_candidate_summary"
            ],
            "isolation_experiments": event_count_isolation_summary,
        },
        "stylized_facts": {
            "manual_test_tolerance_policy": {
                "return_metrics": RETURN_METRIC_TOLERANCES,
                "note": (
                    "These thresholds are heuristic manual-test gates meant to flag large "
                    "return-shape mismatches between observed, coarse, and seeded paths, not production "
                    "release criteria."
                ),
            },
            "observed_vs_coarse_returns": {
                "observed_metrics": observed_return_metrics,
                "coarse_metrics": coarse_return_metrics,
                "comparison": observed_vs_coarse_return_alignment,
            },
            "observed_vs_seeded_returns": {
                "observed_metrics": observed_return_metrics,
                "seeded_metrics": seeded_return_metrics,
                "comparison": observed_vs_seeded_return_alignment,
            },
            "coarse_vs_seeded_returns": {
                "coarse_metrics": coarse_return_metrics,
                "seeded_metrics": seeded_return_metrics,
                "comparison": coarse_vs_seeded_return_alignment,
            },
        },
        "notes": [
            (
                "The seeded tick simulator emits a priced fine path with a derived event-tape view, "
                "so the manual test aggregates seeded fine-path returns onto the same hourly grid as the coarse path."
            ),
            (
                "If the input CSV omits the `sign` column, this example derives trade signs from price changes "
                "before calibration. Positive price changes map to `buy`, negative price changes map to `sell`, "
                "and flat moves reuse the previous sign with `buy` as the initial fallback."
            ),
            (
                "By default this workflow infers a reusable session definition from the FX workflow context "
                "and the observed tick window. Pass --session-definition-json to override it explicitly."
            ),
            (
                "The summary now compares return stylized facts only. Activity diagnostics are no longer the "
                "primary seeded-vs-coarse alignment check for this workflow."
            ),
            (
                "The default runtime seasonality path materializes hourly factors from the calibrated "
                "normalized event-level seasonality signal and renormalizes the simulation-window mean to 1.0 "
                "before running the coarse and seeded simulators."
            ),
            (
                "Pass --event-count-isolation-runs to compare that default runtime path against the legacy "
                "recomposed hourly seasonality construction and the unrenormalized materialized hourly factors."
            ),
        ],
    }


def _parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description=(
            "Calibrate Janiform on a EUR/USD tick CSV with `time` and `close` columns and an optional "
            "`sign` column, then simulate a coarse path and a seeded priced fine path and compare "
            "return stylized facts."
        )
    )
    parser.add_argument("csv_path", type=Path, help="Path to the EUR/USD tick CSV.")
    parser.add_argument(
        "--artifact-directory",
        type=Path,
        default=None,
        help="Directory for calibration artifacts and generated helper files. Defaults to a temporary directory.",
    )
    parser.add_argument(
        "--session-definition-json",
        type=Path,
        default=None,
        help="Optional custom calendar registry JSON to use instead of automatic session inference.",
    )
    parser.add_argument(
        "--exchange-code",
        default=DEFAULT_EXCHANGE_CODE,
        help="Exchange code used inside the generated or supplied session definition.",
    )
    parser.add_argument(
        "--timestamp-timezone",
        default="UTC",
        help="Timezone to apply when CSV timestamps are naive.",
    )
    parser.add_argument(
        "--session-timezone",
        default=DEFAULT_SESSION_TIMEZONE,
        help="Timezone hint used during automatic session inference when no custom JSON is supplied.",
    )
    parser.add_argument(
        "--max-interval-count",
        type=int,
        default=DEFAULT_MAX_INTERVAL_COUNT,
        help="Maximum simulation intervals to simulate when --simulation-interval-count is not supplied.",
    )
    parser.add_argument(
        "--simulation-interval-count",
        type=int,
        default=None,
        help="Exact simulation interval count to simulate. Defaults to the largest available window capped by --max-interval-count.",
    )
    parser.add_argument(
        "--simulation-interval",
        default="1h",
        help="Fixed-width UTC simulation interval, for example 1h or 15min. Default: 1h.",
    )
    parser.add_argument(
        "--coarse-seed",
        type=int,
        default=17,
        help="Seed for the coarse simulator.",
    )
    parser.add_argument(
        "--seeded-tick-seed",
        type=int,
        default=23,
        help="Seed for the seeded tick simulator.",
    )
    parser.add_argument(
        "--plot-path",
        type=Path,
        default=None,
        help="Optional output path for a PNG showing observed versus simulated paths and return distributions.",
    )
    parser.add_argument(
        "--plot-data-csv-path",
        type=Path,
        default=None,
        help=(
            "Optional output path for a CSV containing simulation interval boundary timestamps plus observed, "
            "coarse, and seeded returns and path values. Defaults to the run artifact directory."
        ),
    )
    parser.add_argument(
        "--event-count-isolation-runs",
        action="store_true",
        help=(
            "Run extra seeded simulations that vary only the runtime seasonality construction "
            "to isolate event-count inflation sources."
        ),
    )
    return parser.parse_args()


def _load_tick_records(
    *,
    csv_path: Path,
    timestamp_timezone: str,
) -> TickRecordLoadResult:
    with csv_path.open("r", encoding="utf-8", newline="") as csv_file:
        reader = csv.DictReader(csv_file)
        if reader.fieldnames is None:
            raise ValueError("CSV must contain a header row")

        required_columns = {"time", "close"}
        missing_columns = required_columns - set(reader.fieldnames)
        if missing_columns:
            missing_columns_text = ", ".join(sorted(missing_columns))
            raise ValueError(f"CSV is missing required columns: {missing_columns_text}")

        sign_column_present = "sign" in reader.fieldnames
        tick_records: list[TickRecord] = []
        previous_close: Optional[float] = None
        previous_sign: str = "buy"
        inferred_sign_count = 0
        for row_index, row in enumerate(reader, start=2):
            raw_timestamp = row.get("time")
            raw_close = row.get("close")
            raw_sign = row.get("sign")
            if raw_timestamp is None or raw_close is None:
                raise ValueError(f"CSV row {row_index} must contain `time` and `close`")

            timestamp = _parse_timestamp(
                raw_timestamp=raw_timestamp,
                timestamp_timezone=timestamp_timezone,
            )
            try:
                close = float(raw_close)
            except ValueError as e:
                raise ValueError(f"CSV row {row_index} has a non-numeric close value") from e

            if not math.isfinite(close) or close <= 0.0:
                raise ValueError(
                    f"CSV row {row_index} has an invalid close value. Prices must be finite and positive."
                )

            normalized_sign = _resolve_tick_sign(
                raw_sign=raw_sign,
                row_index=row_index,
                current_close=close,
                previous_close=previous_close,
                previous_sign=previous_sign,
            )
            if raw_sign is None:
                inferred_sign_count += 1
            elif not raw_sign.strip():
                inferred_sign_count += 1
            elif not sign_column_present:
                inferred_sign_count += 1

            tick_records.append(
                TickRecord(
                    timestamp=timestamp,
                    close=close,
                    sign=normalized_sign,
                )
            )
            previous_close = close
            previous_sign = normalized_sign

    if not tick_records:
        raise ValueError("CSV must contain at least one tick row")

    tick_records.sort(key=lambda record: record.timestamp)
    previous_timestamp: Optional[datetime] = None
    for record in tick_records:
        if previous_timestamp is not None and record.timestamp == previous_timestamp:
            raise ValueError(
                "CSV contains duplicated timestamps after normalization. "
                "This example uses duplicate_record_policy=error to match calibration settings."
            )
        previous_timestamp = record.timestamp

    return TickRecordLoadResult(
        tick_records=tuple(tick_records),
        sign_column_present=sign_column_present,
        inferred_sign_count=inferred_sign_count,
    )


def _resolve_tick_sign(
    *,
    raw_sign: Optional[str],
    row_index: int,
    current_close: float,
    previous_close: Optional[float],
    previous_sign: str,
) -> str:
    if raw_sign is not None and raw_sign.strip():
        normalized_sign = raw_sign.strip().lower()
        if normalized_sign not in {"buy", "sell"}:
            raise ValueError(
                f"CSV row {row_index} has an invalid sign value. Expected `buy` or `sell`."
            )
        return normalized_sign

    if previous_close is None:
        return "buy"
    if current_close > previous_close:
        return "buy"
    if current_close < previous_close:
        return "sell"
    return previous_sign


def _write_normalized_tick_csv(
    *,
    csv_path: Path,
    tick_records: Sequence[TickRecord],
) -> None:
    csv_path.parent.mkdir(parents=True, exist_ok=True)
    with csv_path.open("w", encoding="utf-8", newline="") as csv_file:
        writer = csv.DictWriter(
            csv_file,
            fieldnames=["time", "close", "sign"],
        )
        writer.writeheader()
        for tick_record in tick_records:
            writer.writerow(
                {
                    "time": tick_record.timestamp.isoformat(),
                    "close": tick_record.close,
                    "sign": tick_record.sign,
                }
            )


def _parse_timestamp(
    *,
    raw_timestamp: str,
    timestamp_timezone: str,
) -> datetime:
    parsed_timestamp = _parse_timestamp_value(raw_timestamp.strip())
    if parsed_timestamp.tzinfo is None:
        parsed_timestamp = parsed_timestamp.replace(
            tzinfo=_resolve_time_zone(timestamp_timezone)
        )

    return parsed_timestamp.astimezone(UTC)


def _parse_timestamp_value(raw_value: str) -> datetime:
    if not raw_value:
        raise ValueError("timestamp value cannot be blank")

    parsed_numeric_value = _try_parse_numeric_timestamp_text(raw_value)
    if parsed_numeric_value is not None:
        return parsed_numeric_value

    normalized_value = raw_value.replace(" UTC", "+00:00").replace(" GMT", "+00:00")
    if normalized_value.endswith("Z"):
        normalized_value = normalized_value[:-1] + "+00:00"

    try:
        return datetime.fromisoformat(normalized_value)
    except ValueError:
        pass

    for format_string in _COMMON_TIMESTAMP_FORMATS:
        try:
            return datetime.strptime(normalized_value, format_string)
        except ValueError:
            continue

    raise ValueError(f"unsupported timestamp format: {raw_value}")


def _try_parse_numeric_timestamp_text(raw_value: str) -> Optional[datetime]:
    unsigned_value = raw_value.lstrip("+-")
    if unsigned_value.isdigit():
        digit_count = len(unsigned_value)
        if digit_count not in {9, 10, 12, 13, 15, 16, 18, 19}:
            return None

    try:
        if any(character in raw_value for character in (".", "e", "E")):
            parsed_numeric_value = float(raw_value)
        else:
            parsed_numeric_value = int(raw_value)
    except ValueError:
        return None

    return _parse_unix_epoch_value(parsed_numeric_value)


def _parse_unix_epoch_value(raw_value: int | float) -> datetime:
    if not math.isfinite(raw_value):
        raise ValueError("timestamp value must be finite")

    absolute_value = abs(raw_value)
    if absolute_value >= 100_000_000_000_000_000:
        return _from_epoch_fraction(raw_value, 1_000_000_000)
    if absolute_value >= 100_000_000_000_000:
        return _from_epoch_fraction(raw_value, 1_000_000)
    if absolute_value >= 100_000_000_000:
        return _from_epoch_fraction(raw_value, 1_000)

    return _from_epoch_fraction(raw_value, 1)


def _from_epoch_fraction(raw_value: int | float, divisor: int) -> datetime:
    if isinstance(raw_value, int):
        whole_seconds, fractional_units = divmod(raw_value, divisor)
        microseconds = (fractional_units * 1_000_000) // divisor
        return datetime.fromtimestamp(whole_seconds, tz=UTC) + timedelta(
            microseconds=microseconds
        )

    return datetime.fromtimestamp(raw_value / divisor, tz=UTC)


def _resolve_time_zone(timezone_name: str) -> datetime.tzinfo:
    normalized_timezone = timezone_name.strip()
    if normalized_timezone.upper() in {"UTC", "Z", "GMT", "UT"}:
        return UTC

    return ZoneInfo(normalized_timezone)


def _resolve_simulation_window(
    *,
    timestamps: Sequence[datetime],
    interval_duration_nanos: int,
    requested_interval_count: Optional[int],
    max_interval_count: int,
) -> tuple[datetime, int]:
    first_timestamp = timestamps[0]
    last_timestamp = timestamps[-1]
    aligned_start = _ceil_to_interval(first_timestamp, interval_duration_nanos)
    aligned_end = _floor_to_interval(last_timestamp, interval_duration_nanos)
    interval_timedelta = _timedelta_from_nanos(interval_duration_nanos)
    available_interval_count = int((aligned_end - aligned_start) / interval_timedelta)
    if available_interval_count <= 0:
        raise ValueError(
            "The input data does not span one complete aligned simulation interval. "
            "At least one full interval is required for this manual test."
        )

    if requested_interval_count is not None:
        if requested_interval_count <= 0:
            raise ValueError("--simulation-interval-count must be strictly positive")
        if requested_interval_count > available_interval_count:
            raise ValueError(
                "--simulation-interval-count exceeds the number of whole aligned intervals "
                f"available in the sample ({available_interval_count})"
            )
        return aligned_start, requested_interval_count

    interval_count = min(max_interval_count, available_interval_count)
    if interval_count <= 0:
        raise ValueError("--max-interval-count must be strictly positive")

    return aligned_start, interval_count


def _build_market_data_request(
    *,
    csv_path: Path,
    disk_checkpoint_directory: Path,
    exchange_code: str,
    run_id: str,
    session_definition_path: Optional[Path],
    session_timezone: str,
    timestamp_timezone: str,
) -> MarketDataCalibrationRequest:
    session_configuration_fields: dict[str, Any] = {
        "exchange_code": exchange_code,
        "session_timezone": session_timezone,
    }
    if session_definition_path is not None:
        session_configuration_fields["session_definition_source"] = CalibrationInputFileReference(
            path=str(session_definition_path),
            file_format=CalibrationInputFileFormat.JSON,
            provenance_label="session_definition",
        )

    return MarketDataCalibrationRequest(
        schema_version="calibration.market_data.request.v1",
        run_id=run_id,
        tick_data_source=CalibrationInputFileReference(
            path=str(csv_path),
            file_format=CalibrationInputFileFormat.CSV,
            provenance_label="eurusd_tick_csv",
        ),
        session_configuration=CalibrationSessionConfigurationInput(**session_configuration_fields),
        column_mapping=CalibrationMarketDataColumnMapping(
            tick_data=CalibrationTickDataColumnMapping(
                timestamp_column="time",
                close_price_column="close",
                trade_sign_column="sign",
            ),
        ),
        workflow_scope=CalibrationMarketDataWorkflowScope.FULL_WITHOUT_OPTION_SURFACE,
        normalization_policy=CalibrationMarketDataNormalizationPolicy(
            timestamp_timezone=timestamp_timezone,
            sort_records_by_timestamp=True,
            duplicate_record_policy=CalibrationDuplicateRecordPolicy.ERROR,
            tick_price_input_policy=CalibrationTickPriceInputPolicy.CLOSE_PRICE_COLUMN,
        ),
        output_preferences=CalibrationMarketDataOutputPreferences(
            execution_mode=CalibrationExecutionMode.DISK,
            disk_checkpoint_directory=str(disk_checkpoint_directory),
            emit_derived_staged_execution_input=False,
            emit_input_provenance_report=True,
        ),
    )


def _build_default_fx_session_definition(
    *,
    exchange_code: str,
    session_timezone: str,
) -> dict[str, Any]:
    regular_sessions: list[dict[str, Any]] = []
    for weekday in ("Monday", "Tuesday", "Wednesday", "Thursday", "Friday"):
        regular_sessions.append(
            {
                "weekday": weekday,
                "windows": [
                    {
                        "label": "regular",
                        "open_time": "00:00",
                        "close_time": "23:59",
                    }
                ],
            }
        )

    return {
        "schema_version": "calendar.registry.v1",
        "artifact_date": datetime.now(tz=UTC).date().isoformat(),
        "exchanges": [
            {
                "schema_version": "calendar.v1",
                "exchange_code": exchange_code,
                "exchange_name": "EUR/USD Spot FX 24/5",
                "timezone_name": session_timezone,
                "rule_metadata": {
                    "holiday_calendar_code": "FX_24_5",
                    "early_close_rule_set": None,
                },
                "regular_sessions": regular_sessions,
                "holiday_dates": [],
                "early_close_sessions": [],
            }
        ],
    }


def _load_exchange_timezone_from_session_definition(
    *,
    session_definition_path: Path,
    exchange_code: str,
) -> str:
    session_definition = json.loads(session_definition_path.read_text(encoding="utf-8"))
    exchanges = session_definition.get("exchanges")
    if not isinstance(exchanges, list):
        raise ValueError("session definition must contain an `exchanges` list")

    for exchange in exchanges:
        if isinstance(exchange, dict) and exchange.get("exchange_code") == exchange_code:
            timezone_name = exchange.get("timezone_name")
            if not isinstance(timezone_name, str) or not timezone_name.strip():
                raise ValueError(
                    f"session definition for `{exchange_code}` must contain a non-empty timezone_name"
                )
            return timezone_name

    raise ValueError(f"session definition does not contain exchange_code `{exchange_code}`")


def _build_run_id(csv_path: Path) -> str:
    sanitized_stem = "".join(
        character if character.isalnum() else "-"
        for character in csv_path.stem.lower()
    ).strip("-")
    if not sanitized_stem:
        sanitized_stem = "ticks"

    return f"eurusd-manual-{sanitized_stem}"


def _stage_parameter_artifact_paths(
    *,
    artifact_directory: Path,
    run_id: str,
) -> dict[str, Path]:
    run_directory = artifact_directory / "artifacts" / "runs" / run_id
    return {
        "seasonality": run_directory / "seasonality.stage_parameter.artifact.json",
        "hawkes": run_directory / "hawkes.stage_parameter.artifact.json",
        "return_components": run_directory / "return_components.stage_parameter.artifact.json",
    }


def _load_stage_parameter_artifacts(
    *,
    artifact_paths: Mapping[str, Path],
) -> dict[str, dict[str, Any]]:
    loaded_artifacts: dict[str, dict[str, Any]] = {}
    for artifact_name, artifact_path in artifact_paths.items():
        if not artifact_path.exists():
            raise FileNotFoundError(
                f"expected calibration artifact was not emitted: {artifact_path}"
            )
        loaded_artifacts[artifact_name] = json.loads(artifact_path.read_text(encoding="utf-8"))

    return loaded_artifacts


def _format_staged_calibration_failure(calibration_response: Any) -> str:
    failure_lines: list[str] = [
        "staged calibration failed before all stage artifacts were emitted",
        f"run_id: {calibration_response.run_id}",
        f"status: {calibration_response.status.value}",
    ]

    if calibration_response.failure is not None:
        failure_lines.append(
            f"failure_kind: {calibration_response.failure.failure_kind}"
        )
        if calibration_response.failure.stage is not None:
            failure_lines.append(
                f"failure_stage: {calibration_response.failure.stage.value}"
            )
        if calibration_response.failure.error_message:
            failure_lines.append(
                f"failure_message: {calibration_response.failure.error_message}"
            )

    for stage_report in calibration_response.stage_reports:
        stage_summary = (
            f"stage {stage_report.stage.value}: {stage_report.status.value}"
        )
        if stage_report.failure is not None:
            stage_summary += f" ({stage_report.failure.failure_kind})"
            if stage_report.failure.error_message:
                stage_summary += f": {stage_report.failure.error_message}"
        failure_lines.append(stage_summary)

    return "\n".join(failure_lines)


def _extract_calibrated_model(
    *,
    calendar_registry: CalendarRegistry,
    stage_artifacts: Mapping[str, Mapping[str, Any]],
    exchange_code: str,
    exchange_timezone: str,
    interval_starts: Sequence[datetime],
    interval_duration_nanos: int,
    simulation_interval_delta: float,
) -> dict[str, Any]:
    seasonality_payload = _require_nested_mapping(
        stage_artifacts["seasonality"],
        ("stage_parameters", "payload"),
        "seasonality.stage_parameters.payload",
    )
    hawkes_payload = _require_nested_mapping(
        stage_artifacts["hawkes"],
        ("stage_parameters", "payload"),
        "hawkes.stage_parameters.payload",
    )
    return_components_payload = _require_nested_mapping(
        stage_artifacts["return_components"],
        ("stage_parameters", "payload"),
        "return_components.stage_parameters.payload",
    )

    hawkes_fitted_parameters = _require_nested_mapping(
        hawkes_payload,
        ("fitted_parameters",),
        "hawkes.stage_parameters.payload.fitted_parameters",
    )
    return_components_fitted_parameters = _require_nested_mapping(
        return_components_payload,
        ("fitted_parameters",),
        "return_components.stage_parameters.payload.fitted_parameters",
    )
    news_process_configuration_payload = _require_nested_mapping(
        return_components_fitted_parameters,
        ("news_process_configuration",),
        "return_components.stage_parameters.payload.fitted_parameters.news_process_configuration",
    )
    nig_shared_plus_shift_configuration_payload = _require_nested_mapping(
        return_components_fitted_parameters,
        ("nig_shared_plus_shift_configuration",),
        "return_components.stage_parameters.payload.fitted_parameters.nig_shared_plus_shift_configuration",
    )

    hawkes_kernel_parameters = HawkesKernelParameters.from_mapping(
        _require_nested_mapping(
            hawkes_fitted_parameters,
            ("kernel_parameters",),
            "hawkes.stage_parameters.payload.fitted_parameters.kernel_parameters",
        ),
    )
    news_process_configuration = NewsProcessConfiguration.from_mapping(
        dict(news_process_configuration_payload),
    )
    nig_shared_plus_shift_configuration = NigSharedPlusShiftConfiguration.from_mapping(
        dict(nig_shared_plus_shift_configuration_payload),
        field_path=(
            "return_components.stage_parameters.payload.fitted_parameters."
            "nig_shared_plus_shift_configuration"
        ),
    )
    initial_excitation_state = HawkesExcitationState.from_mapping(
        {
            "y_plus_plus": 0.0,
            "y_plus_minus": 0.0,
            "y_minus_plus": 0.0,
            "y_minus_minus": 0.0,
        }
    )

    hourly_seasonality_factors = _compose_hourly_seasonality_factors(
        seasonality_payload=seasonality_payload,
        exchange_timezone=exchange_timezone,
        interval_starts=interval_starts,
    )
    hourly_news_materialization = materialize_hourly_news_intensity_factors(
        calendar_registry=calendar_registry,
        exchange_code=exchange_code,
        range_start_utc=interval_starts[0],
        range_end_utc=interval_starts[-1] + _timedelta_from_nanos(interval_duration_nanos),
        interval=f"{interval_duration_nanos}ns",
        settings=HourlyNewsIntensityMaterializationSettings(
            known_scheduled_event_timestamps_utc=[
                _utc_datetime_from_nanos(timestamp_utc_nanos)
                for timestamp_utc_nanos in news_process_configuration.arrivals.known.scheduled_event_timestamps_utc_nanos
            ],
            surprise_baseline_intensity=news_process_configuration.arrivals.surprise.baseline_intensity,
        ),
    )

    interval_factors = SufficientStatisticsRiskIntervalFactors(
        interval_delta=simulation_interval_delta,
        s_h=tuple(hourly_seasonality_factors),
        nu_h_known=tuple(hourly_news_materialization.known_component_nu_h),
        nu_h_surprise=tuple(hourly_news_materialization.surprise_component_nu_h),
    )
    risk_model_inputs = SufficientStatisticsRiskModelInputs.from_mapping(
        {
            "equation_specification": {
                "hawkes_kernel_parameters": hawkes_kernel_parameters.to_payload(),
                "gamma_severity_mark_configuration": (
                    news_process_configuration.shared_mark_law.gamma_severity_marks.to_payload()
                ),
            },
            "nig_shared_plus_shift_configuration": nig_shared_plus_shift_configuration.to_payload(),
            "initial_excitation_state": initial_excitation_state.to_payload(),
        }
    )

    return {
        "hawkes_kernel_parameters": hawkes_kernel_parameters,
        "nig_shared_plus_shift_configuration": nig_shared_plus_shift_configuration,
        "initial_excitation_state": initial_excitation_state,
        "news_process_configuration": news_process_configuration,
        "risk_model_inputs": risk_model_inputs,
        "interval_factors": interval_factors,
        "hourly_total_news_intensity": tuple(hourly_news_materialization.nu_h),
        "seasonality_stage_payload": seasonality_payload,
        "seasonality_normalization": _extract_seasonality_normalization_summary(
            seasonality_payload
        ),
        "hawkes_selected_candidate_summary": _extract_hawkes_candidate_summary(
            hawkes_artifact=stage_artifacts["hawkes"],
            hawkes_payload=hawkes_payload,
        ),
        "return_components_selected_candidate_summary": (
            _extract_return_components_candidate_summary(
                return_components_artifact=stage_artifacts["return_components"],
                return_components_payload=return_components_payload,
            )
        ),
    }


def _compose_hourly_seasonality_factors(
    *,
    seasonality_payload: Mapping[str, Any],
    exchange_timezone: str,
    interval_starts: Sequence[datetime],
) -> list[float]:
    timezone = _resolve_time_zone(exchange_timezone)
    intraday_minutes_per_bin = int(seasonality_payload["intraday_minutes_per_bin"])
    intraday_factor_by_bin = [
        float(value)
        for value in _require_sequence(
            seasonality_payload,
            "intraday_factor_by_bin",
            "seasonality.stage_parameters.payload.intraday_factor_by_bin",
        )
    ]
    day_of_week_factor = [
        float(value)
        for value in _require_sequence(
            seasonality_payload,
            "day_of_week_factor",
            "seasonality.stage_parameters.payload.day_of_week_factor",
        )
    ]
    day_of_year_factor = [
        float(value)
        for value in _require_sequence(
            seasonality_payload,
            "day_of_year_factor",
            "seasonality.stage_parameters.payload.day_of_year_factor",
        )
    ]

    hourly_factors: list[float] = []
    for interval_start in interval_starts:
        exchange_local_timestamp = interval_start.astimezone(timezone)
        minute_of_day = exchange_local_timestamp.hour * 60 + exchange_local_timestamp.minute
        intraday_bin_index = minute_of_day // intraday_minutes_per_bin
        weekday_index = exchange_local_timestamp.isoweekday() - 1
        day_of_year_index = exchange_local_timestamp.timetuple().tm_yday - 1
        hourly_factors.append(
            float(
                intraday_factor_by_bin[intraday_bin_index]
                * day_of_week_factor[weekday_index]
                * day_of_year_factor[day_of_year_index]
            )
        )

    return hourly_factors


def _extract_seasonality_normalization_summary(
    seasonality_payload: Mapping[str, Any],
) -> dict[str, Any]:
    normalization_metadata = _require_nested_mapping(
        seasonality_payload,
        ("normalization_metadata",),
        "seasonality.stage_parameters.payload.normalization_metadata",
    )

    return {
        "normalization_policy": normalization_metadata["normalization_policy"],
        "fit_window_event_count": int(normalization_metadata["fit_window_event_count"]),
        "target_mean_factor": float(normalization_metadata["target_mean_factor"]),
        "pre_normalization_mean_factor": float(
            normalization_metadata["pre_normalization_mean_factor"]
        ),
        "post_normalization_mean_factor": float(
            normalization_metadata["post_normalization_mean_factor"]
        ),
        "normalization_constant": float(
            normalization_metadata["normalization_constant"]
        ),
    }


def _run_event_count_isolation_experiments(
    *,
    calendar_registry: CalendarRegistry,
    exchange_code: str,
    simulation_start_utc: datetime,
    simulation_end_utc: datetime,
    simulation_interval: str,
    horizon_interval_count: int,
    seeded_tick_seed: int,
    calibrated_model: Mapping[str, Any],
    default_seeded_path: Any,
) -> dict[str, Any]:
    baseline_seasonality_factors = tuple(calibrated_model["interval_factors"].s_h)

    runtime_hourly_factor_contracts = _build_runtime_hourly_factor_contract_variants(
        calendar_registry=calendar_registry,
        exchange_code=exchange_code,
        simulation_start_utc=simulation_start_utc,
        simulation_end_utc=simulation_end_utc,
        simulation_interval=simulation_interval,
        calibrated_model=calibrated_model,
    )

    materialized_hourly_factor_contract = runtime_hourly_factor_contracts[
        "materialized_hourly_factor_contract"
    ]
    renormalized_hourly_factor_contract = runtime_hourly_factor_contracts[
        "renormalized_hourly_factor_contract"
    ]

    baseline_hourly_factor_contract = _load_simulation_hourly_factor_contract(
        calendar_registry=calendar_registry,
        exchange_code=exchange_code,
        simulation_start_utc=simulation_start_utc,
        simulation_end_utc=simulation_end_utc,
        simulation_interval=simulation_interval,
        calibrated_model=calibrated_model,
        seasonality_source=HourlySeasonalityProviderExplicitSource(
            hourly_factors=baseline_seasonality_factors,
            materialization_method="manual_test.calibrated_hourly_seasonality.v1",
        ),
    )

    renormalized_factors = _mean_renormalize_hourly_factors(
        baseline_seasonality_factors
    )

    return {
        "baseline_recomposed_explicit": _run_event_count_isolation_variant(
            seeded_tick_seed=seeded_tick_seed,
            simulation_start_utc=simulation_start_utc,
            horizon_interval_count=horizon_interval_count,
            calibrated_model=calibrated_model,
            seasonality_factors=baseline_seasonality_factors,
            hourly_factor_contract=baseline_hourly_factor_contract,
        ),
        "materialized_from_normalized_event_signal": _run_event_count_isolation_variant(
            seeded_tick_seed=seeded_tick_seed,
            simulation_start_utc=simulation_start_utc,
            horizon_interval_count=horizon_interval_count,
            calibrated_model=calibrated_model,
            seasonality_factors=materialized_hourly_factor_contract.s_h,
            hourly_factor_contract=materialized_hourly_factor_contract,
        ),
        "materialized_from_normalized_event_signal_mean_renormalized": (
            _event_count_isolation_variant_summary(
                seasonality_factors=renormalized_hourly_factor_contract.s_h,
                seeded_path=default_seeded_path,
            )
        ),
        "recomposed_explicit_mean_renormalized": _run_event_count_isolation_variant(
            seeded_tick_seed=seeded_tick_seed,
            simulation_start_utc=simulation_start_utc,
            horizon_interval_count=horizon_interval_count,
            calibrated_model=calibrated_model,
            seasonality_factors=renormalized_factors,
            hourly_factor_contract=renormalized_hourly_factor_contract,
        ),
    }


def _run_event_count_isolation_variant(
    *,
    seeded_tick_seed: int,
    simulation_start_utc: datetime,
    horizon_interval_count: int,
    calibrated_model: Mapping[str, Any],
    seasonality_factors: Sequence[float],
    hourly_factor_contract: Any,
) -> dict[str, Any]:
    try:
        seeded_path = _run_seeded_simulation_for_hourly_factor_contract(
            seeded_tick_seed=seeded_tick_seed,
            simulation_start_utc=simulation_start_utc,
            horizon_interval_count=horizon_interval_count,
            calibrated_model=calibrated_model,
            hourly_factor_contract=hourly_factor_contract,
        )
    except Exception as error:
        return _event_count_isolation_variant_failure_summary(
            seasonality_factors=seasonality_factors,
            error=error,
        )

    return _event_count_isolation_variant_summary(
        seasonality_factors=seasonality_factors,
        seeded_path=seeded_path,
    )


def _run_seeded_simulation_for_hourly_factor_contract(
    *,
    seeded_tick_seed: int,
    simulation_start_utc: datetime,
    horizon_interval_count: int,
    calibrated_model: Mapping[str, Any],
    hourly_factor_contract: Any,
) -> Any:
    seeded_request = SeededTickSimulationRequest.from_runtime_inputs(
        seed=seeded_tick_seed,
        start_time_utc=simulation_start_utc,
        horizon_interval_count=horizon_interval_count,
        kernel_parameters=calibrated_model["hawkes_kernel_parameters"],
        nig_shared_plus_shift_configuration=calibrated_model[
            "nig_shared_plus_shift_configuration"
        ],
        news_process_configuration=calibrated_model["news_process_configuration"],
        initial_excitation_state=calibrated_model["initial_excitation_state"],
        hourly_factor_contract=hourly_factor_contract,
    )

    return run_seeded_tick_event_simulation(seeded_request)


def _event_count_isolation_variant_summary(
    *,
    seasonality_factors: Sequence[float],
    seeded_path: Any,
) -> dict[str, Any]:
    return {
        "event_count": len(seeded_path.event_tape),
        "fine_path_transition_count": len(seeded_path.fine_path),
        "seasonality_factor_mean": _round_optional_float(_mean(seasonality_factors)),
        "seasonality_factor_min": _round_optional_float(
            min(seasonality_factors) if seasonality_factors else None
        ),
        "seasonality_factor_max": _round_optional_float(
            max(seasonality_factors) if seasonality_factors else None
        ),
    }


def _event_count_isolation_variant_failure_summary(
    *,
    seasonality_factors: Sequence[float],
    error: Exception,
) -> dict[str, Any]:
    return {
        "event_count": None,
        "fine_path_transition_count": None,
        "seasonality_factor_mean": _round_optional_float(_mean(seasonality_factors)),
        "seasonality_factor_min": _round_optional_float(
            min(seasonality_factors) if seasonality_factors else None
        ),
        "seasonality_factor_max": _round_optional_float(
            max(seasonality_factors) if seasonality_factors else None
        ),
        "status": "rejected",
        "error_id": type(error).__name__,
        "error_message": str(error),
    }


def _build_runtime_hourly_factor_contract(
    *,
    calendar_registry: CalendarRegistry,
    exchange_code: str,
    simulation_start_utc: datetime,
    simulation_end_utc: datetime,
    simulation_interval: str,
    calibrated_model: Mapping[str, Any],
) -> Any:
    runtime_hourly_factor_contracts = _build_runtime_hourly_factor_contract_variants(
        calendar_registry=calendar_registry,
        exchange_code=exchange_code,
        simulation_start_utc=simulation_start_utc,
        simulation_end_utc=simulation_end_utc,
        simulation_interval=simulation_interval,
        calibrated_model=calibrated_model,
    )

    return runtime_hourly_factor_contracts["renormalized_hourly_factor_contract"]


def _build_runtime_hourly_factor_contract_variants(
    *,
    calendar_registry: CalendarRegistry,
    exchange_code: str,
    simulation_start_utc: datetime,
    simulation_end_utc: datetime,
    simulation_interval: str,
    calibrated_model: Mapping[str, Any],
) -> dict[str, Any]:
    materialized_seasonality_source = (
        build_runtime_hourly_seasonality_source_from_calibration_payload(
            calendar_registry=calendar_registry,
            range_start_utc=simulation_start_utc,
            range_end_utc=simulation_end_utc,
            interval=simulation_interval,
            seasonality_stage_payload=calibrated_model["seasonality_stage_payload"],
            mean_renormalize=False,
        )
    )
    renormalized_seasonality_source = (
        build_runtime_hourly_seasonality_source_from_calibration_payload(
            calendar_registry=calendar_registry,
            range_start_utc=simulation_start_utc,
            range_end_utc=simulation_end_utc,
            interval=simulation_interval,
            seasonality_stage_payload=calibrated_model["seasonality_stage_payload"],
            mean_renormalize=True,
        )
    )

    materialized_hourly_factor_contract = _load_simulation_hourly_factor_contract(
        calendar_registry=calendar_registry,
        exchange_code=exchange_code,
        simulation_start_utc=simulation_start_utc,
        simulation_end_utc=simulation_end_utc,
        simulation_interval=simulation_interval,
        calibrated_model=calibrated_model,
        seasonality_source=materialized_seasonality_source,
    )
    renormalized_hourly_factor_contract = _load_simulation_hourly_factor_contract(
        calendar_registry=calendar_registry,
        exchange_code=exchange_code,
        simulation_start_utc=simulation_start_utc,
        simulation_end_utc=simulation_end_utc,
        simulation_interval=simulation_interval,
        calibrated_model=calibrated_model,
        seasonality_source=renormalized_seasonality_source,
    )

    return {
        "materialized_hourly_factor_contract": materialized_hourly_factor_contract,
        "renormalized_hourly_factor_contract": renormalized_hourly_factor_contract,
    }


def _load_simulation_hourly_factor_contract(
    *,
    calendar_registry: CalendarRegistry,
    exchange_code: str,
    simulation_start_utc: datetime,
    simulation_end_utc: datetime,
    simulation_interval: str,
    calibrated_model: Mapping[str, Any],
    seasonality_source: HourlySeasonalityProviderExplicitSource,
) -> Any:
    return build_hourly_factor_contract(
        calendar_registry=calendar_registry,
        exchange_code=exchange_code,
        range_start_utc=simulation_start_utc,
        range_end_utc=simulation_end_utc,
        interval=simulation_interval,
        seasonality_hourly_factors=seasonality_source.hourly_factors,
        news_intensity_hourly_factors=calibrated_model["hourly_total_news_intensity"],
        seasonality_materialization_method=seasonality_source.materialization_method,
        news_intensity_materialization_method="manual_test.calibrated_hourly_news_intensity.v1",
    )


def _build_runtime_interval_factors(
    *,
    calibrated_interval_factors: SufficientStatisticsRiskIntervalFactors,
    runtime_hourly_seasonality_factors: Sequence[float],
) -> SufficientStatisticsRiskIntervalFactors:
    return SufficientStatisticsRiskIntervalFactors(
        interval_delta=calibrated_interval_factors.interval_delta,
        s_h=tuple(float(value) for value in runtime_hourly_seasonality_factors),
        nu_h_known=calibrated_interval_factors.nu_h_known,
        nu_h_surprise=calibrated_interval_factors.nu_h_surprise,
    )

def _mean_renormalize_hourly_factors(
    hourly_factors: Sequence[float],
) -> tuple[float, ...]:
    mean_hourly_factor = _mean(hourly_factors)
    if mean_hourly_factor in {None, 0.0}:
        return tuple(float(factor) for factor in hourly_factors)

    return tuple(
        float(factor) / mean_hourly_factor
        for factor in hourly_factors
    )


def _extract_hawkes_candidate_summary(
    *,
    hawkes_artifact: Mapping[str, Any],
    hawkes_payload: Mapping[str, Any],
) -> dict[str, Any]:
    selected_candidate = _find_selected_candidate_diagnostic(
        stage_payload=hawkes_payload,
        diagnostics_field_name="candidate_diagnostics",
        context="hawkes.stage_parameters.payload",
    )
    fitted_parameters = _require_nested_mapping(
        hawkes_payload,
        ("fitted_parameters",),
        "hawkes.stage_parameters.payload.fitted_parameters",
    )
    kernel_parameters = _require_nested_mapping(
        fitted_parameters,
        ("kernel_parameters",),
        "hawkes.stage_parameters.payload.fitted_parameters.kernel_parameters",
    )
    optimizer_state = _extract_optimizer_state_summary(
        artifact=hawkes_artifact,
        context="hawkes.optimizer_state",
    )
    endogenous_branching_ratio = _compute_hawkes_endogenous_branching_ratio(
        kernel_parameters
    )

    if selected_candidate is None:
        return {
            "candidate_id": hawkes_payload["selected_candidate_id"],
            "raw_objective_value": optimizer_state["selected_candidate_raw_objective"],
            "penalized_objective_value": optimizer_state[
                "selected_candidate_penalized_objective"
            ],
            "valid_candidate_count": optimizer_state["valid_candidate_count"],
            "input_candidate_count": optimizer_state["input_candidate_count"],
            "warnings": [],
            "implied_metrics": None,
            "source_regime": None,
            "kernel_parameters": {
                "lambda_bar": float(kernel_parameters["lambda_bar"]),
                "beta": float(kernel_parameters["beta"]),
                "endogenous_branching_ratio": _round_optional_float(
                    endogenous_branching_ratio
                ),
            },
        }

    implied_metrics = _require_nested_mapping(
        selected_candidate,
        ("implied_metrics",),
        "hawkes.stage_parameters.payload.candidate_diagnostics[].implied_metrics",
    )
    source_regime = _require_nested_mapping(
        selected_candidate,
        ("source_regime",),
        "hawkes.stage_parameters.payload.candidate_diagnostics[].source_regime",
    )
    return {
        "candidate_id": selected_candidate["candidate_id"],
        "raw_objective_value": float(selected_candidate["raw_objective_value"]),
        "penalized_objective_value": float(
            selected_candidate["penalized_objective_value"]
        ),
        "constraint_violation_count": int(
            selected_candidate["constraint_violation_count"]
        ),
        "valid_candidate_count": optimizer_state["valid_candidate_count"],
        "input_candidate_count": optimizer_state["input_candidate_count"],
        "warnings": [
            str(value)
            for value in _require_sequence(
                selected_candidate,
                "warnings",
                "hawkes.stage_parameters.payload.candidate_diagnostics[].warnings",
            )
        ],
        "implied_metrics": {
            "model_implied_open_event_rate_per_hour": float(
                implied_metrics["model_implied_open_event_rate_per_hour"]
            ),
            "model_implied_expected_event_count_over_observation_window": float(
                implied_metrics[
                    "model_implied_expected_event_count_over_observation_window"
                ]
            ),
            "event_volume_ratio_vs_total_observed": float(
                implied_metrics["event_volume_ratio_vs_total_observed"]
            ),
            "endogenous_branching_ratio": float(
                implied_metrics["endogenous_branching_ratio"]
            ),
            "model_implied_known_news_activity_share": float(
                implied_metrics["model_implied_known_news_activity_share"]
            ),
        },
        "source_regime": {
            "total_tick_count": int(source_regime["total_tick_count"]),
            "open_session_tick_count": int(source_regime["open_session_tick_count"]),
            "open_session_duration_hours": float(
                source_regime["open_session_duration_hours"]
            ),
            "known_news_open_event_count": int(
                source_regime["known_news_open_event_count"]
            ),
            "open_session_known_news_activity_share": float(
                source_regime["open_session_known_news_activity_share"]
            ),
        },
    }


def _extract_return_components_candidate_summary(
    *,
    return_components_artifact: Mapping[str, Any],
    return_components_payload: Mapping[str, Any],
) -> dict[str, Any]:
    fitted_parameters = _require_nested_mapping(
        return_components_payload,
        ("fitted_parameters",),
        "return_components.stage_parameters.payload.fitted_parameters",
    )
    news_process_configuration = _require_nested_mapping(
        fitted_parameters,
        ("news_process_configuration",),
        "return_components.stage_parameters.payload.fitted_parameters.news_process_configuration",
    )
    arrivals = _require_nested_mapping(
        news_process_configuration,
        ("arrivals",),
        "return_components.stage_parameters.payload.fitted_parameters.news_process_configuration.arrivals",
    )
    known_arrivals = _require_nested_mapping(
        arrivals,
        ("known",),
        "return_components.stage_parameters.payload.fitted_parameters.news_process_configuration.arrivals.known",
    )
    surprise_arrivals = _require_nested_mapping(
        arrivals,
        ("surprise",),
        "return_components.stage_parameters.payload.fitted_parameters.news_process_configuration.arrivals.surprise",
    )
    known_component_mark_scales = _require_nested_mapping(
        news_process_configuration,
        ("known_component_mark_scales",),
        "return_components.stage_parameters.payload.fitted_parameters.news_process_configuration.known_component_mark_scales",
    )
    surprise_component_mark_scales = _require_nested_mapping(
        news_process_configuration,
        ("surprise_component_mark_scales",),
        "return_components.stage_parameters.payload.fitted_parameters.news_process_configuration.surprise_component_mark_scales",
    )
    optimizer_state = _extract_optimizer_state_summary(
        artifact=return_components_artifact,
        context="return_components.optimizer_state",
    )
    selected_candidate = _find_selected_candidate_diagnostic(
        stage_payload=return_components_payload,
        diagnostics_field_name="candidate_diagnostics",
        context="return_components.stage_parameters.payload",
    )

    if selected_candidate is None:
        retained_known_events = _require_sequence(
            known_arrivals,
            "scheduled_event_timestamps_utc_nanos",
            "return_components.stage_parameters.payload.fitted_parameters.news_process_configuration.arrivals.known.scheduled_event_timestamps_utc_nanos",
        )

        return {
            "candidate_id": return_components_payload["selected_candidate_id"],
            "raw_objective_value": optimizer_state["selected_candidate_raw_objective"],
            "penalized_objective_value": optimizer_state[
                "selected_candidate_penalized_objective"
            ],
            "valid_candidate_count": optimizer_state["valid_candidate_count"],
            "input_candidate_count": optimizer_state["input_candidate_count"],
            "constraint_violation_count": None,
            "retained_known_news_event_count": len(retained_known_events),
            "surprise_baseline_intensity_per_hour": float(
                surprise_arrivals["baseline_intensity"]
            ),
            "known_return_jump_scale": float(
                known_component_mark_scales["return_jump_scale"]
            ),
            "surprise_return_jump_scale": float(
                surprise_component_mark_scales["return_jump_scale"]
            ),
            "known_activity_coupling_scale": float(
                known_component_mark_scales["activity_coupling_scale"]
            ),
            "surprise_activity_coupling_scale": float(
                surprise_component_mark_scales["activity_coupling_scale"]
            ),
            "objective_metric_buckets": None,
        }

    candidate_families = _require_nested_mapping(
        selected_candidate,
        ("candidate_families",),
        "return_components.stage_parameters.payload.candidate_diagnostics[].candidate_families",
    )
    surprise_arrival_controls = _require_nested_mapping(
        candidate_families,
        ("surprise_news_arrival_controls",),
        "return_components.stage_parameters.payload.candidate_diagnostics[].candidate_families.surprise_news_arrival_controls",
    )
    known_arrival_controls = _require_nested_mapping(
        candidate_families,
        ("known_news_arrival_controls",),
        "return_components.stage_parameters.payload.candidate_diagnostics[].candidate_families.known_news_arrival_controls",
    )
    return_jump_controls = _require_nested_mapping(
        candidate_families,
        ("return_jump_mark_scale_controls",),
        "return_components.stage_parameters.payload.candidate_diagnostics[].candidate_families.return_jump_mark_scale_controls",
    )
    activity_coupling_controls = _require_nested_mapping(
        candidate_families,
        ("news_to_hawkes_activity_coupling_controls",),
        "return_components.stage_parameters.payload.candidate_diagnostics[].candidate_families.news_to_hawkes_activity_coupling_controls",
    )
    objective_metric_buckets = _require_nested_mapping(
        selected_candidate,
        ("objective_metric_buckets",),
        "return_components.stage_parameters.payload.candidate_diagnostics[].objective_metric_buckets",
    )
    retained_known_events = _require_sequence(
        known_arrival_controls,
        "retained_known_event_timestamps_utc_nanos",
        "return_components.stage_parameters.payload.candidate_diagnostics[].candidate_families.known_news_arrival_controls.retained_known_event_timestamps_utc_nanos",
    )

    return {
        "candidate_id": selected_candidate["candidate_id"],
        "raw_objective_value": float(selected_candidate["raw_objective_value"]),
        "penalized_objective_value": float(
            selected_candidate["penalized_objective_value"]
        ),
        "valid_candidate_count": optimizer_state["valid_candidate_count"],
        "input_candidate_count": optimizer_state["input_candidate_count"],
        "constraint_violation_count": int(
            selected_candidate["constraint_violation_count"]
        ),
        "retained_known_news_event_count": len(retained_known_events),
        "surprise_baseline_intensity_per_hour": float(
            surprise_arrival_controls["baseline_intensity"]
        ),
        "known_return_jump_scale": float(
            return_jump_controls["known_return_jump_scale"]
        ),
        "surprise_return_jump_scale": float(
            return_jump_controls["surprise_return_jump_scale"]
        ),
        "known_activity_coupling_scale": float(
            activity_coupling_controls["known_activity_coupling_scale"]
        ),
        "surprise_activity_coupling_scale": float(
            activity_coupling_controls["surprise_activity_coupling_scale"]
        ),
        "objective_metric_buckets": {
            "open_jump_fit": float(objective_metric_buckets["open_jump_fit"]),
            "closed_gap_fit": float(objective_metric_buckets["closed_gap_fit"]),
            "activity_coupling_fit": float(
                objective_metric_buckets["activity_coupling_fit"]
            ),
        },
    }


def _find_selected_candidate_diagnostic(
    *,
    stage_payload: Mapping[str, Any],
    diagnostics_field_name: str,
    context: str,
) -> Optional[dict[str, Any]]:
    selected_candidate_id = str(stage_payload["selected_candidate_id"])
    candidate_diagnostics = stage_payload.get(diagnostics_field_name)
    if not isinstance(candidate_diagnostics, Sequence) or isinstance(
        candidate_diagnostics, str
    ):
        return None

    for candidate_diagnostic in candidate_diagnostics:
        if isinstance(candidate_diagnostic, Mapping) and (
            candidate_diagnostic.get("candidate_id") == selected_candidate_id
        ):
            return dict(candidate_diagnostic)

    return None


def _extract_optimizer_state_summary(
    *,
    artifact: Mapping[str, Any],
    context: str,
) -> dict[str, Any]:
    optimizer_state = _require_nested_mapping(
        artifact,
        ("optimizer_state",),
        context,
    )

    return {
        "input_candidate_count": int(optimizer_state["input_candidate_count"]),
        "valid_candidate_count": int(optimizer_state["valid_candidate_count"]),
        "selected_candidate_raw_objective": float(
            optimizer_state["selected_candidate_raw_objective"]
        ),
        "selected_candidate_penalized_objective": float(
            optimizer_state["selected_candidate_penalized_objective"]
        ),
    }


def _compute_hawkes_endogenous_branching_ratio(
    kernel_parameters: Mapping[str, Any],
) -> Optional[float]:
    beta = float(kernel_parameters["beta"])
    if beta <= 0.0:
        return None

    alpha = _require_nested_mapping(
        kernel_parameters,
        ("alpha",),
        "hawkes.stage_parameters.payload.fitted_parameters.kernel_parameters.alpha",
    )

    scaled_plus_plus = float(alpha["plus_plus"]) / beta
    scaled_plus_minus = float(alpha["plus_minus"]) / beta
    scaled_minus_plus = float(alpha["minus_plus"]) / beta
    scaled_minus_minus = float(alpha["minus_minus"]) / beta
    trace = scaled_plus_plus + scaled_minus_minus

    discriminant = math.sqrt(
        (scaled_plus_plus - scaled_minus_minus) ** 2
        + 4.0 * scaled_plus_minus * scaled_minus_plus
    )

    return 0.5 * (trace + discriminant)


def _build_observed_hourly_series(
    *,
    tick_records: Sequence[TickRecord],
    simulation_start_utc: datetime,
    horizon_interval_count: int,
    interval_duration_nanos: int,
) -> dict[str, Any]:
    interval_starts = _interval_boundaries(
        simulation_start_utc=simulation_start_utc,
        horizon_interval_count=horizon_interval_count,
        interval_duration_nanos=interval_duration_nanos,
    )[:-1]

    boundary_prices = _build_boundary_prices(
        tick_records=tick_records,
        simulation_start_utc=simulation_start_utc,
        horizon_interval_count=horizon_interval_count,
        interval_duration_nanos=interval_duration_nanos,
    )
    hourly_returns = [
        math.log(boundary_prices[boundary_index + 1] / boundary_prices[boundary_index])
        for boundary_index in range(horizon_interval_count)
    ]

    return {
        "interval_starts": interval_starts,
        "hourly_returns": hourly_returns,
    }


def _build_boundary_prices(
    *,
    tick_records: Sequence[TickRecord],
    simulation_start_utc: datetime,
    horizon_interval_count: int,
    interval_duration_nanos: int,
) -> list[float]:
    boundary_prices: list[float] = []
    tick_index = 0
    latest_close: Optional[float] = None

    for boundary_timestamp in _interval_boundaries(
        simulation_start_utc=simulation_start_utc,
        horizon_interval_count=horizon_interval_count,
        interval_duration_nanos=interval_duration_nanos,
    ):
        while tick_index < len(tick_records) and tick_records[tick_index].timestamp <= boundary_timestamp:
            latest_close = tick_records[tick_index].close
            tick_index += 1

        if latest_close is None:
            raise ValueError(
                "Unable to anchor all hourly boundary prices from the observed sample. "
                "Check that the CSV contains data covering the chosen simulation window."
            )

        boundary_prices.append(latest_close)

    return boundary_prices


def _write_manual_test_plot(
    *,
    plot_path: Path,
    tick_records: Sequence[TickRecord],
    observed_hourly_returns: Sequence[float],
    coarse_hourly_returns: Sequence[float],
    seeded_hourly_returns: Sequence[float],
    simulation_start_utc: datetime,
    interval_duration_nanos: int,
    starting_price: float,
) -> None:
    import matplotlib.dates as mdates
    import matplotlib.pyplot as plt

    plot_path.parent.mkdir(parents=True, exist_ok=True)

    tick_timestamps = [record.timestamp for record in tick_records]
    tick_prices = [record.close for record in tick_records]
    hourly_boundaries = _interval_boundaries(
        simulation_start_utc=simulation_start_utc,
        horizon_interval_count=len(observed_hourly_returns),
        interval_duration_nanos=interval_duration_nanos,
    )
    panel_values, panel_title, panel_ylabel = _price_panel_series_from_returns(
        observed_hourly_returns=observed_hourly_returns,
        coarse_hourly_returns=coarse_hourly_returns,
        seeded_hourly_returns=seeded_hourly_returns,
        starting_price=starting_price,
    )

    figure, axes = plt.subplots(3, 1, figsize=(11, 10), constrained_layout=True)

    axes[0].plot(tick_timestamps, tick_prices, linewidth=1.0, color="#1f4e79")
    axes[0].set_title("Observed EUR/USD Tick Path")
    axes[0].set_ylabel("Midprice")
    axes[0].xaxis.set_major_formatter(mdates.DateFormatter("%H:%M"))

    _plot_hourly_path_series(
        axes=axes[1],
        hourly_boundaries=hourly_boundaries,
        series_values=panel_values["observed"],
        label="Observed",
        linewidth=1.8,
        color="#1b9e77",
    )
    _plot_hourly_path_series(
        axes=axes[1],
        hourly_boundaries=hourly_boundaries,
        series_values=panel_values["coarse"],
        label="Coarse",
        linewidth=1.6,
        color="#d95f02",
    )
    _plot_hourly_path_series(
        axes=axes[1],
        hourly_boundaries=hourly_boundaries,
        series_values=panel_values["seeded"],
        label="Seeded",
        linewidth=1.6,
        color="#7570b3",
    )

    axes[1].set_title(panel_title)
    axes[1].set_ylabel(panel_ylabel)
    axes[1].xaxis.set_major_formatter(mdates.DateFormatter("%H:%M"))
    axes[1].set_xlim(hourly_boundaries[0], hourly_boundaries[-1])
    axes[1].legend(loc="best")

    histogram_range = _histogram_display_range(
        observed_hourly_returns=observed_hourly_returns,
        coarse_hourly_returns=coarse_hourly_returns,
        seeded_hourly_returns=seeded_hourly_returns,
    )
    histogram_bins = _histogram_bin_count(
        observed_hourly_returns=observed_hourly_returns,
        coarse_hourly_returns=coarse_hourly_returns,
        seeded_hourly_returns=seeded_hourly_returns,
        histogram_range=histogram_range,
    )
    axes[2].hist(
        observed_hourly_returns,
        bins=histogram_bins,
        range=histogram_range,
        alpha=0.25,
        color="#1b9e77",
        label="Observed return distribution",
    )
    axes[2].hist(
        coarse_hourly_returns,
        bins=histogram_bins,
        range=histogram_range,
        alpha=0.25,
        color="#d95f02",
        label="Coarse return distribution",
    )
    axes[2].hist(
        seeded_hourly_returns,
        bins=histogram_bins,
        range=histogram_range,
        alpha=0.25,
        color="#7570b3",
        label="Seeded return distribution",
    )
    axes[2].set_title("Hourly Return Distribution")
    axes[2].set_xlabel("Hourly log return")
    axes[2].set_ylabel("Frequency")
    axes[2].set_xlim(*histogram_range)
    axes[2].legend(loc="best")

    figure.savefig(plot_path, dpi=160)
    plt.close(figure)


def _write_manual_test_series_csv(
    *,
    csv_path: Path,
    hourly_boundaries: Sequence[datetime],
    observed_hourly_returns: Sequence[float],
    coarse_hourly_returns: Sequence[float],
    seeded_hourly_returns: Sequence[float],
    starting_price: float,
) -> None:
    observed_prices = _price_path_from_returns(
        hourly_returns=observed_hourly_returns,
        starting_price=starting_price,
    )
    coarse_prices = _price_path_from_returns(
        hourly_returns=coarse_hourly_returns,
        starting_price=starting_price,
    )
    seeded_prices = _price_path_from_returns(
        hourly_returns=seeded_hourly_returns,
        starting_price=starting_price,
    )
    observed_log_prices = _log_price_path_from_returns(
        hourly_returns=observed_hourly_returns,
        starting_price=starting_price,
    )
    coarse_log_prices = _log_price_path_from_returns(
        hourly_returns=coarse_hourly_returns,
        starting_price=starting_price,
    )
    seeded_log_prices = _log_price_path_from_returns(
        hourly_returns=seeded_hourly_returns,
        starting_price=starting_price,
    )

    csv_path.parent.mkdir(parents=True, exist_ok=True)
    with csv_path.open("w", encoding="utf-8", newline="") as csv_file:
        writer = csv.DictWriter(
            csv_file,
            fieldnames=[
                "boundary_index",
                "boundary_timestamp_utc",
                "observed_price",
                "coarse_price",
                "seeded_price",
                "observed_log_price",
                "coarse_log_price",
                "seeded_log_price",
                "observed_interval_return",
                "coarse_interval_return",
                "seeded_interval_return",
            ],
        )
        writer.writeheader()

        for boundary_index, boundary_timestamp in enumerate(hourly_boundaries):
            writer.writerow(
                {
                    "boundary_index": boundary_index,
                    "boundary_timestamp_utc": boundary_timestamp.isoformat(),
                    "observed_price": observed_prices[boundary_index],
                    "coarse_price": coarse_prices[boundary_index],
                    "seeded_price": seeded_prices[boundary_index],
                    "observed_log_price": observed_log_prices[boundary_index],
                    "coarse_log_price": coarse_log_prices[boundary_index],
                    "seeded_log_price": seeded_log_prices[boundary_index],
                    "observed_interval_return": (
                        ""
                        if boundary_index == 0
                        else observed_hourly_returns[boundary_index - 1]
                    ),
                    "coarse_interval_return": (
                        ""
                        if boundary_index == 0
                        else coarse_hourly_returns[boundary_index - 1]
                    ),
                    "seeded_interval_return": (
                        ""
                        if boundary_index == 0
                        else seeded_hourly_returns[boundary_index - 1]
                    ),
                }
            )


def _plot_hourly_path_series(
    *,
    axes: Any,
    hourly_boundaries: Sequence[datetime],
    series_values: Sequence[float],
    label: str,
    linewidth: float,
    color: str,
) -> None:
    marker_style = "o" if len(hourly_boundaries) <= 24 else None
    axes.plot(
        hourly_boundaries,
        series_values,
        label=label,
        linewidth=linewidth,
        marker=marker_style,
        markersize=4.0 if marker_style is not None else None,
        color=color,
    )


def _histogram_display_range(
    *,
    observed_hourly_returns: Sequence[float],
    coarse_hourly_returns: Sequence[float],
    seeded_hourly_returns: Sequence[float],
) -> tuple[float, float]:
    combined_returns = _combined_finite_returns(
        observed_hourly_returns,
        coarse_hourly_returns,
        seeded_hourly_returns,
    )
    if not combined_returns:
        return (-1.0, 1.0)

    data_min = min(combined_returns)
    data_max = max(combined_returns)
    if math.isclose(data_min, data_max, rel_tol=0.0, abs_tol=1e-12):
        padding = max(abs(data_min) * 0.25, 1e-4)
        return (data_min - padding, data_max + padding)

    if len(combined_returns) < 8:
        span = data_max - data_min
        padding = max(span * 0.15, 1e-4)
        return (data_min - padding, data_max + padding)

    lower_quartile = _quantile(combined_returns, 0.25)
    upper_quartile = _quantile(combined_returns, 0.75)
    lower_tail = _quantile(combined_returns, 0.05)
    upper_tail = _quantile(combined_returns, 0.95)
    if (
        lower_quartile is None
        or upper_quartile is None
        or lower_tail is None
        or upper_tail is None
    ):
        span = data_max - data_min
        padding = max(span * 0.15, 1e-4)
        return (data_min - padding, data_max + padding)

    interquartile_range = upper_quartile - lower_quartile
    robust_min = max(data_min, min(lower_tail, lower_quartile - 1.5 * interquartile_range))
    robust_max = min(data_max, max(upper_tail, upper_quartile + 1.5 * interquartile_range))
    if robust_min >= robust_max:
        robust_min = data_min
        robust_max = data_max

    span = robust_max - robust_min
    padding = max(span * 0.15, 1e-4)
    return (robust_min - padding, robust_max + padding)


def _histogram_bin_count(
    *,
    observed_hourly_returns: Sequence[float],
    coarse_hourly_returns: Sequence[float],
    seeded_hourly_returns: Sequence[float],
    histogram_range: tuple[float, float],
) -> int:
    combined_returns = _combined_finite_returns(
        observed_hourly_returns,
        coarse_hourly_returns,
        seeded_hourly_returns,
    )
    if len(combined_returns) <= 3:
        return 5

    histogram_span = histogram_range[1] - histogram_range[0]
    if histogram_span <= 0.0:
        return 5

    sturges_bins = math.ceil(math.log2(len(combined_returns)) + 1.0)
    square_root_bins = math.ceil(math.sqrt(len(combined_returns)))
    lower_quartile = _quantile(combined_returns, 0.25)
    upper_quartile = _quantile(combined_returns, 0.75)
    if (
        lower_quartile is None
        or upper_quartile is None
        or upper_quartile <= lower_quartile
    ):
        return max(5, min(60, max(sturges_bins, square_root_bins)))

    interquartile_range = upper_quartile - lower_quartile
    freedman_diaconis_width = 2.0 * interquartile_range / (len(combined_returns) ** (1.0 / 3.0))
    if freedman_diaconis_width <= 0.0:
        return max(5, min(60, max(sturges_bins, square_root_bins)))

    freedman_diaconis_bins = math.ceil(histogram_span / freedman_diaconis_width)
    return max(5, min(60, max(sturges_bins, min(freedman_diaconis_bins, 2 * square_root_bins))))


def _combined_finite_returns(*return_series: Sequence[float]) -> list[float]:
    return [
        float(value)
        for series in return_series
        for value in series
        if math.isfinite(value)
    ]


def _price_path_from_returns(
    *,
    hourly_returns: Sequence[float],
    starting_price: float,
) -> list[float]:
    price_values = [starting_price]
    current_price = starting_price
    for hourly_return in hourly_returns:
        current_price *= math.exp(hourly_return)
        price_values.append(current_price)

    return price_values


def _log_price_path_from_returns(
    *,
    hourly_returns: Sequence[float],
    starting_price: float,
) -> list[float]:
    log_price_values = [math.log(starting_price)]
    current_log_price = math.log(starting_price)
    for hourly_return in hourly_returns:
        current_log_price += hourly_return
        log_price_values.append(current_log_price)

    return log_price_values


def _price_panel_series_from_returns(
    *,
    observed_hourly_returns: Sequence[float],
    coarse_hourly_returns: Sequence[float],
    seeded_hourly_returns: Sequence[float],
    starting_price: float,
) -> tuple[dict[str, list[float]], str, str]:
    all_return_series = (
        observed_hourly_returns,
        coarse_hourly_returns,
        seeded_hourly_returns,
    )
    cumulative_log_return_extreme = max(
        (
            max(abs(value) for value in _cumulative_path(hourly_returns))
            if hourly_returns
            else 0.0
        )
        for hourly_returns in all_return_series
    )

    if cumulative_log_return_extreme <= 12.0:
        return (
            {
                "observed": _price_path_from_returns(
                    hourly_returns=observed_hourly_returns,
                    starting_price=starting_price,
                ),
                "coarse": _price_path_from_returns(
                    hourly_returns=coarse_hourly_returns,
                    starting_price=starting_price,
                ),
                "seeded": _price_path_from_returns(
                    hourly_returns=seeded_hourly_returns,
                    starting_price=starting_price,
                ),
            },
            "Hourly Price Paths",
            "Price",
        )

    return (
        {
            "observed": _log_price_path_from_returns(
                hourly_returns=observed_hourly_returns,
                starting_price=starting_price,
            ),
            "coarse": _log_price_path_from_returns(
                hourly_returns=coarse_hourly_returns,
                starting_price=starting_price,
            ),
            "seeded": _log_price_path_from_returns(
                hourly_returns=seeded_hourly_returns,
                starting_price=starting_price,
            ),
        },
        "Hourly Log-Price Paths (price-path fallback due to extreme simulated returns)",
        "Log price",
    )


def _cumulative_path(hourly_returns: Sequence[float]) -> list[float]:
    cumulative_values = [0.0]
    cumulative_total = 0.0
    for hourly_return in hourly_returns:
        cumulative_total += hourly_return
        cumulative_values.append(cumulative_total)

    return cumulative_values


def _aggregate_seeded_hourly_returns(
    *,
    seeded_path: Any,
    horizon_interval_count: int,
) -> list[float]:
    hourly_returns = [0.0 for _ in range(horizon_interval_count)]
    interval_index = 0

    for fine_path_record in seeded_path.fine_path:
        if interval_index >= horizon_interval_count:
            break

        hourly_returns[interval_index] += float(fine_path_record.return_components.total_return)
        if fine_path_record.transition_kind.value == "interval_boundary_drift":
            interval_index += 1

    if interval_index != horizon_interval_count:
        raise ValueError(
            "seeded fine path did not emit one interval-boundary drift transition per simulated interval"
        )

    return hourly_returns


def _compute_return_stylized_facts(series: Sequence[float]) -> dict[str, Optional[float]]:
    absolute_returns = [abs(value) for value in series]
    return {
        "mean_return": _mean(series),
        "return_volatility": _population_std(series),
        "return_skewness": _sample_skewness(series),
        "return_excess_kurtosis": _sample_excess_kurtosis(series),
        "abs_return_lag1_autocorrelation": _lag_one_autocorrelation(absolute_returns),
        "abs_return_tail_ratio_q99_q95": _quantile_ratio(absolute_returns, 0.99, 0.95),
    }


def _compare_metric_maps(
    *,
    reference_label: str,
    candidate_label: str,
    reference_metrics: Mapping[str, Optional[float]],
    candidate_metrics: Mapping[str, Optional[float]],
    tolerances: Mapping[str, float],
    relative_metric_names: frozenset[str],
) -> dict[str, Any]:
    metric_results: dict[str, Any] = {}
    passed_metric_count = 0
    failed_metric_count = 0
    unavailable_metric_count = 0

    for metric_name, tolerance in tolerances.items():
        reference_value = reference_metrics.get(metric_name)
        candidate_value = candidate_metrics.get(metric_name)
        if reference_value is None or candidate_value is None:
            metric_results[metric_name] = {
                "reference_value": reference_value,
                "candidate_value": candidate_value,
                "delta": None,
                "comparison_value": None,
                "comparison_basis": "relative" if metric_name in relative_metric_names else "absolute",
                "tolerance": tolerance,
                "status": "unavailable",
            }
            unavailable_metric_count += 1
            continue

        delta = abs(candidate_value - reference_value)
        if metric_name in relative_metric_names:
            scale = max(abs(reference_value), 1e-12)
            comparison_value = delta / scale
            comparison_basis = "relative"
        else:
            comparison_value = delta
            comparison_basis = "absolute"

        status = "pass" if comparison_value <= tolerance else "fail"
        if status == "pass":
            passed_metric_count += 1
        else:
            failed_metric_count += 1

        metric_results[metric_name] = {
            "reference_value": _round_optional_float(reference_value),
            "candidate_value": _round_optional_float(candidate_value),
            "delta": _round_optional_float(delta),
            "comparison_value": _round_optional_float(comparison_value),
            "comparison_basis": comparison_basis,
            "tolerance": tolerance,
            "status": status,
        }

    if failed_metric_count > 0:
        status = "fail"
    elif passed_metric_count > 0:
        status = "pass"
    else:
        status = "unavailable"

    return {
        "reference_label": reference_label,
        "candidate_label": candidate_label,
        "status": status,
        "passed_metric_count": passed_metric_count,
        "failed_metric_count": failed_metric_count,
        "unavailable_metric_count": unavailable_metric_count,
        "metrics": metric_results,
    }


def _lag_one_autocorrelation(series: Sequence[float]) -> Optional[float]:
    if len(series) < 2:
        return None

    left_values = list(series[:-1])
    right_values = list(series[1:])
    left_mean = _mean(left_values)
    right_mean = _mean(right_values)
    if left_mean is None or right_mean is None:
        return None

    numerator = sum(
        (left_value - left_mean) * (right_value - right_mean)
        for left_value, right_value in zip(left_values, right_values)
    )
    left_variance = sum((left_value - left_mean) ** 2 for left_value in left_values)
    right_variance = sum((right_value - right_mean) ** 2 for right_value in right_values)
    denominator = math.sqrt(left_variance * right_variance)
    if denominator == 0.0:
        return 0.0

    return _finite_or_none(numerator / denominator)


def _sample_skewness(series: Sequence[float]) -> Optional[float]:
    mean_value = _mean(series)
    standard_deviation = _population_std(series)
    if mean_value is None or standard_deviation is None:
        return None
    if standard_deviation == 0.0:
        return 0.0

    normalized_moment = _mean(
        [((value - mean_value) / standard_deviation) ** 3 for value in series]
    )
    return _finite_or_none(normalized_moment)


def _sample_excess_kurtosis(series: Sequence[float]) -> Optional[float]:
    mean_value = _mean(series)
    standard_deviation = _population_std(series)
    if mean_value is None or standard_deviation is None:
        return None
    if standard_deviation == 0.0:
        return 0.0

    normalized_moment = _mean(
        [((value - mean_value) / standard_deviation) ** 4 for value in series]
    )
    if normalized_moment is None:
        return None

    return _finite_or_none(normalized_moment - 3.0)


def _quantile_ratio(
    series: Sequence[float],
    upper_quantile: float,
    lower_quantile: float,
) -> Optional[float]:
    if not series:
        return None

    lower_value = _quantile(series, lower_quantile)
    if lower_value is None or lower_value <= 0.0:
        return None

    upper_value = _quantile(series, upper_quantile)
    if upper_value is None:
        return None

    return _finite_or_none(upper_value / lower_value)


def _quantile(series: Sequence[float], quantile: float) -> Optional[float]:
    if not series:
        return None

    sorted_values = sorted(float(value) for value in series)
    if len(sorted_values) == 1:
        return sorted_values[0]

    position = (len(sorted_values) - 1) * quantile
    lower_index = int(math.floor(position))
    upper_index = int(math.ceil(position))
    lower_value = sorted_values[lower_index]
    upper_value = sorted_values[upper_index]
    if lower_index == upper_index:
        return lower_value

    weight = position - lower_index
    return lower_value + (upper_value - lower_value) * weight


def _mean(series: Sequence[float]) -> Optional[float]:
    if not series:
        return None

    return _finite_or_none(sum(series) / len(series))


def _population_variance(series: Sequence[float]) -> Optional[float]:
    mean_value = _mean(series)
    if mean_value is None:
        return None

    return _finite_or_none(
        sum((value - mean_value) ** 2 for value in series) / len(series)
    )


def _population_std(series: Sequence[float]) -> Optional[float]:
    variance_value = _population_variance(series)
    if variance_value is None:
        return None

    return _finite_or_none(math.sqrt(variance_value))


def _finite_or_none(value: Optional[float]) -> Optional[float]:
    if value is None:
        return None

    numeric_value = float(value)
    if not math.isfinite(numeric_value):
        return None

    return numeric_value


def _round_optional_float(value: Optional[float]) -> Optional[float]:
    if value is None:
        return None

    return round(value, 8)


def _utc_datetime_from_nanos(timestamp_utc_nanos: int) -> datetime:
    seconds, nanos_remainder = divmod(timestamp_utc_nanos, 1_000_000_000)
    microseconds = nanos_remainder // 1_000

    return datetime.fromtimestamp(seconds, tz=UTC).replace(microsecond=microseconds)


def _timedelta_from_nanos(duration_nanos: int) -> timedelta:
    seconds, nanos_remainder = divmod(duration_nanos, 1_000_000_000)
    microseconds, residual_nanos = divmod(nanos_remainder, 1_000)
    if residual_nanos != 0:
        raise ValueError(
            "This manual test requires simulation intervals that are representable with Python datetimes "
            "(whole-microsecond resolution)."
        )

    return timedelta(seconds=seconds, microseconds=microseconds)


def _interval_boundaries(
    *,
    simulation_start_utc: datetime,
    horizon_interval_count: int,
    interval_duration_nanos: int,
) -> list[datetime]:
    interval_timedelta = _timedelta_from_nanos(interval_duration_nanos)
    return [
        simulation_start_utc + interval_index * interval_timedelta
        for interval_index in range(horizon_interval_count + 1)
    ]


def _ceil_to_interval(timestamp_utc: datetime, interval_duration_nanos: int) -> datetime:
    floored_timestamp = _floor_to_interval(timestamp_utc, interval_duration_nanos)
    if timestamp_utc == floored_timestamp:
        return floored_timestamp

    return floored_timestamp + _timedelta_from_nanos(interval_duration_nanos)


def _floor_to_interval(timestamp_utc: datetime, interval_duration_nanos: int) -> datetime:
    normalized_timestamp = timestamp_utc.astimezone(UTC)
    delta = normalized_timestamp - _UNIX_EPOCH_UTC
    timestamp_nanos = (
        delta.days * 86_400_000_000_000
        + delta.seconds * 1_000_000_000
        + delta.microseconds * 1_000
    )
    floored_timestamp_nanos = (
        timestamp_nanos // interval_duration_nanos
    ) * interval_duration_nanos
    return _utc_datetime_from_nanos(floored_timestamp_nanos)


def _require_nested_mapping(
    payload: Mapping[str, Any],
    path_parts: Sequence[str],
    context: str,
) -> dict[str, Any]:
    current_value: Any = payload
    traversed_parts: list[str] = []
    for path_part in path_parts:
        traversed_parts.append(path_part)
        if not isinstance(current_value, Mapping) or path_part not in current_value:
            joined_path = ".".join(traversed_parts)
            raise KeyError(f"missing required mapping field `{joined_path}` while loading {context}")

        current_value = current_value[path_part]

    if not isinstance(current_value, Mapping):
        raise TypeError(f"{context} must resolve to an object")

    return dict(current_value)


def _require_sequence(
    payload: Mapping[str, Any],
    field_name: str,
    context: str,
) -> Sequence[Any]:
    field_value = payload.get(field_name)
    if not isinstance(field_value, Sequence) or isinstance(field_value, (str, bytes)):
        raise TypeError(f"{context} must be a sequence")

    return field_value


def main() -> None:
    parsed_arguments = _parse_args()
    if parsed_arguments.artifact_directory is None:
        with TemporaryDirectory(prefix="janiform-eurusd-manual-") as temp_directory:
            summary = run_example(
                csv_path=parsed_arguments.csv_path,
                artifact_directory=Path(temp_directory),
                exchange_code=parsed_arguments.exchange_code,
                timestamp_timezone=parsed_arguments.timestamp_timezone,
                session_timezone=parsed_arguments.session_timezone,
                max_interval_count=parsed_arguments.max_interval_count,
                simulation_interval_count=parsed_arguments.simulation_interval_count,
                simulation_interval=parsed_arguments.simulation_interval,
                coarse_seed=parsed_arguments.coarse_seed,
                seeded_tick_seed=parsed_arguments.seeded_tick_seed,
                session_definition_path=parsed_arguments.session_definition_json,
                plot_path=parsed_arguments.plot_path,
                plot_data_csv_path=parsed_arguments.plot_data_csv_path,
                event_count_isolation_runs=parsed_arguments.event_count_isolation_runs,
            )

            print(json.dumps(summary, indent=2, sort_keys=True))
            return

    summary = run_example(
        csv_path=parsed_arguments.csv_path,
        artifact_directory=parsed_arguments.artifact_directory,
        exchange_code=parsed_arguments.exchange_code,
        timestamp_timezone=parsed_arguments.timestamp_timezone,
        session_timezone=parsed_arguments.session_timezone,
        max_interval_count=parsed_arguments.max_interval_count,
        simulation_interval_count=parsed_arguments.simulation_interval_count,
        simulation_interval=parsed_arguments.simulation_interval,
        coarse_seed=parsed_arguments.coarse_seed,
        seeded_tick_seed=parsed_arguments.seeded_tick_seed,
        session_definition_path=parsed_arguments.session_definition_json,
        plot_path=parsed_arguments.plot_path,
        plot_data_csv_path=parsed_arguments.plot_data_csv_path,
        event_count_isolation_runs=parsed_arguments.event_count_isolation_runs,
    )

    print(json.dumps(summary, indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
