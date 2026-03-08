import json
from datetime import UTC, datetime
from pathlib import Path
from typing import Optional

import pyarrow as pa
from pyarrow import parquet as pyarrow_parquet

from janiform.calendar import CalendarRegistry
from janiform.calibration import (
    CALIBRATION_STAGED_EXECUTION_INPUT_SCHEMA_VERSION,
    CalibrationCoupledGapMarkConfiguration,
    CalibrationDayOfWeekSeasonalityEstimatorSettings,
    CalibrationDayOfYearSeasonalityEstimatorSettings,
    CalibrationDoubleExponentialBaseShockConfiguration,
    CalibrationExecutionMode,
    CalibrationHawkesStageCandidateInput,
    CalibrationHawkesStructuredAlphaParameterization,
    CalibrationHawkesStructuredRhoParameterization,
    CalibrationHawkesStructuredRhoSelfDominancePolicy,
    CalibrationIntradaySeasonalityEstimatorSettings,
    CalibrationIntradaySmoothingKernel,
    CalibrationObjectiveTermInput,
    CalibrationOptimizerSettings,
    CalibrationReturnComponentsCloseOpenGapDetectionPolicy,
    CalibrationReturnComponentsDetectedIntradayJumpCandidate,
    CalibrationReturnComponentsKnownNewsCalendarInput,
    CalibrationReturnComponentsKnownNewsReturnSample,
    CalibrationReturnComponentsObservedCloseOpenBoundaryObservation,
    CalibrationReturnComponentsObjectiveMetricWeights,
    CalibrationReturnComponentsSignalSourceWindow,
    CalibrationReturnComponentsStageCandidateGenerationInput,
    CalibrationRunDataInputs,
    CalibrationRunSeedSettings,
    CalibrationScheduledEventOverlay,
    CalibrationSeasonalityStageExecutionInput,
    CalibrationSeasonalityStageExecutionSettings,
    CalibrationSharedNewsMarkLawConfiguration,
    CalibrationStage,
    CalibrationStageSeedOverride,
    CalibrationStageSolverSettings,
    CalibrationStagedExecutionRuntimeResources,
    CalibrationTimestampEventRecord,
    CalibrationTimestampEventSessionFilter,
    StagedCalibrationExecutionInput,
    StagedCalibrationRequest,
)
from janiform.hourly_factors import (
    HourlyNewsIntensityProviderExplicitSource,
    HourlySeasonalityProviderExplicitSource,
)
from janiform.pricing import (
    GammaSeverityMarkConfiguration,
    NigSharedPlusShiftConfiguration,
    VanillaOptionPayoffConfiguration,
    VanillaOptionPayoffType,
    VanillaOptionPricingIntervalFactors,
    VanillaOptionPricingMarketInputs,
    VanillaOptionPricingModelParameters,
    VanillaOptionPricingOptionalOutputSelection,
    VanillaOptionPricingRequest,
)
from janiform.simulation import (
    HawkesExcitationState,
    HawkesKernelLoadings,
    HawkesKernelParameters,
)

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


def build_calendar_registry() -> CalendarRegistry:
    return CalendarRegistry(CALENDAR_REGISTRY_JSON)


def build_hawkes_kernel_parameters() -> HawkesKernelParameters:
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


def build_initial_excitation_state() -> HawkesExcitationState:
    return HawkesExcitationState(
        y_plus_plus=0.0,
        y_plus_minus=0.0,
        y_minus_plus=0.0,
        y_minus_minus=0.0,
    )


def build_simulation_seasonality_source() -> HourlySeasonalityProviderExplicitSource:
    return HourlySeasonalityProviderExplicitSource(
        hourly_factors=[1.0, 0.9, 1.1],
        materialization_method="shared_precomputed_hourly_s_h.v1",
    )


def build_simulation_news_source() -> HourlyNewsIntensityProviderExplicitSource:
    return HourlyNewsIntensityProviderExplicitSource(
        hourly_factors=[0.2, 0.0, 0.3],
        materialization_method="shared_precomputed_hourly_nu_h.v1",
    )


def build_pricing_request() -> VanillaOptionPricingRequest:
    return VanillaOptionPricingRequest(
        market_inputs=VanillaOptionPricingMarketInputs(
            spot=101.0,
            strike=100.0,
            maturity_interval=1.0,
            risk_free_rate=0.02,
            dividend_yield=0.01,
        ),
        payoff=VanillaOptionPayoffConfiguration(payoff_type=VanillaOptionPayoffType.CALL),
        boundary_state=HawkesExcitationState(
            y_plus_plus=0.0,
            y_plus_minus=0.0,
            y_minus_plus=0.0,
            y_minus_minus=0.0,
        ),
        model_parameters=VanillaOptionPricingModelParameters(
            hawkes_kernel_parameters=HawkesKernelParameters(
                lambda_bar=0.8,
                beta=1.6,
                alpha=HawkesKernelLoadings(
                    plus_plus=0.2,
                    plus_minus=0.1,
                    minus_plus=0.1,
                    minus_minus=0.25,
                ),
                rho=HawkesKernelLoadings(
                    plus_plus=0.1,
                    plus_minus=0.05,
                    minus_plus=0.05,
                    minus_minus=0.1,
                ),
            ),
            gamma_severity_mark_configuration=GammaSeverityMarkConfiguration(
                shape_k=1.8,
                scale_theta=0.7,
            ),
            nig_shared_plus_shift_configuration=NigSharedPlusShiftConfiguration(
                alpha=2.3,
                beta_0=-0.8,
                delta=0.7,
                mu=0.1,
                delta_beta=-0.2,
            ),
        ),
    )


