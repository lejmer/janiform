from datetime import UTC, datetime
from pathlib import Path
from typing import Any, Optional

from janiform._timestamps import ONE_HOUR_NANOS
from janiform.calendar import CalendarRegistry
from janiform.pricing import (
    AnchoredVanillaOptionPricingBoundaryInput,
    GammaSeverityMarkConfiguration,
    MeanReversionAnchorModel,
    NigSharedPlusShiftConfiguration,
    VanillaOptionPayoffConfiguration,
    VanillaOptionPayoffType,
    VanillaOptionPricingMarketInputs,
    VanillaOptionPricingModelParameters,
    VanillaOptionPricingOptionalOutputSelection,
    VanillaOptionPricingRequest,
)
from janiform.simulation.coarse import (
    CoarseSimulationRunRequest,
    CoupledGapMarkConfiguration,
    DoubleExponentialBaseShockConfiguration,
    KnownNewsArrivalConfiguration,
    NewsArrivalConfiguration,
    NewsComponentMarkScaleConfiguration,
    NewsProcessConfiguration,
    SharedNewsMarkLawConfiguration,
    SurpriseNewsArrivalConfiguration,
)
from janiform.simulation.shared import (
    HawkesExcitationState,
    HawkesKernelLoadings,
    HawkesKernelParameters,
    MeanReversionBoundaryState,
    MeanReversionFittedParameters,
    MeanReversionRuntimeIntervalAnchors,
)
from tests.support.hourly_factor_contracts import build_hourly_factor_contract

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
SIMULATION_RANGE_START_UTC: datetime = datetime(2026, 2, 2, 14, 0, 0, tzinfo=UTC)


def build_coarse_simulation_request(calendar_registry: CalendarRegistry) -> CoarseSimulationRunRequest:
    horizon_interval_count = 3
    interval_delta = 1.0
    mean_reversion_runtime_interval_anchors = MeanReversionRuntimeIntervalAnchors(
        interval_alignment_policy="explicit_runtime_interval_grid_alignment.v1",
        start_anchor_value=0.0,
        start_pressure_state=0.0,
        end_anchor_values=tuple(0.0 for _ in range(horizon_interval_count)),
        end_pressure_states=tuple(0.0 for _ in range(horizon_interval_count)),
        interval_deltas=tuple(interval_delta for _ in range(horizon_interval_count)),
    )
    request_payload: dict[str, Any] = {
        "schema_version": "simulation.coarse_interval.run_request.v1",
        "seed": 3_010,
        "start_time_utc_nanos": int(SIMULATION_RANGE_START_UTC.timestamp()) * 1_000_000_000,
        "horizon_interval_count": horizon_interval_count,
        "interval_delta": interval_delta,
        "model_inputs": {
            "risk_model_inputs": {
                "equation_specification": {
                    "hawkes_kernel_parameters": _build_simulation_hawkes_kernel_parameters().to_payload(),
                    "gamma_severity_mark_configuration": GammaSeverityMarkConfiguration(
                        shape_k=1.8,
                        scale_theta=0.7,
                    ).to_payload(),
                },
                "nig_shared_plus_shift_configuration": (
                    _build_simulation_nig_shared_plus_shift_configuration().to_payload()
                ),
                "initial_excitation_state": _build_initial_excitation_state().to_payload(),
                "mean_reversion_fitted_parameters": MeanReversionFittedParameters(
                    kappa=0.0,
                    anchor_model=MeanReversionAnchorModel.zero_gain_endogenous_core_pressure_level(),
                ).to_payload(),
                "mean_reversion_runtime_interval_anchors": (
                    mean_reversion_runtime_interval_anchors.to_payload()
                ),
                "initial_mean_reversion_boundary_state": MeanReversionBoundaryState(
                    observed_log_price=0.0,
                    anchor_value=mean_reversion_runtime_interval_anchors.start_anchor_value,
                    pressure_state=mean_reversion_runtime_interval_anchors.start_pressure_state,
                    deviation=0.0,
                ).to_payload(),
            },
            "news_process_configuration": _build_coarse_simulation_news_process_configuration().to_payload(),
            "calendar_registry_artifact": dict(calendar_registry._registry_payload),
            "exchange_code": "XNYS",
        },
        "hourly_factor_contract": build_hourly_factor_contract(
            request_start_utc_nanos=int(SIMULATION_RANGE_START_UTC.timestamp()) * 1_000_000_000,
            interval_duration_nanos=int(interval_delta * ONE_HOUR_NANOS),
            s_h=(1.0, 0.9, 1.1),
            known_component_nu_h=(1.0, 1.0, 0.0),
            surprise_component_nu_h=(0.1, 0.2, 0.1),
        ).to_payload(),
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
        boundary_input=AnchoredVanillaOptionPricingBoundaryInput(
            interval_index=0,
            hawkes_boundary_state=_build_initial_excitation_state(),
            mean_reversion_fitted_parameters=MeanReversionFittedParameters(
                kappa=0.35,
                anchor_model=MeanReversionAnchorModel.zero_gain_endogenous_core_pressure_level(),
            ),
            interval_delta=1.0,
            observed_boundary_state=MeanReversionBoundaryState(
                observed_log_price=0.25,
                anchor_value=0.25,
                pressure_state=0.0,
                deviation=0.0,
            ),
        ),
        model_parameters=VanillaOptionPricingModelParameters(
            hawkes_kernel_parameters=_build_pricing_hawkes_kernel_parameters(),
            news_process_configuration=_build_simulation_news_process_configuration(),
            nig_shared_plus_shift_configuration=_build_simulation_nig_shared_plus_shift_configuration(),
        ),
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


def _build_initial_excitation_state() -> HawkesExcitationState:
    return HawkesExcitationState(
        y_plus_plus=0.0,
        y_plus_minus=0.0,
        y_minus_plus=0.0,
        y_minus_minus=0.0,
    )


def _build_pricing_hawkes_kernel_parameters() -> HawkesKernelParameters:
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
            plus_plus=0.1,
            plus_minus=0.05,
            minus_plus=0.05,
            minus_minus=0.1,
        ),
    )


def _build_simulation_hawkes_kernel_parameters() -> HawkesKernelParameters:
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


def _build_simulation_nig_shared_plus_shift_configuration() -> NigSharedPlusShiftConfiguration:
    return NigSharedPlusShiftConfiguration(
        alpha=2.3,
        beta_0=-0.8,
        delta=0.7,
        mu=0.1,
        delta_beta=-0.2,
    )


def _build_coarse_simulation_news_process_configuration() -> NewsProcessConfiguration:
    return NewsProcessConfiguration(
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
        "horizon_duration_nanos": ONE_HOUR_NANOS,
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
