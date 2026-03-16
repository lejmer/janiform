//! Model-level paired coarse-versus-fine scale-consistency run contracts.

use std::collections::BTreeSet;
use std::error::Error;
use std::fmt::{Display, Formatter};

use janiform_calibration_artifacts::CalibrationExecutionMode;
use janiform_engine_risk::{
    SufficientStatisticsRiskIntervalFactors, SufficientStatisticsRiskIntervalFactorsError,
    SufficientStatisticsRiskModelInputs, SufficientStatisticsRiskModelInputsError,
};
use janiform_foundations::randomness::{
    SimulationRandomnessSubstreamDomain, derive_simulation_substream_seed,
};
use serde::{Deserialize, Serialize};

use super::interval_grid::{PartitionedIntervalPair, PartitionedIntervalPairError};

pub const SCALE_CONSISTENCY_PAIRED_RUN_SPEC_SCHEMA_VERSION: &str =
    "validation.scale_consistency.paired_run_spec.v1";
pub const SCALE_CONSISTENCY_PAIRED_RUN_CONTRACT_SCHEMA_VERSION: &str =
    "validation.scale_consistency.paired_run_contract.v1";
pub const SCALE_CONSISTENCY_INTERVAL_RELATIONSHIP_POLICY: &str =
    "fine_interval_delta_must_strictly_partition_coarse_interval_delta_and_cover_full_horizon.v1";
pub const SCALE_CONSISTENCY_RANDOM_SEED_POLICY: &str =
    "explicit_seed_policy_required_with_shared_random_control_or_explicit_pair.v1";
pub const SCALE_CONSISTENCY_EXECUTION_MODE_POLICY: &str =
    "memory_transport_only_disk_optional_artifact_output.v1";
pub const SCALE_CONSISTENCY_RANDOM_CONTROL_METADATA_SCHEMA_VERSION: &str =
    "validation.scale_consistency.random_control_metadata.v1";
pub const SCALE_CONSISTENCY_RANDOM_CONTROL_SEED_RESOLUTION_POLICY: &str = "shared_random_control_derives_common_pair_seed_from_base_seed_via_simulation_substream_mixer_and_explicit_pair_passes_configured_seeds_directly.v1";
pub const SCALE_CONSISTENCY_VARIANCE_REDUCTION_POLICY: &str = "shared_random_control_reuses_common_pair_seed_across_coarse_and_fine_and_explicit_pair_disables_cross_run_variance_reduction.v1";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScaleConsistencyArtifactOutputConfig {
    pub artifact_root_uri: String,
    pub file_stem: String,
}

impl ScaleConsistencyArtifactOutputConfig {
    pub fn validate(&self) -> Result<(), ScaleConsistencyArtifactOutputConfigError> {
        if self.artifact_root_uri.trim().is_empty() {
            return Err(ScaleConsistencyArtifactOutputConfigError::EmptyArtifactRootUri);
        }
        if self.file_stem.trim().is_empty() {
            return Err(ScaleConsistencyArtifactOutputConfigError::EmptyFileStem);
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ScaleConsistencyArtifactOutputConfigError {
    EmptyArtifactRootUri,
    EmptyFileStem,
}

impl Display for ScaleConsistencyArtifactOutputConfigError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyArtifactRootUri => {
                write!(
                    formatter,
                    "artifact_output.artifact_root_uri must be a non-empty string"
                )
            }
            Self::EmptyFileStem => {
                write!(
                    formatter,
                    "artifact_output.file_stem must be a non-empty string"
                )
            }
        }
    }
}

impl Error for ScaleConsistencyArtifactOutputConfigError {}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScaleConsistencyExperimentParameterSet {
    pub parameter_set_id: String,
    pub model_inputs: SufficientStatisticsRiskModelInputs,
    pub fine_interval_factors: SufficientStatisticsRiskIntervalFactors,
}

