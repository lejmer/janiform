//! Request validation and metric-spec resolution for scale-consistency deltas.

use std::collections::BTreeSet;

use janiform_analytics_stylized_facts::{
    StylizedFactMetricCatalog, build_stylized_fact_metric_catalog,
};

use super::super::aggregation::SCALE_CONSISTENCY_FINE_PATH_ALIGNMENT_METADATA_SCHEMA_VERSION;
use super::contracts::{
    ScaleConsistencyMetricCategory, ScaleConsistencyMetricDeltaBatchError,
    ScaleConsistencyMetricDeltaBatchRequest, ScaleConsistencyMetricScenarioInput,
    ScaleConsistencyMetricScenarioInputError, ScaleConsistencyMetricSpecification,
    ScaleConsistencyMetricSpecificationError,
};

const RETURNS_CENTRAL_VARIANCE_METRIC_ID: &str = "returns_central_variance";
const RETURNS_DISTRIBUTION_WASSERSTEIN_DISTANCE_METRIC_ID: &str =
    "returns_distribution_wasserstein_distance";
const VOLATILITY_ABS_RETURN_ACF_DISTRIBUTION_DISTANCE_METRIC_ID: &str =
    "volatility_abs_return_acf_distribution_distance";
const OPEN_JUMP_ABS_TAIL_EXPECTED_SHORTFALL_RATIO_METRIC_ID: &str =
    "open_jump_abs_tail_expected_shortfall_ratio";
const CLOSED_GAP_ABS_TAIL_EXPECTED_SHORTFALL_RATIO_METRIC_ID: &str =
    "closed_gap_abs_tail_expected_shortfall_ratio";
const CLOSED_GAP_VS_OPEN_JUMP_WASSERSTEIN_DISTANCE_METRIC_ID: &str =
    "closed_gap_vs_open_jump_wasserstein_distance";

pub(crate) fn is_supported_scale_consistency_metric(metric_id: &str) -> bool {
    matches!(
        metric_id,
        RETURNS_CENTRAL_VARIANCE_METRIC_ID
            | RETURNS_DISTRIBUTION_WASSERSTEIN_DISTANCE_METRIC_ID
            | VOLATILITY_ABS_RETURN_ACF_DISTRIBUTION_DISTANCE_METRIC_ID
            | OPEN_JUMP_ABS_TAIL_EXPECTED_SHORTFALL_RATIO_METRIC_ID
            | CLOSED_GAP_ABS_TAIL_EXPECTED_SHORTFALL_RATIO_METRIC_ID
            | CLOSED_GAP_VS_OPEN_JUMP_WASSERSTEIN_DISTANCE_METRIC_ID
    )
}

#[derive(Debug, Clone)]
pub(super) struct ResolvedMetricSpecification {
    pub(super) metric_id: String,
    pub(super) metric_name: String,
    pub(super) metric_variant: Option<String>,
    pub(super) metric_category: ScaleConsistencyMetricCategory,
    pub(super) metric_kind: ResolvedMetricKind,
}

#[derive(Debug, Clone)]
pub(super) enum ResolvedMetricKind {
    ScalarDifference(ScalarMetricKind),
    PairwiseDistance(PairwiseMetricKind),
}

#[derive(Debug, Clone, Copy)]
pub(super) enum ScalarMetricKind {
    ReturnsCentralVariance,
    OpenJumpAbsTailExpectedShortfallRatio,
    ClosedGapAbsTailExpectedShortfallRatio,
    ClosedGapVsOpenJumpWassersteinDistance,
}

#[derive(Debug, Clone)]
pub(super) enum PairwiseMetricKind {
    ReturnsDistributionWassersteinDistance,
    VolatilityAbsReturnAcfDistributionDistance { lag_interval_counts: Vec<usize> },
}

pub(super) fn resolve_metric_specifications(
    request: &ScaleConsistencyMetricDeltaBatchRequest,
) -> Result<Vec<ResolvedMetricSpecification>, ScaleConsistencyMetricDeltaBatchError> {
    request.validate()
}

