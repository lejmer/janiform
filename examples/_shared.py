import json
from datetime import UTC, datetime
from pathlib import Path
from typing import Any, Optional

import pyarrow as pa
from pyarrow import parquet as pyarrow_parquet

from janiform.calendar import CalendarRegistry
from janiform.calibration import (
    CALIBRATION_STAGED_EXECUTION_INPUT_SCHEMA_VERSION,
    CalibrationCoupledGapMarkConfiguration,
    CalibrationDayOfWeekSeasonalityEstimatorSettings,
    CalibrationDayOfYearSeasonalityEstimatorSettings,
    CalibrationDoubleExponentialBaseShockConfiguration,
    CalibrationHawkesDirectionalLabelDiagnostics,
    CalibrationHawkesObservedSourceRegime,
    CalibrationExecutionMode,
    CalibrationHawkesStageCandidateInput,
    CalibrationHawkesStructuredAlphaParameterization,
    CalibrationHawkesStructuredRhoParameterization,
    CalibrationHawkesStructuredRhoSelfDominancePolicy,
    CalibrationIntradaySeasonalityEstimatorSettings,
    CalibrationIntradaySmoothingKernel,
    CalibrationOptimizerSettings,
    CalibrationReturnComponentsCloseOpenGapDetectionPolicy,
    CalibrationReturnComponentsDetectedIntradayJumpCandidate,
    CalibrationReturnComponentsKnownNewsCalendarInput,
    CalibrationReturnComponentsKnownNewsReturnSample,
    CalibrationReturnComponentsObservedCloseOpenBoundaryObservation,
    CalibrationReturnComponentsObservedOpenSessionReturn,
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
    CoarseSimulationRunRequest,
    CoupledGapMarkConfiguration,
    DoubleExponentialBaseShockConfiguration,
    HawkesExcitationState,
    HawkesKernelLoadings,
    HawkesKernelParameters,
    KnownNewsArrivalConfiguration,
    NewsArrivalConfiguration,
    NewsComponentMarkScaleConfiguration,
    NewsProcessConfiguration,
    SharedNewsMarkLawConfiguration,
    SurpriseNewsArrivalConfiguration,
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
STYLIZED_FACT_METRIC_CATALOG_REQUIRED_FIELDS: tuple[str, ...] = (
    "schema_version",
    "catalog_id",
    "catalog_version",
    "schema_metadata",
    "naming_conventions",
    "uncertainty_policy",
    "insufficient_data_semantics",
    "output_schema_metadata",
    "metrics",
)
STYLIZED_FACT_METRIC_REQUIRED_FIELDS: tuple[str, ...] = (
    "metric_id",
    "metric_name",
    "metric_class",
    "metric_family",
    "domain",
    "definition",
    "unit",
    "horizon_applicability",
    "sample_dependence",
    "uncertainty",
    "sample_sufficiency",
    "tags",
)
STYLIZED_FACT_SECONDARY_METRIC_REQUIRED_FIELDS: tuple[str, ...] = (
    "metric_id",
    "metric_name",
    "metric_class",
    "metric_family",
    "domain",
    "definition",
    "unit",
    "horizon_applicability",
    "sample_dependence",
    "uncertainty",
    "sample_sufficiency",
    "estimator_metadata",
    "secondary_moment_variant",
    "tags",
)
STYLIZED_FACT_UNCERTAINTY_REQUIRED_FIELDS: tuple[str, ...] = (
    "default_method",
    "confidence_level",
    "bootstrap_replication_count",
    "block_length_horizon_count",
    "override_policy",
)
STYLIZED_FACT_SAMPLE_SUFFICIENCY_REQUIRED_FIELDS: tuple[str, ...] = (
    "minimum_sample_count",
    "minimum_effective_sample_count",
    "minimum_tail_observation_count",
    "minimum_horizon_coverage_ratio",
)
STYLIZED_FACT_ESTIMATOR_METADATA_REQUIRED_FIELDS: tuple[str, ...] = (
    "estimator_id",
    "estimator_version",
    "estimator_family",
    "bias_correction",
    "finite_sample_adjustment",
)
STYLIZED_FACT_METRIC_CLASS_VOCABULARY: tuple[str, ...] = (
    "primary_robust",
    "secondary_moment",
)
STYLIZED_FACT_METRIC_FAMILY_VOCABULARY: tuple[str, ...] = (
    "quantile_shape",
    "tail_risk",
    "distribution_distance",
    "l_moment",
    "central_moment",
)
STYLIZED_FACT_DOMAIN_VOCABULARY: tuple[str, ...] = (
    "activity_clustering",
    "heavy_tails",
    "skew",
    "volatility_persistence",
    "open_window_direct_jump_behavior",
    "closed_window_gap_behavior",
)
STYLIZED_FACT_UNCERTAINTY_METHOD_VOCABULARY: tuple[str, ...] = (
    "block_bootstrap",
    "stationary_bootstrap",
    "circular_block_bootstrap",
    "analytical_asymptotic",
)


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


def build_simulation_nig_shared_plus_shift_configuration() -> NigSharedPlusShiftConfiguration:
    return NigSharedPlusShiftConfiguration(
        alpha=2.3,
        beta_0=-0.8,
        delta=0.7,
        mu=0.1,
        delta_beta=-0.2,
    )


def build_simulation_news_process_configuration() -> NewsProcessConfiguration:
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


def build_coarse_simulation_request(calendar_registry: CalendarRegistry) -> CoarseSimulationRunRequest:
    request_payload: dict[str, Any] = {
        "schema_version": "simulation.coarse_interval.run_request.v1",
        "seed": 3_010,
        "start_time_utc_nanos": int(SIMULATION_RANGE_START_UTC.timestamp()) * 1_000_000_000,
        "horizon_interval_count": 3,
        "interval_delta": 1.0,
        "model_inputs": {
            "risk_model_inputs": {
                "equation_specification": {
                    "hawkes_kernel_parameters": build_hawkes_kernel_parameters().to_payload(),
                    "gamma_severity_mark_configuration": GammaSeverityMarkConfiguration(
                        shape_k=1.8,
                        scale_theta=0.7,
                    ).to_payload(),
                },
                "nig_shared_plus_shift_configuration": NigSharedPlusShiftConfiguration(
                    alpha=2.3,
                    beta_0=-0.8,
                    delta=0.7,
                    mu=0.1,
                    delta_beta=-0.2,
                ).to_payload(),
                "initial_excitation_state": build_initial_excitation_state().to_payload(),
            },
            "news_process_configuration": NewsProcessConfiguration(
                schema_version="news.process.configuration.v1",
                arrivals=NewsArrivalConfiguration(
                    known=KnownNewsArrivalConfiguration(
                        scheduled_event_timestamps_utc_nanos=(
                            int(datetime(2026, 2, 2, 14, 5, 0, tzinfo=UTC).timestamp()) * 1_000_000_000,
                            int(datetime(2026, 2, 2, 15, 10, 0, tzinfo=UTC).timestamp()) * 1_000_000_000,
                        ),
                    ),
                    surprise=SurpriseNewsArrivalConfiguration(baseline_intensity=0.1),
                ),
                shared_mark_law=SharedNewsMarkLawConfiguration(
                    gamma_severity_marks=GammaSeverityMarkConfiguration(
                        shape_k=1.4,
                        scale_theta=0.6,
                    ),
                    double_exponential_base_shock=DoubleExponentialBaseShockConfiguration(
                        probability_positive=0.35,
                        eta_plus=2.1,
                        eta_minus=3.0,
                    ),
                    coupled_gap_marks=CoupledGapMarkConfiguration(sigma_g=0.8),
                ),
                known_component_mark_scales=NewsComponentMarkScaleConfiguration(
                    activity_coupling_scale=1.0,
                    return_jump_scale=1.0,
                ),
                surprise_component_mark_scales=NewsComponentMarkScaleConfiguration(
                    activity_coupling_scale=1.0,
                    return_jump_scale=1.0,
                ),
            ).to_payload(),
            "calendar_registry_artifact": dict(calendar_registry._registry_payload),
            "exchange_code": "XNYS",
        },
        "interval_factors": {
            "interval_delta": 1.0,
            "s_h": [1.0, 0.9, 1.1],
            "nu_h_known": [1.0, 1.0, 0.0],
            "nu_h_surprise": [0.1, 0.2, 0.1],
        },
    }

    return CoarseSimulationRunRequest.from_mapping(request_payload)


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


def build_stylized_fact_metric_catalog() -> dict[str, Any]:
    return {
        "schema_version": "validation.stylized_fact.metric_catalog.v1",
        "catalog_id": "stylized_fact_metric_catalog",
        "catalog_version": "1.0.0",
        "schema_metadata": {
            "schema_family": "validation.stylized_fact.metric_catalog",
            "schema_generation": "v1",
            "forward_extension_policy": "additive_fields_only.v1",
        },
        "naming_conventions": {
            "naming_policy": "snake_case_metric_ids_used_as_canonical_output_keys.v1",
            "metric_id_format": "snake_case",
            "horizon_label_policy": "explicit_horizon_bucket_labels_with_no_implicit_interpolation.v1",
            "canonical_primary_metric_class_label": "primary_robust",
            "canonical_secondary_metric_class_label": "secondary_moment",
        },
        "uncertainty_policy": {
            "default_policy_id": "dependent_return_series_default_block_bootstrap_with_approved_overrides.v1",
            "dependent_return_series_default_method": "block_bootstrap",
            "default_confidence_level": 0.95,
            "default_bootstrap_replication_count": 2000,
            "default_block_length_horizon_count": 5,
            "override_approval_policy": "allow_only_explicitly_whitelisted_uncertainty_methods.v1",
            "approved_override_methods": [
                "stationary_bootstrap",
                "circular_block_bootstrap",
                "analytical_asymptotic",
            ],
        },
        "insufficient_data_semantics": {
            "status_code": "insufficient_data",
            "decision_policy": "emit_insufficient_data_with_coverage_and_skip_threshold_decision.v1",
            "non_terminal_decision_state": True,
            "required_result_fields": [
                "status",
                "sample_coverage",
                "required_minimums",
                "observed_counts",
            ],
        },
        "output_schema_metadata": {
            "schema_version": "validation.stylized_fact.metric_catalog.output_schema_metadata.v1",
            "required_catalog_fields": list(STYLIZED_FACT_METRIC_CATALOG_REQUIRED_FIELDS),
            "required_metric_fields": list(STYLIZED_FACT_METRIC_REQUIRED_FIELDS),
            "required_primary_metric_fields": list(STYLIZED_FACT_METRIC_REQUIRED_FIELDS),
            "required_secondary_metric_fields": list(STYLIZED_FACT_SECONDARY_METRIC_REQUIRED_FIELDS),
            "required_uncertainty_fields": list(STYLIZED_FACT_UNCERTAINTY_REQUIRED_FIELDS),
            "required_sample_sufficiency_fields": list(STYLIZED_FACT_SAMPLE_SUFFICIENCY_REQUIRED_FIELDS),
            "required_estimator_metadata_fields": list(STYLIZED_FACT_ESTIMATOR_METADATA_REQUIRED_FIELDS),
            "canonical_metric_class_vocabulary": list(STYLIZED_FACT_METRIC_CLASS_VOCABULARY),
            "canonical_metric_family_vocabulary": list(STYLIZED_FACT_METRIC_FAMILY_VOCABULARY),
            "canonical_domain_vocabulary": list(STYLIZED_FACT_DOMAIN_VOCABULARY),
            "canonical_uncertainty_method_vocabulary": list(STYLIZED_FACT_UNCERTAINTY_METHOD_VOCABULARY),
        },
        "metrics": [
            _build_stylized_fact_primary_metric(
                metric_id="returns_abs_quantile_shape_ratio_q99_q95",
                metric_name="Returns Abs Quantile Shape Ratio Q99 Q95",
                metric_family="quantile_shape",
                domain="heavy_tails",
                definition="ratio of absolute return 99th to 95th percentile",
                unit="ratio",
                minimum_sample_count=500,
                minimum_effective_sample_count=350,
                minimum_tail_observation_count=None,
                minimum_horizon_coverage_ratio=0.85,
                tags=["returns", "heavy_tails", "quantile_shape"],
            ),
            _build_stylized_fact_secondary_metric(
                metric_id="returns_l_moment_lambda2",
                metric_name="Returns L Moment Lambda2",
                metric_family="l_moment",
                domain="heavy_tails",
                definition="second L-moment for return dispersion",
                unit="return_units",
                minimum_sample_count=300,
                minimum_effective_sample_count=240,
                minimum_tail_observation_count=None,
                minimum_horizon_coverage_ratio=0.8,
                estimator_metadata={
                    "estimator_id": "pwms_l_moment_estimator",
                    "estimator_version": "1.0.0",
                    "estimator_family": "l_moment",
                    "bias_correction": "none",
                    "finite_sample_adjustment": "pwm_unbiased_order_statistic_weights",
                },
                secondary_moment_variant={
                    "variant": "l_moment",
                    "moment_order": 2,
                    "ratio_denominator_order": None,
                },
                tags=["returns", "l_moment", "dispersion"],
            ),
        ],
    }


def build_stylized_fact_horizon_comparison_request() -> dict[str, Any]:
    return {
        "metric_catalog_id": "stylized_fact_metric_catalog",
        "metric_catalog_version": "1.0.0",
        "threshold_policy": {
            "policy_id": "stylized_fact_delta_policy.v1",
            "default_max_absolute_delta": 0.2,
            "default_max_standardized_delta": 5.0,
            "metric_threshold_overrides": [
                {
                    "metric_id": "returns_l_moment_lambda2",
                    "metric_variant": None,
                    "max_absolute_delta": 0.25,
                    "max_standardized_delta": 4.0,
                }
            ],
        },
        "calibration_runs": [
            {
                "calibration_run_id": "run_001",
                "empirical_dataset_id": "empirical_set",
                "simulated_dataset_id": "simulated_set",
                "empirical_metrics": [
                    _build_stylized_fact_observed_metric(
                        horizon_label="1h",
                        metric_id="returns_abs_quantile_shape_ratio_q99_q95",
                        metric_name="Returns Abs Quantile Shape Ratio Q99 Q95",
                        value=1.0,
                    ),
                    _build_stylized_fact_observed_metric(
                        horizon_label="1h",
                        metric_id="returns_l_moment_lambda2",
                        metric_name="Returns L Moment Lambda2",
                        value=None,
                    ),
                ],
                "simulated_metrics": [
                    _build_stylized_fact_observed_metric(
                        horizon_label="1h",
                        metric_id="returns_abs_quantile_shape_ratio_q99_q95",
                        metric_name="Returns Abs Quantile Shape Ratio Q99 Q95",
                        value=1.1,
                    ),
                    _build_stylized_fact_observed_metric(
                        horizon_label="1h",
                        metric_id="returns_l_moment_lambda2",
                        metric_name="Returns L Moment Lambda2",
                        value=0.4,
                    ),
                ],
            }
        ],
    }


def build_stylized_fact_artifact_export_config(
    *,
    export_directory: Path,
    file_stem: str = "stylized_fact_horizon_report",
) -> dict[str, Any]:
    return {
        "export_directory": str(export_directory),
        "file_stem": file_stem,
    }


def _build_stylized_fact_uncertainty_config() -> dict[str, Any]:
    return {
        "default_method": "block_bootstrap",
        "confidence_level": 0.95,
        "bootstrap_replication_count": 2000,
        "block_length_horizon_count": 5,
        "override_policy": {
            "override_allowed": False,
            "approved_alternative_methods": [],
        },
    }


def _build_stylized_fact_sample_sufficiency(
    *,
    minimum_sample_count: int,
    minimum_effective_sample_count: int,
    minimum_tail_observation_count: Optional[int],
    minimum_horizon_coverage_ratio: float,
) -> dict[str, Any]:
    return {
        "minimum_sample_count": minimum_sample_count,
        "minimum_effective_sample_count": minimum_effective_sample_count,
        "minimum_tail_observation_count": minimum_tail_observation_count,
        "minimum_horizon_coverage_ratio": minimum_horizon_coverage_ratio,
    }


def _build_stylized_fact_primary_metric(
    *,
    metric_id: str,
    metric_name: str,
    metric_family: str,
    domain: str,
    definition: str,
    unit: str,
    minimum_sample_count: int,
    minimum_effective_sample_count: int,
    minimum_tail_observation_count: Optional[int],
    minimum_horizon_coverage_ratio: float,
    tags: list[str],
) -> dict[str, Any]:
    return {
        "metric_id": metric_id,
        "metric_name": metric_name,
        "metric_class": "primary_robust",
        "metric_family": metric_family,
        "domain": domain,
        "definition": definition,
        "unit": unit,
        "horizon_applicability": "all_configured_horizons",
        "sample_dependence": "dependent_return_series",
        "uncertainty": _build_stylized_fact_uncertainty_config(),
        "sample_sufficiency": _build_stylized_fact_sample_sufficiency(
            minimum_sample_count=minimum_sample_count,
            minimum_effective_sample_count=minimum_effective_sample_count,
            minimum_tail_observation_count=minimum_tail_observation_count,
            minimum_horizon_coverage_ratio=minimum_horizon_coverage_ratio,
        ),
        "tags": tags,
    }


def _build_stylized_fact_secondary_metric(
    *,
    metric_id: str,
    metric_name: str,
    metric_family: str,
    domain: str,
    definition: str,
    unit: str,
    minimum_sample_count: int,
    minimum_effective_sample_count: int,
    minimum_tail_observation_count: Optional[int],
    minimum_horizon_coverage_ratio: float,
    estimator_metadata: dict[str, Any],
    secondary_moment_variant: dict[str, Any],
    tags: list[str],
) -> dict[str, Any]:
    return {
        "metric_id": metric_id,
        "metric_name": metric_name,
        "metric_class": "secondary_moment",
        "metric_family": metric_family,
        "domain": domain,
        "definition": definition,
        "unit": unit,
        "horizon_applicability": "all_configured_horizons",
        "sample_dependence": "dependent_return_series",
        "uncertainty": _build_stylized_fact_uncertainty_config(),
        "sample_sufficiency": _build_stylized_fact_sample_sufficiency(
            minimum_sample_count=minimum_sample_count,
            minimum_effective_sample_count=minimum_effective_sample_count,
            minimum_tail_observation_count=minimum_tail_observation_count,
            minimum_horizon_coverage_ratio=minimum_horizon_coverage_ratio,
        ),
        "estimator_metadata": estimator_metadata,
        "secondary_moment_variant": secondary_moment_variant,
        "tags": tags,
    }


def _build_stylized_fact_observed_metric(
    *,
    horizon_label: str,
    metric_id: str,
    metric_name: str,
    value: Optional[float],
) -> dict[str, Any]:
    confidence_interval: Optional[dict[str, Any]] = None
    if value is not None:
        confidence_interval = {
            "lower_bound": value - 0.05,
            "upper_bound": value + 0.05,
            "confidence_level": 0.95,
        }

    return {
        "horizon_label": horizon_label,
        "horizon_duration_nanos": 3_600_000_000_000,
        "metric_id": metric_id,
        "metric_name": metric_name,
        "metric_variant": None,
        "value": value,
        "confidence_interval": confidence_interval,
        "sample_coverage": {
            "horizon_interval_count": 12,
            "covered_interval_count": 10,
        },
        "observed_counts": {
            "sample_count": 100,
            "effective_sample_count": 80,
            "tail_observation_count": 20,
        },
        "required_minimums": {
            "minimum_sample_count": 10,
            "minimum_effective_sample_count": 8,
            "minimum_tail_observation_count": 1,
            "minimum_horizon_coverage_ratio": 0.75,
        },
    }


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
                source_regime=CalibrationHawkesObservedSourceRegime(
                    observation_duration_hours=12.0,
                    open_session_duration_hours=8.0,
                    total_tick_count=95,
                    open_session_tick_count=84,
                    closed_session_tick_count=11,
                    open_session_buy_tick_count=39,
                    open_session_sell_tick_count=45,
                    open_session_mean_interarrival_seconds=34.0,
                    open_session_interarrival_coefficient_of_variation=1.18,
                    open_session_short_gap_threshold_seconds=17.0,
                    open_session_short_gap_ratio=0.43,
                    known_news_open_event_count=2,
                    open_session_known_news_activity_share=0.06,
                ),
                directional_label_diagnostics=CalibrationHawkesDirectionalLabelDiagnostics(
                    policy_id="tick_price_change_with_flat_move_carryover.v1",
                    first_inferred_trade_sign="buy",
                    provided_label_count=10,
                    inferred_label_count=74,
                    flat_move_carry_count=9,
                    price_up_inference_count=35,
                    price_down_inference_count=30,
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
                    post_shock_activity_lift=1.20,
                ),
                CalibrationReturnComponentsKnownNewsReturnSample(
                    event_timestamp_utc_nanos=1770120000000000000,
                    open_window_jump_return=-0.012,
                    closed_window_gap_return=0.004,
                    post_shock_activity_lift=1.05,
                ),
            ),
            observed_open_session_returns=(
                CalibrationReturnComponentsObservedOpenSessionReturn(
                    return_timestamp_utc_nanos=1770046200000000000,
                    log_return=0.004,
                    post_shock_activity_lift=1.00,
                ),
                CalibrationReturnComponentsObservedOpenSessionReturn(
                    return_timestamp_utc_nanos=1770048300000000000,
                    log_return=0.018,
                    post_shock_activity_lift=1.35,
                ),
                CalibrationReturnComponentsObservedOpenSessionReturn(
                    return_timestamp_utc_nanos=1770123600000000000,
                    log_return=-0.006,
                    post_shock_activity_lift=0.95,
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
            open_session_sell_share_proxy=0.52,
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
                continuous_distribution_fit_weight=1.0,
                jump_fit_weight=1.0,
                gap_fit_weight=1.0,
                activity_coupling_fit_weight=1.0,
                price_range_sanity_fit_weight=1.0,
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