def build_pricing_interval_factors() -> VanillaOptionPricingIntervalFactors:
    return VanillaOptionPricingIntervalFactors(
        s_h=1.0,
        nu_h_known=0.01,
        nu_h_surprise=0.02,
    )


def build_pricing_optional_output_selection() -> VanillaOptionPricingOptionalOutputSelection:
    return VanillaOptionPricingOptionalOutputSelection(
        include_transform_diagnostics=True,
        include_integration_quality_diagnostics=True,
        include_calibration_hooks=True,
    )


def write_market_data_fixture_files(working_directory: Path) -> dict[str, Path]:
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
                "timestamp_utc,trade_price",
                "2026-02-02T14:30:00Z,100.0",
                "2026-02-02T15:00:00Z,101.0",
                "2026-02-02T15:10:00Z,104.0",
                "2026-02-02T20:59:00Z,102.0",
                "2026-02-04T14:30:00Z,90.0",
                "2026-02-04T14:40:00Z,100.0",
            ]
        ),
        encoding="utf-8",
    )

    tick_parquet_path = data_directory / "ticks.parquet"
    pyarrow_parquet.write_table(
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
    pyarrow_parquet.write_table(
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


def build_calibration_request(
    *,
    execution_mode: Optional[CalibrationExecutionMode] = None,
) -> StagedCalibrationRequest:
    request_fields: dict[str, object] = {
        "run_id": "run-001",
        "data_inputs": CalibrationRunDataInputs(
            exchange_code="XNYS",
            trade_events_data_reference="dataset://synthetic/trades/v1",
            option_surface_data_reference="dataset://synthetic/options/v1",
        ),
        "selected_stages": (
            CalibrationStage.SEASONALITY,
            CalibrationStage.HAWKES,
            CalibrationStage.RETURN_COMPONENTS,
        ),
        "seed_settings": CalibrationRunSeedSettings(
            global_seed=17,
            stage_seed_overrides=(
                CalibrationStageSeedOverride(
                    stage=CalibrationStage.SEASONALITY,
                    seed=101,
                ),
            ),
        ),
        "optimizer_settings_by_stage": (
            build_calibration_solver_settings(CalibrationStage.SEASONALITY),
            build_calibration_solver_settings(CalibrationStage.HAWKES),
            build_calibration_solver_settings(CalibrationStage.RETURN_COMPONENTS),
        ),
    }
    if execution_mode is not None:
        request_fields["execution_mode"] = execution_mode

    return StagedCalibrationRequest(**request_fields)


def build_calibration_execution_input(
    *,
    execution_mode: Optional[CalibrationExecutionMode] = None,
    disk_checkpoint_directory: Optional[str] = None,
) -> StagedCalibrationExecutionInput:
    request: StagedCalibrationRequest = build_calibration_request(execution_mode=execution_mode)
    runtime_resources: Optional[CalibrationStagedExecutionRuntimeResources] = None
    if disk_checkpoint_directory is not None:
        runtime_resources = CalibrationStagedExecutionRuntimeResources(
            disk_checkpoint_directory=disk_checkpoint_directory,
        )

    return StagedCalibrationExecutionInput(
        schema_version=CALIBRATION_STAGED_EXECUTION_INPUT_SCHEMA_VERSION,
        request=request,
        seasonality_stage_input=CalibrationSeasonalityStageExecutionInput(
            timestamp_event_records=(
                CalibrationTimestampEventRecord(
                    source_record_position=0,
                    timestamp_utc_nanos=1770042600000000000,
                ),
                CalibrationTimestampEventRecord(
                    source_record_position=1,
                    timestamp_utc_nanos=1770046200000000000,
                ),
            ),
            scheduled_event_overlays=(
                CalibrationScheduledEventOverlay(
                    overlay_id="macro_release",
                    start_timestamp_utc_nanos=1770042300000000000,
                    end_timestamp_utc_nanos=1770042900000000000,
                    log_adjustment=0.05,
                    precedence=1,
                    enabled=True,
                    source_label="known_macro_calendar",
                ),
            ),
            settings=CalibrationSeasonalityStageExecutionSettings(
                session_filter=CalibrationTimestampEventSessionFilter.OPEN_ONLY,
                sparse_hour_event_threshold=1,
                intraday_settings=CalibrationIntradaySeasonalityEstimatorSettings(
                    minutes_per_bin=1,
                    smoothing_kernel=CalibrationIntradaySmoothingKernel.GAUSSIAN,
                    smoothing_bandwidth_bins=3.0,
                    minimum_bin_events=1,
                    pseudocount=1e-6,
                ),
                day_of_week_settings=CalibrationDayOfWeekSeasonalityEstimatorSettings(
                    minimum_bucket_events=1,
                    pseudocount=1e-6,
                ),
                day_of_year_settings=CalibrationDayOfYearSeasonalityEstimatorSettings(
                    minimum_bucket_events=1,
                    pseudocount=1e-6,
                ),
            ),
        ),
        hawkes_candidate_inputs=(
            CalibrationHawkesStageCandidateInput(
                candidate_id="candidate_stable_low_loss",
                lambda_bar=0.80,
                beta=1.60,
                structured_alpha=CalibrationHawkesStructuredAlphaParameterization(
                    a_self=0.35,
                    a_cross=0.06,
                    delta_self=0.07,
                    delta_cross=0.04,
                ),
                structured_rho=CalibrationHawkesStructuredRhoParameterization(
                    r_self=0.30,
                    r_cross=0.08,
                    delta_self_r=0.04,
                    delta_cross_r=0.05,
                ),
                self_dominance_policy=CalibrationHawkesStructuredRhoSelfDominancePolicy(
                    minimum_self_to_cross_ratio=1.0,
                ),
                stability_numeric_tolerance=0.0,
                objective_terms=(
                    CalibrationObjectiveTermInput(
                        term_id="event_rate_residual",
                        weight=1.0,
                        observed_score=1.6,
                    ),
                    CalibrationObjectiveTermInput(
                        term_id="clustering_autocorrelation",
                        weight=1.0,
                        observed_score=0.7,
                    ),
                ),
            ),
        ),
        return_components_candidate_generation_input=CalibrationReturnComponentsStageCandidateGenerationInput(
            known_news_calendar=CalibrationReturnComponentsKnownNewsCalendarInput(
                source_window=CalibrationReturnComponentsSignalSourceWindow(
                    window_start_utc_nanos=1770037200000000000,
                    window_end_utc_nanos=1770259200000000000,
                ),
                scheduled_event_timestamps_utc_nanos=(
                    1770042600000000000,
                    1770120000000000000,
                    1770211200000000000,
                ),
            ),
            realized_known_news_return_samples=(
                CalibrationReturnComponentsKnownNewsReturnSample(
                    event_timestamp_utc_nanos=1770042600000000000,
                    open_window_jump_return=0.016,
                    closed_window_gap_return=-0.009,
                ),
                CalibrationReturnComponentsKnownNewsReturnSample(
                    event_timestamp_utc_nanos=1770120000000000000,
                    open_window_jump_return=-0.012,
                    closed_window_gap_return=0.004,
                ),
            ),
            detected_intraday_jump_candidates=(
                CalibrationReturnComponentsDetectedIntradayJumpCandidate(
                    jump_timestamp_utc_nanos=1770048300000000000,
                    jump_return=0.018,
                    post_shock_activity_lift=1.35,
                ),
            ),
            observed_close_open_boundary_observations=(
                CalibrationReturnComponentsObservedCloseOpenBoundaryObservation(
                    close_timestamp_utc_nanos=1770066000000000000,
                    close_price=100.0,
                    open_timestamp_utc_nanos=1770215400000000000,
                    open_price=98.70858118310978,
                    post_shock_activity_lift=1.40,
                ),
            ),
            close_open_gap_detection_policy=CalibrationReturnComponentsCloseOpenGapDetectionPolicy(
                material_gap_absolute_return_tolerance=0.005,
            ),
            nig_shared_plus_shift_candidates=(
                NigSharedPlusShiftConfiguration(
                    alpha=2.45,
                    beta_0=-0.70,
                    delta=0.62,
                    mu=0.08,
                    delta_beta=-0.15,
                ),
            ),
            shared_mark_law_template=CalibrationSharedNewsMarkLawConfiguration(
                gamma_severity_marks=GammaSeverityMarkConfiguration(
                    shape_k=1.8,
                    scale_theta=0.7,
                ),
                double_exponential_base_shock=CalibrationDoubleExponentialBaseShockConfiguration(
                    probability_positive=0.45,
                    eta_plus=8.0,
                    eta_minus=10.0,
                ),
                coupled_gap_marks=CalibrationCoupledGapMarkConfiguration(sigma_g=0.22),
            ),
            objective_metric_weights=CalibrationReturnComponentsObjectiveMetricWeights(
                open_jump_fit_weight=1.0,
                closed_gap_fit_weight=1.0,
                activity_coupling_fit_weight=1.0,
            ),
        ),
        runtime_resources=runtime_resources,
    )


def build_calibration_solver_settings(stage: CalibrationStage) -> CalibrationStageSolverSettings:
    return CalibrationStageSolverSettings(
        stage=stage,
        optimizer_settings=CalibrationOptimizerSettings(
            solver_name="lbfgs",
            max_iterations=250,
            absolute_tolerance=1e-8,
            relative_tolerance=1e-6,
            constraint_penalty_weight=10.0,
        ),
    )