impl ScaleConsistencyMetricDeltaBatchRequest {
    fn validate(
        &self,
    ) -> Result<Vec<ResolvedMetricSpecification>, ScaleConsistencyMetricDeltaBatchError> {
        if self.metric_specifications.is_empty() {
            return Err(ScaleConsistencyMetricDeltaBatchError::MissingMetricSpecifications);
        }
        if self.scenarios.is_empty() {
            return Err(ScaleConsistencyMetricDeltaBatchError::MissingScenarios);
        }

        let metric_catalog = build_stylized_fact_metric_catalog();
        let mut observed_metric_keys: BTreeSet<(String, Option<String>)> = BTreeSet::new();
        let mut resolved_metric_specifications: Vec<ResolvedMetricSpecification> =
            Vec::with_capacity(self.metric_specifications.len());

        for (position, metric_specification) in self.metric_specifications.iter().enumerate() {
            let resolved_metric_specification = metric_specification
                .resolve_against_catalog(&metric_catalog)
                .map_err(|source| {
                    ScaleConsistencyMetricDeltaBatchError::InvalidMetricSpecification {
                        position,
                        source,
                    }
                })?;
            let metric_key = (
                resolved_metric_specification.metric_id.clone(),
                resolved_metric_specification.metric_variant.clone(),
            );
            if !observed_metric_keys.insert(metric_key.clone()) {
                return Err(
                    ScaleConsistencyMetricDeltaBatchError::DuplicateMetricSpecification {
                        metric_id: metric_key.0,
                        metric_variant: metric_key.1,
                    },
                );
            }
            resolved_metric_specifications.push(resolved_metric_specification);
        }

        let mut observed_scenario_ids: BTreeSet<&str> = BTreeSet::new();
        for scenario in &self.scenarios {
            scenario.validate().map_err(|source| {
                ScaleConsistencyMetricDeltaBatchError::InvalidScenario {
                    scenario_id: scenario.scenario_id.clone(),
                    source,
                }
            })?;
            if !observed_scenario_ids.insert(scenario.scenario_id.as_str()) {
                return Err(ScaleConsistencyMetricDeltaBatchError::DuplicateScenarioId {
                    scenario_id: scenario.scenario_id.clone(),
                });
            }
        }

        Ok(resolved_metric_specifications)
    }
}