impl ScaleConsistencyExperimentParameterSet {
    pub fn validate(&self) -> Result<(), ScaleConsistencyExperimentParameterSetError> {
        if self.parameter_set_id.trim().is_empty() {
            return Err(ScaleConsistencyExperimentParameterSetError::MissingParameterSetId);
        }

        self.model_inputs.validate().map_err(|source| {
            ScaleConsistencyExperimentParameterSetError::InvalidModelInputs { source }
        })?;

        self.fine_interval_factors.validate().map_err(|source| {
            ScaleConsistencyExperimentParameterSetError::InvalidFineIntervalFactors { source }
        })?;

        if self.fine_interval_factors.interval_delta.is_none() {
            return Err(
                ScaleConsistencyExperimentParameterSetError::MissingFineIntervalFactorDelta,
            );
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ScaleConsistencyExperimentParameterSetError {
    MissingParameterSetId,
    InvalidModelInputs {
        source: SufficientStatisticsRiskModelInputsError,
    },
    InvalidFineIntervalFactors {
        source: SufficientStatisticsRiskIntervalFactorsError,
    },
    MissingFineIntervalFactorDelta,
}

impl Display for ScaleConsistencyExperimentParameterSetError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingParameterSetId => {
                write!(formatter, "parameter_set_id must be a non-empty string")
            }
            Self::InvalidModelInputs { source } => {
                write!(formatter, "model_inputs payload is invalid: {source}")
            }
            Self::InvalidFineIntervalFactors { source } => {
                write!(
                    formatter,
                    "fine_interval_factors payload is invalid: {source}"
                )
            }
            Self::MissingFineIntervalFactorDelta => write!(
                formatter,
                "fine_interval_factors.interval_delta must be provided explicitly to avoid hidden coarse or fine mismatches"
            ),
        }
    }
}

impl Error for ScaleConsistencyExperimentParameterSetError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidModelInputs { source } => Some(source),
            Self::InvalidFineIntervalFactors { source } => Some(source),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScaleConsistencyRandomSeedPolicyKind {
    SharedRandomControl,
    ExplicitPair,
}

/// Variance-reduction policy implied by one paired-run seed configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScaleConsistencyVarianceReductionPolicy {
    CommonRandomNumbers,
    IndependentStreams,
}

/// Provenance for one resolved coarse or fine seed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScaleConsistencyResolvedSeedSource {
    DerivedFromSharedRandomControl,
    ExplicitConfiguredSeed,
}

/// Machine-readable random-control metadata resolved for one paired run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScaleConsistencyRandomControlMetadata {
    pub schema_version: String,
    pub random_seed_policy_kind: ScaleConsistencyRandomSeedPolicyKind,
    pub variance_reduction_policy: ScaleConsistencyVarianceReductionPolicy,
    pub seed_resolution_policy: String,
    pub paired_run_index: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shared_random_control_base_seed: Option<u64>,
    pub coarse_seed: u64,
    pub fine_seed: u64,
    pub coarse_seed_source: ScaleConsistencyResolvedSeedSource,
    pub fine_seed_source: ScaleConsistencyResolvedSeedSource,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ScaleConsistencyRandomSeedPolicy {
    SharedRandomControl { base_seed: u64 },
    ExplicitPair { coarse_seed: u64, fine_seed: u64 },
}

impl ScaleConsistencyRandomSeedPolicy {
    pub const fn kind(&self) -> ScaleConsistencyRandomSeedPolicyKind {
        match self {
            Self::SharedRandomControl { .. } => {
                ScaleConsistencyRandomSeedPolicyKind::SharedRandomControl
            }
            Self::ExplicitPair { .. } => ScaleConsistencyRandomSeedPolicyKind::ExplicitPair,
        }
    }

    pub const fn variance_reduction_policy(&self) -> ScaleConsistencyVarianceReductionPolicy {
        match self {
            Self::SharedRandomControl { .. } => {
                ScaleConsistencyVarianceReductionPolicy::CommonRandomNumbers
            }
            Self::ExplicitPair { .. } => {
                ScaleConsistencyVarianceReductionPolicy::IndependentStreams
            }
        }
    }

