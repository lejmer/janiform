//! Stylized-fact metric catalog builders and JSON IO helpers.

use super::constants::*;
use super::core::*;
use super::errors::{StylizedFactMetricCatalogLoadError, StylizedFactMetricCatalogSerializeError};

pub fn load_stylized_fact_metric_catalog_from_json(
    catalog_payload: &str,
) -> Result<StylizedFactMetricCatalog, StylizedFactMetricCatalogLoadError> {
    let mut catalog: StylizedFactMetricCatalog =
        serde_json::from_str(catalog_payload).map_err(|source| {
            StylizedFactMetricCatalogLoadError::InvalidJson {
                source: source.to_string(),
            }
        })?;
    catalog
        .validate()
        .map_err(|source| StylizedFactMetricCatalogLoadError::InvalidCatalog { source })?;
    catalog.sort_metrics_canonically();

    Ok(catalog)
}

pub fn serialize_stylized_fact_metric_catalog_to_json(
    catalog: &StylizedFactMetricCatalog,
) -> Result<String, StylizedFactMetricCatalogSerializeError> {
    catalog
        .validate()
        .map_err(|source| StylizedFactMetricCatalogSerializeError::InvalidCatalog { source })?;

    serde_json::to_string_pretty(catalog).map_err(|source| {
        StylizedFactMetricCatalogSerializeError::SerializationFailure {
            source: source.to_string(),
        }
    })
}