impl ScaleConsistencyMetricSpecification {
    fn resolve_against_catalog(
        &self,
        metric_catalog: &StylizedFactMetricCatalog,
    ) -> Result<ResolvedMetricSpecification, ScaleConsistencyMetricSpecificationError> {
        if self.metric_id.trim().is_empty() {
            return Err(ScaleConsistencyMetricSpecificationError::MissingMetricId);
        }

        let metric_variant = self
            .metric_variant
            .as_ref()
            .map(|value| value.trim().to_string());
        if matches!(metric_variant.as_deref(), Some("")) {
            return Err(ScaleConsistencyMetricSpecificationError::EmptyMetricVariant);
        }

        let metric_definition = metric_catalog
            .metrics
            .iter()
            .find(|metric| metric.metric_id == self.metric_id)
            .ok_or_else(|| {
                ScaleConsistencyMetricSpecificationError::MetricNotPresentInStylizedFactCatalog {
                    metric_id: self.metric_id.clone(),
                }
            })?;

        let resolved_metric_kind = match self.metric_id.as_str() {
            RETURNS_CENTRAL_VARIANCE_METRIC_ID => {
                validate_absent_lag_configuration(self)?;
                ResolvedMetricKind::ScalarDifference(ScalarMetricKind::ReturnsCentralVariance)
            }
            RETURNS_DISTRIBUTION_WASSERSTEIN_DISTANCE_METRIC_ID => {
                validate_absent_lag_configuration(self)?;
                ResolvedMetricKind::PairwiseDistance(
                    PairwiseMetricKind::ReturnsDistributionWassersteinDistance,
                )
            }
            VOLATILITY_ABS_RETURN_ACF_DISTRIBUTION_DISTANCE_METRIC_ID => {
                let lag_interval_counts = validate_present_lag_configuration(self)?;
                ResolvedMetricKind::PairwiseDistance(
                    PairwiseMetricKind::VolatilityAbsReturnAcfDistributionDistance {
                        lag_interval_counts,
                    },
                )
            }
            OPEN_JUMP_ABS_TAIL_EXPECTED_SHORTFALL_RATIO_METRIC_ID => {
                validate_absent_lag_configuration(self)?;
                ResolvedMetricKind::ScalarDifference(
                    ScalarMetricKind::OpenJumpAbsTailExpectedShortfallRatio,
                )
            }
            CLOSED_GAP_ABS_TAIL_EXPECTED_SHORTFALL_RATIO_METRIC_ID => {
                validate_absent_lag_configuration(self)?;
                ResolvedMetricKind::ScalarDifference(
                    ScalarMetricKind::ClosedGapAbsTailExpectedShortfallRatio,
                )
            }
            CLOSED_GAP_VS_OPEN_JUMP_WASSERSTEIN_DISTANCE_METRIC_ID => {
                validate_absent_lag_configuration(self)?;
                ResolvedMetricKind::ScalarDifference(
                    ScalarMetricKind::ClosedGapVsOpenJumpWassersteinDistance,
                )
            }
            _ => {
                return Err(
                    ScaleConsistencyMetricSpecificationError::MetricUnsupportedByScaleConsistency {
                        metric_id: self.metric_id.clone(),
                    },
                );
            }
        };

        let resolved_metric_variant = match &resolved_metric_kind {
            ResolvedMetricKind::PairwiseDistance(
                PairwiseMetricKind::VolatilityAbsReturnAcfDistributionDistance {
                    lag_interval_counts,
                },
            ) => metric_variant.or_else(|| Some(format_lag_variant(lag_interval_counts))),
            _ => metric_variant,
        };

        Ok(ResolvedMetricSpecification {
            metric_id: metric_definition.metric_id.clone(),
            metric_name: metric_definition.metric_name.clone(),
            metric_variant: resolved_metric_variant,
            metric_category: match &resolved_metric_kind {
                ResolvedMetricKind::ScalarDifference(ScalarMetricKind::ReturnsCentralVariance)
                | ResolvedMetricKind::PairwiseDistance(
                    PairwiseMetricKind::ReturnsDistributionWassersteinDistance,
                ) => ScaleConsistencyMetricCategory::Returns,
                ResolvedMetricKind::PairwiseDistance(
                    PairwiseMetricKind::VolatilityAbsReturnAcfDistributionDistance { .. },
                ) => ScaleConsistencyMetricCategory::Volatility,
                ResolvedMetricKind::ScalarDifference(
                    ScalarMetricKind::OpenJumpAbsTailExpectedShortfallRatio,
                )
                | ResolvedMetricKind::ScalarDifference(
                    ScalarMetricKind::ClosedGapAbsTailExpectedShortfallRatio,
                )
                | ResolvedMetricKind::ScalarDifference(
                    ScalarMetricKind::ClosedGapVsOpenJumpWassersteinDistance,
                ) => ScaleConsistencyMetricCategory::Events,
            },
            metric_kind: resolved_metric_kind,
        })
    }
}