    pub fn validate(&self) -> Result<(), ScaleConsistencyRandomSeedPolicyError> {
        if let Self::ExplicitPair {
            coarse_seed,
            fine_seed,
        } = self
            && coarse_seed == fine_seed
        {
            return Err(
                ScaleConsistencyRandomSeedPolicyError::DuplicateExplicitPairSeeds {
                    coarse_seed: *coarse_seed,
                },
            );
        }

        Ok(())
    }

    pub fn resolve_random_control_metadata(
        &self,
        paired_run_index: u64,
    ) -> ScaleConsistencyRandomControlMetadata {
        match self {
            Self::SharedRandomControl { base_seed } => {
                let shared_pair_seed = derive_simulation_substream_seed(
                    *base_seed,
                    SimulationRandomnessSubstreamDomain::ScaleConsistencyPairedExperiment,
                    paired_run_index,
                );

                ScaleConsistencyRandomControlMetadata {
                    schema_version: SCALE_CONSISTENCY_RANDOM_CONTROL_METADATA_SCHEMA_VERSION
                        .to_string(),
                    random_seed_policy_kind: self.kind(),
                    variance_reduction_policy: self.variance_reduction_policy(),
                    seed_resolution_policy: SCALE_CONSISTENCY_RANDOM_CONTROL_SEED_RESOLUTION_POLICY
                        .to_string(),
                    paired_run_index,
                    shared_random_control_base_seed: Some(*base_seed),
                    coarse_seed: shared_pair_seed,
                    fine_seed: shared_pair_seed,
                    coarse_seed_source:
                        ScaleConsistencyResolvedSeedSource::DerivedFromSharedRandomControl,
                    fine_seed_source:
                        ScaleConsistencyResolvedSeedSource::DerivedFromSharedRandomControl,
                }
            }
            Self::ExplicitPair {
                coarse_seed,
                fine_seed,
            } => ScaleConsistencyRandomControlMetadata {
                schema_version: SCALE_CONSISTENCY_RANDOM_CONTROL_METADATA_SCHEMA_VERSION
                    .to_string(),
                random_seed_policy_kind: self.kind(),
                variance_reduction_policy: self.variance_reduction_policy(),
                seed_resolution_policy: SCALE_CONSISTENCY_RANDOM_CONTROL_SEED_RESOLUTION_POLICY
                    .to_string(),
                paired_run_index,
                shared_random_control_base_seed: None,
                coarse_seed: *coarse_seed,
                fine_seed: *fine_seed,
                coarse_seed_source: ScaleConsistencyResolvedSeedSource::ExplicitConfiguredSeed,
                fine_seed_source: ScaleConsistencyResolvedSeedSource::ExplicitConfiguredSeed,
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ScaleConsistencyRandomSeedPolicyError {
    DuplicateExplicitPairSeeds { coarse_seed: u64 },
}

impl Display for ScaleConsistencyRandomSeedPolicyError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DuplicateExplicitPairSeeds { coarse_seed } => write!(
                formatter,
                "random_seed_policy explicit_pair must use distinct coarse_seed and fine_seed values, found duplicate {coarse_seed}"
            ),
        }
    }
}

impl Error for ScaleConsistencyRandomSeedPolicyError {}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScaleConsistencyPairedExperimentScenarioSpec {
    pub scenario_id: String,
    pub coarse_interval_delta: f64,
    pub fine_interval_delta: f64,
    pub horizon_interval_count: usize,
    pub parameter_set: ScaleConsistencyExperimentParameterSet,
    pub random_seed_policy: ScaleConsistencyRandomSeedPolicy,
}

impl ScaleConsistencyPairedExperimentScenarioSpec {
    pub fn validate(&self) -> Result<(), ScaleConsistencyPairedExperimentScenarioSpecError> {
        if self.scenario_id.trim().is_empty() {
            return Err(ScaleConsistencyPairedExperimentScenarioSpecError::MissingScenarioId);
        }
        if !self.coarse_interval_delta.is_finite() || self.coarse_interval_delta <= 0.0 {
            return Err(
                ScaleConsistencyPairedExperimentScenarioSpecError::InvalidCoarseIntervalDelta {
                    value: self.coarse_interval_delta,
                },
            );
        }
        if !self.fine_interval_delta.is_finite() || self.fine_interval_delta <= 0.0 {
            return Err(
                ScaleConsistencyPairedExperimentScenarioSpecError::InvalidFineIntervalDelta {
                    value: self.fine_interval_delta,
                },
            );
        }
        if self.fine_interval_delta >= self.coarse_interval_delta {
            return Err(
                ScaleConsistencyPairedExperimentScenarioSpecError::FineIntervalMustBeStrictlySmaller {
                    coarse_interval_delta: self.coarse_interval_delta,
                    fine_interval_delta: self.fine_interval_delta,
                },
            );
        }
        if self.horizon_interval_count == 0 {
            return Err(
                ScaleConsistencyPairedExperimentScenarioSpecError::InvalidHorizonIntervalCount {
                    value: self.horizon_interval_count,
                },
            );
        }

        self.parameter_set.validate().map_err(|source| {
            ScaleConsistencyPairedExperimentScenarioSpecError::InvalidParameterSet { source }
        })?;
        self.random_seed_policy.validate().map_err(|source| {
            ScaleConsistencyPairedExperimentScenarioSpecError::InvalidRandomSeedPolicy { source }
        })?;

        let fine_subinterval_count_per_coarse_interval =
            validate_interval_pair(self.coarse_interval_delta, self.fine_interval_delta)?;

        let available_factor_interval_delta = self
            .parameter_set
            .fine_interval_factors
            .interval_delta
            .expect("parameter set validation requires explicit fine interval factor delta");
        if available_factor_interval_delta != self.fine_interval_delta {
            return Err(
                ScaleConsistencyPairedExperimentScenarioSpecError::FineIntervalFactorDeltaMismatch {
                    expected_fine_interval_delta: self.fine_interval_delta,
                    available_factor_interval_delta,
                },
            );
        }

        let required_fine_interval_count = self
            .horizon_interval_count
            .checked_mul(fine_subinterval_count_per_coarse_interval)
            .ok_or(
                ScaleConsistencyPairedExperimentScenarioSpecError::FineIntervalCountOverflow {
                    coarse_horizon_interval_count: self.horizon_interval_count,
                    fine_subinterval_count_per_coarse_interval,
                },
            )?;

        let available_fine_interval_count = self.parameter_set.fine_interval_factors.s_h.len();
        if required_fine_interval_count > available_fine_interval_count {
            return Err(
                ScaleConsistencyPairedExperimentScenarioSpecError::HorizonExceedsAvailableFineIntervals {
                    coarse_horizon_interval_count: self.horizon_interval_count,
                    required_fine_interval_count,
                    available_fine_interval_count,
                },
            );
        }

        Ok(())
    }