pub fn build_stylized_fact_metric_catalog() -> StylizedFactMetricCatalog {
    let mut catalog = StylizedFactMetricCatalog {
        schema_version: STYLIZED_FACT_METRIC_CATALOG_SCHEMA_VERSION.to_string(),
        catalog_id: "stylized_fact_metric_catalog".to_string(),
        catalog_version: "1.0.0".to_string(),
        schema_metadata: StylizedFactMetricCatalogSchemaMetadata {
            schema_family: STYLIZED_FACT_METRIC_CATALOG_SCHEMA_FAMILY.to_string(),
            schema_generation: STYLIZED_FACT_METRIC_CATALOG_SCHEMA_GENERATION.to_string(),
            forward_extension_policy: STYLIZED_FACT_METRIC_CATALOG_FORWARD_EXTENSION_POLICY
                .to_string(),
        },
        naming_conventions: StylizedFactMetricNamingConventions {
            naming_policy: STYLIZED_FACT_METRIC_CATALOG_NAMING_POLICY.to_string(),
            metric_id_format: "snake_case".to_string(),
            horizon_label_policy: STYLIZED_FACT_METRIC_CATALOG_HORIZON_LABEL_POLICY.to_string(),
            canonical_primary_metric_class_label: "primary_robust".to_string(),
            canonical_secondary_metric_class_label: "secondary_moment".to_string(),
        },
        uncertainty_policy: StylizedFactMetricCatalogUncertaintyPolicy {
            default_policy_id: STYLIZED_FACT_METRIC_CATALOG_UNCERTAINTY_POLICY_ID.to_string(),
            dependent_return_series_default_method: StylizedFactUncertaintyMethod::BlockBootstrap,
            default_confidence_level: 0.95,
            default_bootstrap_replication_count: 2000,
            default_block_length_horizon_count: 5,
            override_approval_policy:
                STYLIZED_FACT_METRIC_CATALOG_UNCERTAINTY_OVERRIDE_APPROVAL_POLICY.to_string(),
            approved_override_methods: vec![
                StylizedFactUncertaintyMethod::StationaryBootstrap,
                StylizedFactUncertaintyMethod::CircularBlockBootstrap,
                StylizedFactUncertaintyMethod::AnalyticalAsymptotic,
            ],
        },
        insufficient_data_semantics: StylizedFactInsufficientDataSemantics {
            status_code: STYLIZED_FACT_METRIC_CATALOG_INSUFFICIENT_DATA_STATUS_CODE.to_string(),
            decision_policy: STYLIZED_FACT_METRIC_CATALOG_INSUFFICIENT_DATA_DECISION_POLICY
                .to_string(),
            non_terminal_decision_state: true,
            required_result_fields: required_field_list_to_owned(&[
                "status",
                "sample_coverage",
                "required_minimums",
                "observed_counts",
            ]),
        },
        output_schema_metadata: StylizedFactMetricCatalogOutputSchemaMetadata {
            schema_version: STYLIZED_FACT_METRIC_CATALOG_OUTPUT_SCHEMA_METADATA_VERSION.to_string(),
            required_catalog_fields: required_field_list_to_owned(
                &STYLIZED_FACT_METRIC_CATALOG_REQUIRED_FIELDS,
            ),
            required_metric_fields: required_field_list_to_owned(
                &STYLIZED_FACT_METRIC_REQUIRED_FIELDS,
            ),
            required_primary_metric_fields: required_field_list_to_owned(
                &STYLIZED_FACT_PRIMARY_METRIC_REQUIRED_FIELDS,
            ),
            required_secondary_metric_fields: required_field_list_to_owned(
                &STYLIZED_FACT_SECONDARY_METRIC_REQUIRED_FIELDS,
            ),
            required_uncertainty_fields: required_field_list_to_owned(
                &STYLIZED_FACT_UNCERTAINTY_REQUIRED_FIELDS,
            ),
            required_sample_sufficiency_fields: required_field_list_to_owned(
                &STYLIZED_FACT_SAMPLE_SUFFICIENCY_REQUIRED_FIELDS,
            ),
            required_estimator_metadata_fields: required_field_list_to_owned(
                &STYLIZED_FACT_ESTIMATOR_METADATA_REQUIRED_FIELDS,
            ),
            canonical_metric_class_vocabulary: required_field_list_to_owned(
                &STYLIZED_FACT_METRIC_CLASS_VOCABULARY,
            ),
            canonical_metric_family_vocabulary: required_field_list_to_owned(
                &STYLIZED_FACT_METRIC_FAMILY_VOCABULARY,
            ),
            canonical_domain_vocabulary: required_field_list_to_owned(
                &STYLIZED_FACT_DOMAIN_VOCABULARY,
            ),
            canonical_uncertainty_method_vocabulary: required_field_list_to_owned(
                &STYLIZED_FACT_UNCERTAINTY_METHOD_VOCABULARY,
            ),
        },
        metrics: vec![
            build_primary_metric(
                "activity_burst_duration_quantile_ratio_q90_q50",
                "Activity Burst Duration Quantile Ratio Q90 Q50",
                StylizedFactMetricFamily::QuantileShape,
                StylizedFactMetricDomain::ActivityClustering,
                "ratio of burst-duration 90th to 50th percentile",
                "ratio",
                StylizedFactMetricHorizonApplicability::AllConfiguredHorizons,
                StylizedFactSampleDependence::DependentEventSeries,
                StylizedFactMinimumSampleSufficiencyRequirements {
                    minimum_sample_count: 250,
                    minimum_effective_sample_count: 200,
                    minimum_tail_observation_count: None,
                    minimum_horizon_coverage_ratio: 0.8,
                },
                &["activity", "clustering", "quantile_shape"],
            ),
            build_primary_metric(
                "activity_interarrival_expected_shortfall_ratio",
                "Activity Interarrival Expected Shortfall Ratio",
                StylizedFactMetricFamily::TailRisk,
                StylizedFactMetricDomain::ActivityClustering,
                "ratio of interarrival expected shortfall to interarrival value at risk",
                "ratio",
                StylizedFactMetricHorizonApplicability::AllConfiguredHorizons,
                StylizedFactSampleDependence::DependentEventSeries,
                StylizedFactMinimumSampleSufficiencyRequirements {
                    minimum_sample_count: 300,
                    minimum_effective_sample_count: 220,
                    minimum_tail_observation_count: Some(35),
                    minimum_horizon_coverage_ratio: 0.8,
                },
                &["activity", "clustering", "tail_risk"],
            ),
            build_primary_metric(
                "returns_abs_quantile_shape_ratio_q99_q95",
                "Returns Abs Quantile Shape Ratio Q99 Q95",
                StylizedFactMetricFamily::QuantileShape,
                StylizedFactMetricDomain::HeavyTails,
                "ratio of absolute return 99th to 95th percentile",
                "ratio",
                StylizedFactMetricHorizonApplicability::AllConfiguredHorizons,
                StylizedFactSampleDependence::DependentReturnSeries,
                StylizedFactMinimumSampleSufficiencyRequirements {
                    minimum_sample_count: 500,
                    minimum_effective_sample_count: 350,
                    minimum_tail_observation_count: None,
                    minimum_horizon_coverage_ratio: 0.85,
                },
                &["returns", "heavy_tails", "quantile_shape"],
            ),
            build_primary_metric(
                "returns_left_tail_expected_shortfall_to_var_ratio",
                "Returns Left Tail Expected Shortfall To VaR Ratio",
                StylizedFactMetricFamily::TailRisk,
                StylizedFactMetricDomain::HeavyTails,
                "ratio of left-tail expected shortfall to left-tail value at risk",
                "ratio",
                StylizedFactMetricHorizonApplicability::AllConfiguredHorizons,
                StylizedFactSampleDependence::DependentReturnSeries,
                StylizedFactMinimumSampleSufficiencyRequirements {
                    minimum_sample_count: 500,
                    minimum_effective_sample_count: 350,
                    minimum_tail_observation_count: Some(40),
                    minimum_horizon_coverage_ratio: 0.85,
                },
                &["returns", "heavy_tails", "tail_risk"],
            ),
            build_primary_metric(
                "returns_distribution_wasserstein_distance",
                "Returns Distribution Wasserstein Distance",
                StylizedFactMetricFamily::DistributionDistance,
                StylizedFactMetricDomain::HeavyTails,
                "wasserstein distance between empirical and simulated return distributions",
                "return_units",
                StylizedFactMetricHorizonApplicability::AllConfiguredHorizons,
                StylizedFactSampleDependence::DependentReturnSeries,
                StylizedFactMinimumSampleSufficiencyRequirements {
                    minimum_sample_count: 450,
                    minimum_effective_sample_count: 325,
                    minimum_tail_observation_count: None,
                    minimum_horizon_coverage_ratio: 0.8,
                },
                &["returns", "heavy_tails", "distribution_distance"],
            ),
            build_primary_metric(
                "returns_bowley_skewness_quantile",
                "Returns Bowley Skewness Quantile",
                StylizedFactMetricFamily::QuantileShape,
                StylizedFactMetricDomain::Skew,
                "bowley quantile skewness using quartile spread asymmetry",
                "ratio",
                StylizedFactMetricHorizonApplicability::AllConfiguredHorizons,
                StylizedFactSampleDependence::DependentReturnSeries,
                StylizedFactMinimumSampleSufficiencyRequirements {
                    minimum_sample_count: 450,
                    minimum_effective_sample_count: 320,
                    minimum_tail_observation_count: None,
                    minimum_horizon_coverage_ratio: 0.8,
                },
                &["returns", "skew", "quantile_shape"],
            ),
            build_primary_metric(
                "returns_signed_tail_expected_shortfall_imbalance",
                "Returns Signed Tail Expected Shortfall Imbalance",
                StylizedFactMetricFamily::TailRisk,
                StylizedFactMetricDomain::Skew,
                "difference of right and left expected shortfall magnitudes normalized by value at risk",
                "ratio",
                StylizedFactMetricHorizonApplicability::AllConfiguredHorizons,
                StylizedFactSampleDependence::DependentReturnSeries,
                StylizedFactMinimumSampleSufficiencyRequirements {
                    minimum_sample_count: 500,
                    minimum_effective_sample_count: 360,
                    minimum_tail_observation_count: Some(45),
                    minimum_horizon_coverage_ratio: 0.85,
                },
                &["returns", "skew", "tail_risk"],
            ),
            build_primary_metric(
                "volatility_abs_return_acf_distribution_distance",
                "Volatility Abs Return ACF Distribution Distance",
                StylizedFactMetricFamily::DistributionDistance,
                StylizedFactMetricDomain::VolatilityPersistence,
                "distribution distance between lagged absolute-return autocorrelation profiles",
                "acf_units",
                StylizedFactMetricHorizonApplicability::HorizonAndLagGrid,
                StylizedFactSampleDependence::DependentReturnSeries,
                StylizedFactMinimumSampleSufficiencyRequirements {
                    minimum_sample_count: 450,
                    minimum_effective_sample_count: 320,
                    minimum_tail_observation_count: None,
                    minimum_horizon_coverage_ratio: 0.75,
                },
                &["volatility", "persistence", "distribution_distance"],
            ),
            build_primary_metric(
                "open_jump_abs_tail_expected_shortfall_ratio",
                "Open Jump Abs Tail Expected Shortfall Ratio",
                StylizedFactMetricFamily::TailRisk,
                StylizedFactMetricDomain::OpenWindowDirectJumpBehavior,
                "ratio of open-window direct-jump absolute expected shortfall to value at risk",
                "ratio",
                StylizedFactMetricHorizonApplicability::OpenWindowOnly,
                StylizedFactSampleDependence::DependentReturnSeries,
                StylizedFactMinimumSampleSufficiencyRequirements {
                    minimum_sample_count: 180,
                    minimum_effective_sample_count: 120,
                    minimum_tail_observation_count: Some(25),
                    minimum_horizon_coverage_ratio: 0.7,
                },
                &["open_jump", "tail_risk", "returns"],
            ),
            build_primary_metric(
                "closed_gap_abs_tail_expected_shortfall_ratio",
                "Closed Gap Abs Tail Expected Shortfall Ratio",
                StylizedFactMetricFamily::TailRisk,
                StylizedFactMetricDomain::ClosedWindowGapBehavior,
                "ratio of closed-window gap absolute expected shortfall to value at risk",
                "ratio",
                StylizedFactMetricHorizonApplicability::ClosedWindowOnly,
                StylizedFactSampleDependence::DependentReturnSeries,
                StylizedFactMinimumSampleSufficiencyRequirements {
                    minimum_sample_count: 120,
                    minimum_effective_sample_count: 90,
                    minimum_tail_observation_count: Some(20),
                    minimum_horizon_coverage_ratio: 0.6,
                },
                &["closed_gap", "tail_risk", "returns"],
            ),
            build_primary_metric(
                "closed_gap_vs_open_jump_wasserstein_distance",
                "Closed Gap Vs Open Jump Wasserstein Distance",
                StylizedFactMetricFamily::DistributionDistance,
                StylizedFactMetricDomain::ClosedWindowGapBehavior,
                "wasserstein distance between closed-gap and open-jump return-size distributions",
                "return_units",
                StylizedFactMetricHorizonApplicability::AllConfiguredHorizons,
                StylizedFactSampleDependence::DependentReturnSeries,
                StylizedFactMinimumSampleSufficiencyRequirements {
                    minimum_sample_count: 200,
                    minimum_effective_sample_count: 140,
                    minimum_tail_observation_count: None,
                    minimum_horizon_coverage_ratio: 0.65,
                },
                &["closed_gap", "open_jump", "distribution_distance"],
            ),
            build_secondary_metric(
                "returns_l_moment_lambda2",
                "Returns L Moment Lambda2",
                StylizedFactMetricFamily::LMoment,
                StylizedFactMetricDomain::HeavyTails,
                "second L-moment for return dispersion",
                "return_units",
                StylizedFactMetricHorizonApplicability::AllConfiguredHorizons,
                StylizedFactSampleDependence::DependentReturnSeries,
                StylizedFactMinimumSampleSufficiencyRequirements {
                    minimum_sample_count: 300,
                    minimum_effective_sample_count: 240,
                    minimum_tail_observation_count: None,
                    minimum_horizon_coverage_ratio: 0.8,
                },
                StylizedFactEstimatorMetadata {
                    estimator_id: "pwms_l_moment_estimator".to_string(),
                    estimator_version: "1.0.0".to_string(),
                    estimator_family: "l_moment".to_string(),
                    bias_correction: "none".to_string(),
                    finite_sample_adjustment: "pwm_unbiased_order_statistic_weights".to_string(),
                },
                StylizedFactSecondaryMomentVariant::LMoment {
                    moment_order: 2,
                    ratio_denominator_order: None,
                },
                &["returns", "l_moment", "dispersion"],
            ),
            build_secondary_metric(
                "returns_l_moment_tau3",
                "Returns L Moment Tau3",
                StylizedFactMetricFamily::LMoment,
                StylizedFactMetricDomain::Skew,
                "L-moment skewness ratio tau3 equals lambda3 over lambda2",
                "ratio",
                StylizedFactMetricHorizonApplicability::AllConfiguredHorizons,
                StylizedFactSampleDependence::DependentReturnSeries,
                StylizedFactMinimumSampleSufficiencyRequirements {
                    minimum_sample_count: 300,
                    minimum_effective_sample_count: 240,
                    minimum_tail_observation_count: None,
                    minimum_horizon_coverage_ratio: 0.8,
                },
                StylizedFactEstimatorMetadata {
                    estimator_id: "pwms_l_moment_estimator".to_string(),
                    estimator_version: "1.0.0".to_string(),
                    estimator_family: "l_moment".to_string(),
                    bias_correction: "none".to_string(),
                    finite_sample_adjustment: "pwm_unbiased_order_statistic_weights".to_string(),
                },
                StylizedFactSecondaryMomentVariant::LMoment {
                    moment_order: 3,
                    ratio_denominator_order: Some(2),
                },
                &["returns", "l_moment", "skew"],
            ),
            build_secondary_metric(
                "returns_central_variance",
                "Returns Central Variance",
                StylizedFactMetricFamily::CentralMoment,
                StylizedFactMetricDomain::HeavyTails,
                "second central moment of returns",
                "return_units_squared",
                StylizedFactMetricHorizonApplicability::AllConfiguredHorizons,
                StylizedFactSampleDependence::DependentReturnSeries,
                StylizedFactMinimumSampleSufficiencyRequirements {
                    minimum_sample_count: 300,
                    minimum_effective_sample_count: 240,
                    minimum_tail_observation_count: None,
                    minimum_horizon_coverage_ratio: 0.8,
                },
                StylizedFactEstimatorMetadata {
                    estimator_id: "central_moment_estimator".to_string(),
                    estimator_version: "1.0.0".to_string(),
                    estimator_family: "central_moment".to_string(),
                    bias_correction: "bessel_correction".to_string(),
                    finite_sample_adjustment: "divide_by_n_minus_one".to_string(),
                },
                StylizedFactSecondaryMomentVariant::CentralMoment {
                    moment_order: 2,
                    standardized: false,
                    excess_kurtosis: false,
                },
                &["returns", "central_moment", "variance"],
            ),
            build_secondary_metric(
                "returns_central_skewness",
                "Returns Central Skewness",
                StylizedFactMetricFamily::CentralMoment,
                StylizedFactMetricDomain::Skew,
                "third standardized central moment of returns",
                "ratio",
                StylizedFactMetricHorizonApplicability::AllConfiguredHorizons,
                StylizedFactSampleDependence::DependentReturnSeries,
                StylizedFactMinimumSampleSufficiencyRequirements {
                    minimum_sample_count: 350,
                    minimum_effective_sample_count: 260,
                    minimum_tail_observation_count: None,
                    minimum_horizon_coverage_ratio: 0.82,
                },
                StylizedFactEstimatorMetadata {
                    estimator_id: "fisher_pearson_skewness".to_string(),
                    estimator_version: "1.0.0".to_string(),
                    estimator_family: "central_moment".to_string(),
                    bias_correction: "fisher_pearson_adjustment".to_string(),
                    finite_sample_adjustment: "n_based_skewness_scaling".to_string(),
                },
                StylizedFactSecondaryMomentVariant::CentralMoment {
                    moment_order: 3,
                    standardized: true,
                    excess_kurtosis: false,
                },
                &["returns", "central_moment", "skew"],
            ),
            build_secondary_metric(
                "returns_central_excess_kurtosis",
                "Returns Central Excess Kurtosis",
                StylizedFactMetricFamily::CentralMoment,
                StylizedFactMetricDomain::HeavyTails,
                "fourth standardized central moment minus three",
                "ratio",
                StylizedFactMetricHorizonApplicability::AllConfiguredHorizons,
                StylizedFactSampleDependence::DependentReturnSeries,
                StylizedFactMinimumSampleSufficiencyRequirements {
                    minimum_sample_count: 350,
                    minimum_effective_sample_count: 260,
                    minimum_tail_observation_count: None,
                    minimum_horizon_coverage_ratio: 0.82,
                },
                StylizedFactEstimatorMetadata {
                    estimator_id: "fisher_pearson_kurtosis".to_string(),
                    estimator_version: "1.0.0".to_string(),
                    estimator_family: "central_moment".to_string(),
                    bias_correction: "fisher_pearson_adjustment".to_string(),
                    finite_sample_adjustment: "n_based_kurtosis_scaling".to_string(),
                },
                StylizedFactSecondaryMomentVariant::CentralMoment {
                    moment_order: 4,
                    standardized: true,
                    excess_kurtosis: true,
                },
                &["returns", "central_moment", "kurtosis"],
            ),
        ],
    };

    catalog.sort_metrics_canonically();

    debug_assert!(catalog.validate().is_ok());
    catalog
}

