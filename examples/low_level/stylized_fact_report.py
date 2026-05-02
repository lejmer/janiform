from typing import Any, Optional

from janiform.stylized_facts import (
    StylizedFactHorizonComparisonReportBundle,
    run_stylized_fact_horizon_comparison_report_generator,
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
            "required_catalog_fields": [
                "schema_version",
                "catalog_id",
                "catalog_version",
                "schema_metadata",
                "naming_conventions",
                "uncertainty_policy",
                "insufficient_data_semantics",
                "output_schema_metadata",
                "metrics",
            ],
            "required_metric_fields": [
                "metric_id",
                "metric_name",
                "metric_class",
                "metric_family",
                "domain",
                "definition",
                "unit",
                "sample_sufficiency",
                "uncertainty",
                "tags",
            ],
            "required_primary_metric_fields": [
                "metric_id",
                "metric_name",
                "metric_class",
                "metric_family",
                "domain",
                "definition",
                "unit",
                "sample_sufficiency",
                "uncertainty",
                "tags",
            ],
            "required_secondary_metric_fields": [
                "metric_id",
                "metric_name",
                "metric_class",
                "metric_family",
                "domain",
                "definition",
                "unit",
                "sample_sufficiency",
                "uncertainty",
                "tags",
                "secondary_moment_variant",
                "estimator_metadata",
            ],
            "required_uncertainty_fields": [
                "policy_id",
                "method",
                "confidence_level",
            ],
            "required_sample_sufficiency_fields": [
                "minimum_sample_count",
                "minimum_effective_sample_count",
                "minimum_tail_observation_count",
                "minimum_horizon_coverage_ratio",
            ],
            "required_estimator_metadata_fields": [
                "estimator_id",
                "estimator_version",
                "estimator_family",
                "bias_correction",
                "finite_sample_adjustment",
            ],
            "canonical_metric_class_vocabulary": [
                "primary_robust",
                "secondary_moment",
            ],
            "canonical_metric_family_vocabulary": [
                "quantile_shape",
                "l_moment",
            ],
            "canonical_domain_vocabulary": [
                "heavy_tails",
            ],
            "canonical_uncertainty_method_vocabulary": [
                "block_bootstrap",
                "stationary_bootstrap",
                "circular_block_bootstrap",
                "analytical_asymptotic",
            ],
        },
        "metrics": [
            {
                "metric_id": "returns_abs_quantile_shape_ratio_q99_q95",
                "metric_name": "Returns Abs Quantile Shape Ratio Q99 Q95",
                "metric_class": "primary_robust",
                "metric_family": "quantile_shape",
                "domain": "heavy_tails",
                "definition": "ratio of absolute return 99th to 95th percentile",
                "unit": "ratio",
                "sample_sufficiency": {
                    "minimum_sample_count": 500,
                    "minimum_effective_sample_count": 350,
                    "minimum_tail_observation_count": None,
                    "minimum_horizon_coverage_ratio": 0.85,
                },
                "uncertainty": {
                    "policy_id": "dependent_return_series_default_block_bootstrap_with_approved_overrides.v1",
                    "method": "block_bootstrap",
                    "confidence_level": 0.95,
                },
                "tags": ["returns", "heavy_tails", "quantile_shape"],
            },
            {
                "metric_id": "returns_l_moment_lambda2",
                "metric_name": "Returns L Moment Lambda2",
                "metric_class": "secondary_moment",
                "metric_family": "l_moment",
                "domain": "heavy_tails",
                "definition": "second L-moment for return dispersion",
                "unit": "return_units",
                "sample_sufficiency": {
                    "minimum_sample_count": 300,
                    "minimum_effective_sample_count": 240,
                    "minimum_tail_observation_count": None,
                    "minimum_horizon_coverage_ratio": 0.8,
                },
                "uncertainty": {
                    "policy_id": "dependent_return_series_default_block_bootstrap_with_approved_overrides.v1",
                    "method": "block_bootstrap",
                    "confidence_level": 0.95,
                },
                "secondary_moment_variant": {
                    "variant": "l_moment",
                    "moment_order": 2,
                    "ratio_denominator_order": None,
                },
                "estimator_metadata": {
                    "estimator_id": "pwms_l_moment_estimator",
                    "estimator_version": "1.0.0",
                    "estimator_family": "l_moment",
                    "bias_correction": "none",
                    "finite_sample_adjustment": "pwm_unbiased_order_statistic_weights",
                },
                "tags": ["returns", "l_moment", "dispersion"],
            },
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
                    build_observed_metric(
                        horizon_label="1h",
                        metric_id="returns_abs_quantile_shape_ratio_q99_q95",
                        metric_name="Returns Abs Quantile Shape Ratio Q99 Q95",
                        value=1.0,
                    ),
                    build_observed_metric(
                        horizon_label="1h",
                        metric_id="returns_l_moment_lambda2",
                        metric_name="Returns L Moment Lambda2",
                        value=None,
                    ),
                ],
                "simulated_metrics": [
                    build_observed_metric(
                        horizon_label="1h",
                        metric_id="returns_abs_quantile_shape_ratio_q99_q95",
                        metric_name="Returns Abs Quantile Shape Ratio Q99 Q95",
                        value=1.1,
                    ),
                    build_observed_metric(
                        horizon_label="1h",
                        metric_id="returns_l_moment_lambda2",
                        metric_name="Returns L Moment Lambda2",
                        value=0.4,
                    ),
                ],
            }
        ],
    }


def build_observed_metric(
    *,
    horizon_label: str,
    metric_id: str,
    metric_name: str,
    value: Optional[float],
) -> dict[str, Any]:
    return {
        "horizon_label": horizon_label,
        "metric_id": metric_id,
        "metric_name": metric_name,
        "metric_variant": None,
        "value": value,
        "confidence_interval": (
            {
                "lower_bound": value - 0.05,
                "upper_bound": value + 0.05,
                "confidence_level": 0.95,
            }
            if value is not None
            else None
        ),
        "sample_coverage": {
            "horizon_interval_count": 12,
            "covered_interval_count": 10,
            "sample_count": 900,
            "effective_sample_count": 700,
            "tail_observation_count": 65,
            "minimum_sample_count": 300,
            "minimum_effective_sample_count": 240,
            "minimum_tail_observation_count": 0,
            "minimum_horizon_coverage_ratio_scaled_bps": 8000,
        },
        "status": "available" if value is not None else "insufficient_data",
    }


def build_stylized_fact_report_bundle() -> StylizedFactHorizonComparisonReportBundle:
    return run_stylized_fact_horizon_comparison_report_generator(
        metric_catalog=build_stylized_fact_metric_catalog(),
        comparison_request=build_stylized_fact_horizon_comparison_request(),
        execution_mode="memory",
    )


def main() -> None:
    build_stylized_fact_report_bundle()


if __name__ == "__main__":
    main()