    pub fn fine_subinterval_count_per_coarse_interval(
        &self,
    ) -> Result<usize, ScaleConsistencyPairedExperimentScenarioSpecError> {
        validate_interval_pair(self.coarse_interval_delta, self.fine_interval_delta)
    }

    pub fn resolve_random_control_metadata(
        &self,
        paired_run_index: u64,
    ) -> ScaleConsistencyRandomControlMetadata {
        self.random_seed_policy
            .resolve_random_control_metadata(paired_run_index)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ScaleConsistencyPairedExperimentScenarioSpecError {
    MissingScenarioId,
    InvalidCoarseIntervalDelta {
        value: f64,
    },
    InvalidFineIntervalDelta {
        value: f64,
    },
    FineIntervalMustBeStrictlySmaller {
        coarse_interval_delta: f64,
        fine_interval_delta: f64,
    },
    IntervalDeltaRatioMismatch {
        coarse_interval_delta: f64,
        fine_interval_delta: f64,
        implied_ratio: f64,
    },
    InvalidHorizonIntervalCount {
        value: usize,
    },
    InvalidParameterSet {
        source: ScaleConsistencyExperimentParameterSetError,
    },
    InvalidRandomSeedPolicy {
        source: ScaleConsistencyRandomSeedPolicyError,
    },
    FineIntervalFactorDeltaMismatch {
        expected_fine_interval_delta: f64,
        available_factor_interval_delta: f64,
    },
    FineIntervalCountOverflow {
        coarse_horizon_interval_count: usize,
        fine_subinterval_count_per_coarse_interval: usize,
    },
    HorizonExceedsAvailableFineIntervals {
        coarse_horizon_interval_count: usize,
        required_fine_interval_count: usize,
        available_fine_interval_count: usize,
    },
}

impl Display for ScaleConsistencyPairedExperimentScenarioSpecError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingScenarioId => {
                write!(formatter, "scenario_id must be a non-empty string")
            }
            Self::InvalidCoarseIntervalDelta { value } => write!(
                formatter,
                "coarse_interval_delta must be finite and strictly positive, found {value}"
            ),
            Self::InvalidFineIntervalDelta { value } => write!(
                formatter,
                "fine_interval_delta must be finite and strictly positive, found {value}"
            ),
            Self::FineIntervalMustBeStrictlySmaller {
                coarse_interval_delta,
                fine_interval_delta,
            } => write!(
                formatter,
                "fine_interval_delta must be strictly smaller than coarse_interval_delta, found coarse_interval_delta={coarse_interval_delta} and fine_interval_delta={fine_interval_delta}"
            ),
            Self::IntervalDeltaRatioMismatch {
                coarse_interval_delta,
                fine_interval_delta,
                implied_ratio,
            } => write!(
                formatter,
                "coarse_interval_delta must be an exact integer multiple of fine_interval_delta, found coarse_interval_delta={coarse_interval_delta}, fine_interval_delta={fine_interval_delta}, implied_ratio={implied_ratio}"
            ),
            Self::InvalidHorizonIntervalCount { value } => write!(
                formatter,
                "horizon_interval_count must be greater than 0, found {value}"
            ),
            Self::InvalidParameterSet { source } => {
                write!(formatter, "parameter_set payload is invalid: {source}")
            }
            Self::InvalidRandomSeedPolicy { source } => {
                write!(formatter, "random_seed_policy payload is invalid: {source}")
            }
            Self::FineIntervalFactorDeltaMismatch {
                expected_fine_interval_delta,
                available_factor_interval_delta,
            } => write!(
                formatter,
                "parameter_set.fine_interval_factors.interval_delta must match fine_interval_delta exactly, found fine_interval_delta={expected_fine_interval_delta} and fine_interval_factors.interval_delta={available_factor_interval_delta}"
            ),
            Self::FineIntervalCountOverflow {
                coarse_horizon_interval_count,
                fine_subinterval_count_per_coarse_interval,
            } => write!(
                formatter,
                "coarse horizon and fine interval ratio overflowed required fine interval count, found horizon_interval_count={coarse_horizon_interval_count} and fine_subinterval_count_per_coarse_interval={fine_subinterval_count_per_coarse_interval}"
            ),
            Self::HorizonExceedsAvailableFineIntervals {
                coarse_horizon_interval_count,
                required_fine_interval_count,
                available_fine_interval_count,
            } => write!(
                formatter,
                "scenario requires {required_fine_interval_count} fine intervals for horizon_interval_count={coarse_horizon_interval_count}, but parameter_set.fine_interval_factors provides only {available_fine_interval_count}"
            ),
        }
    }
}