fn build_primary_metric(
    metric_id: &str,
    metric_name: &str,
    metric_family: StylizedFactMetricFamily,
    domain: StylizedFactMetricDomain,
    definition: &str,
    unit: &str,
    horizon_applicability: StylizedFactMetricHorizonApplicability,
    sample_dependence: StylizedFactSampleDependence,
    sample_sufficiency: StylizedFactMinimumSampleSufficiencyRequirements,
    tags: &[&str],
) -> StylizedFactMetricDefinition {
    StylizedFactMetricDefinition {
        metric_id: metric_id.to_string(),
        metric_name: metric_name.to_string(),
        metric_class: StylizedFactMetricClass::PrimaryRobust,
        metric_family,
        domain,
        definition: definition.to_string(),
        unit: unit.to_string(),
        horizon_applicability,
        sample_dependence,
        uncertainty: default_uncertainty_configuration(sample_dependence),
        sample_sufficiency,
        estimator_metadata: None,
        secondary_moment_variant: None,
        tags: tags.iter().map(|tag| (*tag).to_string()).collect(),
    }
}

#[allow(clippy::too_many_arguments)]
fn build_secondary_metric(
    metric_id: &str,
    metric_name: &str,
    metric_family: StylizedFactMetricFamily,
    domain: StylizedFactMetricDomain,
    definition: &str,
    unit: &str,
    horizon_applicability: StylizedFactMetricHorizonApplicability,
    sample_dependence: StylizedFactSampleDependence,
    sample_sufficiency: StylizedFactMinimumSampleSufficiencyRequirements,
    estimator_metadata: StylizedFactEstimatorMetadata,
    secondary_moment_variant: StylizedFactSecondaryMomentVariant,
    tags: &[&str],
) -> StylizedFactMetricDefinition {
    StylizedFactMetricDefinition {
        metric_id: metric_id.to_string(),
        metric_name: metric_name.to_string(),
        metric_class: StylizedFactMetricClass::SecondaryMoment,
        metric_family,
        domain,
        definition: definition.to_string(),
        unit: unit.to_string(),
        horizon_applicability,
        sample_dependence,
        uncertainty: default_uncertainty_configuration(sample_dependence),
        sample_sufficiency,
        estimator_metadata: Some(estimator_metadata),
        secondary_moment_variant: Some(secondary_moment_variant),
        tags: tags.iter().map(|tag| (*tag).to_string()).collect(),
    }
}