impl ScaleConsistencyMetricScenarioInput {
    fn validate(&self) -> Result<(), ScaleConsistencyMetricScenarioInputError> {
        if self.scenario_id.trim().is_empty() {
            return Err(ScaleConsistencyMetricScenarioInputError::MissingScenarioId);
        }
        if self.parameter_set_id.trim().is_empty() {
            return Err(ScaleConsistencyMetricScenarioInputError::MissingParameterSetId);
        }
        if self.horizon_label.trim().is_empty() {
            return Err(ScaleConsistencyMetricScenarioInputError::MissingHorizonLabel);
        }

        self.coarse_path.validate().map_err(|source| {
            ScaleConsistencyMetricScenarioInputError::InvalidCoarsePath { source }
        })?;

        self.aligned_fine_path
            .aggregated_path
            .validate()
            .map_err(
                |source| ScaleConsistencyMetricScenarioInputError::InvalidAlignedFinePath {
                    source,
                },
            )?;

        if self.aligned_fine_path.alignment_metadata.schema_version
            != SCALE_CONSISTENCY_FINE_PATH_ALIGNMENT_METADATA_SCHEMA_VERSION
        {
            return Err(
                ScaleConsistencyMetricScenarioInputError::InvalidAlignmentMetadataSchemaVersion {
                    actual: self
                        .aligned_fine_path
                        .alignment_metadata
                        .schema_version
                        .clone(),
                },
            );
        }

        if self.coarse_path.interval_delta != self.aligned_fine_path.aggregated_path.interval_delta
        {
            return Err(
                ScaleConsistencyMetricScenarioInputError::CoarseAndFineAggregatedIntervalDeltaMismatch {
                    coarse_interval_delta: self.coarse_path.interval_delta,
                    fine_aggregated_interval_delta: self.aligned_fine_path.aggregated_path.interval_delta,
                },
            );
        }

        if self.coarse_path.simulated_interval_count
            != self
                .aligned_fine_path
                .aggregated_path
                .simulated_interval_count
        {
            return Err(
                ScaleConsistencyMetricScenarioInputError::CoarseAndFineIntervalCountMismatch {
                    coarse_interval_count: self.coarse_path.simulated_interval_count,
                    fine_interval_count: self
                        .aligned_fine_path
                        .aggregated_path
                        .simulated_interval_count,
                },
            );
        }

        if self.coarse_path.interval_delta
            != self
                .aligned_fine_path
                .alignment_metadata
                .coarse_interval_delta
        {
            return Err(
                ScaleConsistencyMetricScenarioInputError::AlignmentMetadataCoarseDeltaMismatch {
                    coarse_interval_delta: self.coarse_path.interval_delta,
                    metadata_coarse_interval_delta: self
                        .aligned_fine_path
                        .alignment_metadata
                        .coarse_interval_delta,
                },
            );
        }

        if self.coarse_path.simulated_interval_count
            != self
                .aligned_fine_path
                .alignment_metadata
                .aggregated_coarse_interval_count
        {
            return Err(
                ScaleConsistencyMetricScenarioInputError::AlignmentMetadataIntervalCountMismatch {
                    coarse_interval_count: self.coarse_path.simulated_interval_count,
                    metadata_aggregated_interval_count: self
                        .aligned_fine_path
                        .alignment_metadata
                        .aggregated_coarse_interval_count,
                },
            );
        }

        Ok(())
    }
}

fn validate_absent_lag_configuration(
    metric_specification: &ScaleConsistencyMetricSpecification,
) -> Result<(), ScaleConsistencyMetricSpecificationError> {
    if metric_specification.lag_interval_counts.is_empty() {
        return Ok(());
    }

    Err(
        ScaleConsistencyMetricSpecificationError::UnexpectedLagIntervalCounts {
            metric_id: metric_specification.metric_id.clone(),
        },
    )
}

fn validate_present_lag_configuration(
    metric_specification: &ScaleConsistencyMetricSpecification,
) -> Result<Vec<usize>, ScaleConsistencyMetricSpecificationError> {
    if metric_specification.lag_interval_counts.is_empty() {
        return Err(
            ScaleConsistencyMetricSpecificationError::MissingLagIntervalCounts {
                metric_id: metric_specification.metric_id.clone(),
            },
        );
    }

    let mut observed_lag_counts: BTreeSet<usize> = BTreeSet::new();
    for lag_interval_count in &metric_specification.lag_interval_counts {
        if *lag_interval_count == 0 {
            return Err(
                ScaleConsistencyMetricSpecificationError::InvalidLagIntervalCount {
                    metric_id: metric_specification.metric_id.clone(),
                    value: *lag_interval_count,
                },
            );
        }

        if !observed_lag_counts.insert(*lag_interval_count) {
            return Err(
                ScaleConsistencyMetricSpecificationError::DuplicateLagIntervalCount {
                    metric_id: metric_specification.metric_id.clone(),
                    value: *lag_interval_count,
                },
            );
        }
    }

    Ok(metric_specification.lag_interval_counts.clone())
}

fn format_lag_variant(lag_interval_counts: &[usize]) -> String {
    let lag_suffix = lag_interval_counts
        .iter()
        .map(usize::to_string)
        .collect::<Vec<_>>()
        .join("_");
    format!("lag_grid_{lag_suffix}")
}