impl Error for ScaleConsistencyPairedExperimentScenarioSpecError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidParameterSet { source } => Some(source),
            Self::InvalidRandomSeedPolicy { source } => Some(source),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScaleConsistencyPairedRunSpec {
    pub run_id: String,
    pub execution_mode: CalibrationExecutionMode,
    pub scenarios: Vec<ScaleConsistencyPairedExperimentScenarioSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_output: Option<ScaleConsistencyArtifactOutputConfig>,
}

impl ScaleConsistencyPairedRunSpec {
    pub fn validate(&self) -> Result<(), ScaleConsistencyPairedRunSpecError> {
        if self.run_id.trim().is_empty() {
            return Err(ScaleConsistencyPairedRunSpecError::MissingRunId);
        }
        if self.scenarios.is_empty() {
            return Err(ScaleConsistencyPairedRunSpecError::MissingScenarios);
        }

        let mut observed_scenario_ids: BTreeSet<&str> = BTreeSet::new();
        for scenario in &self.scenarios {
            scenario.validate().map_err(|source| {
                ScaleConsistencyPairedRunSpecError::InvalidScenario {
                    scenario_id: scenario.scenario_id.clone(),
                    source,
                }
            })?;

            if !observed_scenario_ids.insert(scenario.scenario_id.as_str()) {
                return Err(ScaleConsistencyPairedRunSpecError::DuplicateScenarioId {
                    scenario_id: scenario.scenario_id.clone(),
                });
            }
        }

        if let Some(artifact_output) = &self.artifact_output {
            artifact_output.validate().map_err(|source| {
                ScaleConsistencyPairedRunSpecError::InvalidArtifactOutput { source }
            })?;

            if self.execution_mode != CalibrationExecutionMode::Disk {
                return Err(
                    ScaleConsistencyPairedRunSpecError::ArtifactOutputRequiresDiskMode {
                        execution_mode: self.execution_mode,
                    },
                );
            }
        }

        Ok(())
    }