fn default_uncertainty_configuration(
    sample_dependence: StylizedFactSampleDependence,
) -> StylizedFactMetricUncertaintyConfiguration {
    match sample_dependence {
        StylizedFactSampleDependence::DependentReturnSeries
        | StylizedFactSampleDependence::DependentEventSeries => {
            StylizedFactMetricUncertaintyConfiguration {
                default_method: StylizedFactUncertaintyMethod::BlockBootstrap,
                confidence_level: 0.95,
                bootstrap_replication_count: 2000,
                block_length_horizon_count: 5,
                override_policy: StylizedFactUncertaintyOverridePolicy {
                    override_allowed: true,
                    approved_alternative_methods: vec![
                        StylizedFactUncertaintyMethod::StationaryBootstrap,
                        StylizedFactUncertaintyMethod::CircularBlockBootstrap,
                    ],
                },
            }
        }
        StylizedFactSampleDependence::ApproximatelyIndependentSamples => {
            StylizedFactMetricUncertaintyConfiguration {
                default_method: StylizedFactUncertaintyMethod::AnalyticalAsymptotic,
                confidence_level: 0.95,
                bootstrap_replication_count: 1000,
                block_length_horizon_count: 1,
                override_policy: StylizedFactUncertaintyOverridePolicy {
                    override_allowed: true,
                    approved_alternative_methods: vec![
                        StylizedFactUncertaintyMethod::BlockBootstrap,
                        StylizedFactUncertaintyMethod::StationaryBootstrap,
                    ],
                },
            }
        }
    }
}