    pub const fn schema_version() -> &'static str {
        SCALE_CONSISTENCY_PAIRED_RUN_SPEC_SCHEMA_VERSION
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ScaleConsistencyPairedRunSpecError {
    MissingRunId,
    MissingScenarios,
    DuplicateScenarioId {
        scenario_id: String,
    },
    InvalidScenario {
        scenario_id: String,
        source: ScaleConsistencyPairedExperimentScenarioSpecError,
    },
    InvalidArtifactOutput {
        source: ScaleConsistencyArtifactOutputConfigError,
    },
    ArtifactOutputRequiresDiskMode {
        execution_mode: CalibrationExecutionMode,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScaleConsistencyPairedRunSpecErrorClassification {
    InvalidInput,
    ModeIncompatibleOptions,
}

impl ScaleConsistencyPairedRunSpecError {
    pub const fn classification(&self) -> ScaleConsistencyPairedRunSpecErrorClassification {
        match self {
            Self::ArtifactOutputRequiresDiskMode { .. } => {
                ScaleConsistencyPairedRunSpecErrorClassification::ModeIncompatibleOptions
            }
            _ => ScaleConsistencyPairedRunSpecErrorClassification::InvalidInput,
        }
    }
}

impl Display for ScaleConsistencyPairedRunSpecError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingRunId => write!(formatter, "run_id must be a non-empty string"),
            Self::MissingScenarios => {
                write!(
                    formatter,
                    "scenarios must include at least one paired experiment"
                )
            }
            Self::DuplicateScenarioId { scenario_id } => write!(
                formatter,
                "scenarios must use unique scenario_id values, found duplicate `{scenario_id}`"
            ),
            Self::InvalidScenario {
                scenario_id,
                source,
            } => write!(formatter, "scenario `{scenario_id}` is invalid: {source}"),
            Self::InvalidArtifactOutput { source } => {
                write!(formatter, "artifact_output payload is invalid: {source}")
            }
            Self::ArtifactOutputRequiresDiskMode { execution_mode } => write!(
                formatter,
                "artifact_output requires execution_mode=`disk`, found execution_mode=`{execution_mode:?}`"
            ),
        }
    }
}

impl Error for ScaleConsistencyPairedRunSpecError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidScenario { source, .. } => Some(source),
            Self::InvalidArtifactOutput { source } => Some(source),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScaleConsistencyPairedRunContract {
    pub schema_version: String,
    pub run_spec_schema_version: String,
    pub interval_relationship_policy: String,
    pub random_seed_policy: String,
    pub execution_mode_policy: String,
    pub run_spec: ScaleConsistencyPairedRunSpec,
}

impl ScaleConsistencyPairedRunContract {
    pub fn try_new(
        run_spec: ScaleConsistencyPairedRunSpec,
    ) -> Result<Self, ScaleConsistencyPairedRunContractError> {
        run_spec
            .validate()
            .map_err(|source| ScaleConsistencyPairedRunContractError::InvalidRunSpec { source })?;

        Ok(Self {
            schema_version: SCALE_CONSISTENCY_PAIRED_RUN_CONTRACT_SCHEMA_VERSION.to_string(),
            run_spec_schema_version: SCALE_CONSISTENCY_PAIRED_RUN_SPEC_SCHEMA_VERSION.to_string(),
            interval_relationship_policy: SCALE_CONSISTENCY_INTERVAL_RELATIONSHIP_POLICY
                .to_string(),
            random_seed_policy: SCALE_CONSISTENCY_RANDOM_SEED_POLICY.to_string(),
            execution_mode_policy: SCALE_CONSISTENCY_EXECUTION_MODE_POLICY.to_string(),
            run_spec,
        })
    }

    pub const fn schema_version() -> &'static str {
        SCALE_CONSISTENCY_PAIRED_RUN_CONTRACT_SCHEMA_VERSION
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ScaleConsistencyPairedRunContractError {
    InvalidRunSpec {
        source: ScaleConsistencyPairedRunSpecError,
    },
}

impl Display for ScaleConsistencyPairedRunContractError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidRunSpec { source } => {
                write!(formatter, "run_spec is invalid: {source}")
            }
        }
    }
}

impl Error for ScaleConsistencyPairedRunContractError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidRunSpec { source } => Some(source),
        }
    }
}

fn validate_interval_pair(
    coarse_interval_delta: f64,
    fine_interval_delta: f64,
) -> Result<usize, ScaleConsistencyPairedExperimentScenarioSpecError> {
    PartitionedIntervalPair::try_new(coarse_interval_delta, fine_interval_delta)
        .map(|pair| pair.fine_subinterval_count_per_coarse_interval)
        .map_err(map_interval_pair_error)
}

fn map_interval_pair_error(
    source: PartitionedIntervalPairError,
) -> ScaleConsistencyPairedExperimentScenarioSpecError {
    match source {
        PartitionedIntervalPairError::InvalidCoarseIntervalDelta { value } => {
            ScaleConsistencyPairedExperimentScenarioSpecError::InvalidCoarseIntervalDelta { value }
        }
        PartitionedIntervalPairError::InvalidFineIntervalDelta { value } => {
            ScaleConsistencyPairedExperimentScenarioSpecError::InvalidFineIntervalDelta { value }
        }
        PartitionedIntervalPairError::FineIntervalMustBeStrictlySmaller {
            coarse_interval_delta,
            fine_interval_delta,
        } => ScaleConsistencyPairedExperimentScenarioSpecError::FineIntervalMustBeStrictlySmaller {
            coarse_interval_delta,
            fine_interval_delta,
        },
        PartitionedIntervalPairError::IntervalDeltaRatioMismatch {
            coarse_interval_delta,
            fine_interval_delta,
            implied_ratio,
        } => ScaleConsistencyPairedExperimentScenarioSpecError::IntervalDeltaRatioMismatch {
            coarse_interval_delta,
            fine_interval_delta,
            implied_ratio,
        },
    }
}
