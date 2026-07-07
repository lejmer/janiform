use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt::{Display, Formatter};

use serde::{Deserialize, Serialize};

use super::objective::{
    CALIBRATION_CONSTRAINED_OPTIMIZER_ADAPTER_BOUNDS_POLICY,
    CALIBRATION_CONSTRAINED_OPTIMIZER_ADAPTER_PENALTY_POLICY,
    CALIBRATION_CONSTRAINED_OPTIMIZER_ADAPTER_SELECTION_POLICY,
    CalibrationObjectiveCompositionError, CalibrationObjectiveCompositionReport,
    CalibrationObjectiveTermInput, compose_calibration_weighted_objective,
};
use janiform_calibration_artifacts::{
    CALIBRATION_CONSTRAINED_OPTIMIZER_ADAPTER_RUN_SCHEMA_VERSION, CalibrationOptimizerSettings,
    CalibrationOptimizerSettingsError,
};
use janiform_foundations::splitmix64_output_mixer;
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CalibrationOptimizerParameterBound {
    pub parameter_name: String,
    pub lower_inclusive: f64,
    pub upper_inclusive: f64,
}

impl CalibrationOptimizerParameterBound {
    pub fn try_new(
        parameter_name: impl Into<String>,
        lower_inclusive: f64,
        upper_inclusive: f64,
    ) -> Result<Self, CalibrationOptimizerParameterBoundError> {
        let bound = Self {
            parameter_name: parameter_name.into(),
            lower_inclusive,
            upper_inclusive,
        };
        bound.validate()?;
        Ok(bound)
    }

    fn validate(&self) -> Result<(), CalibrationOptimizerParameterBoundError> {
        if self.parameter_name.trim().is_empty() {
            return Err(CalibrationOptimizerParameterBoundError::MissingParameterName);
        }
        if !self.lower_inclusive.is_finite() {
            return Err(CalibrationOptimizerParameterBoundError::InvalidLowerBound {
                value: self.lower_inclusive,
            });
        }
        if !self.upper_inclusive.is_finite() {
            return Err(CalibrationOptimizerParameterBoundError::InvalidUpperBound {
                value: self.upper_inclusive,
            });
        }
        if self.lower_inclusive > self.upper_inclusive {
            return Err(CalibrationOptimizerParameterBoundError::InvertedBounds {
                lower_inclusive: self.lower_inclusive,
                upper_inclusive: self.upper_inclusive,
            });
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum CalibrationOptimizerParameterBoundError {
    MissingParameterName,
    InvalidLowerBound {
        value: f64,
    },
    InvalidUpperBound {
        value: f64,
    },
    InvertedBounds {
        lower_inclusive: f64,
        upper_inclusive: f64,
    },
}

impl Display for CalibrationOptimizerParameterBoundError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingParameterName => {
                write!(
                    formatter,
                    "optimizer bound parameter_name must be non-empty"
                )
            }
            Self::InvalidLowerBound { value } => write!(
                formatter,
                "optimizer bound lower_inclusive must be finite, found {value}"
            ),
            Self::InvalidUpperBound { value } => write!(
                formatter,
                "optimizer bound upper_inclusive must be finite, found {value}"
            ),
            Self::InvertedBounds {
                lower_inclusive,
                upper_inclusive,
            } => write!(
                formatter,
                "optimizer bound lower_inclusive ({lower_inclusive}) must be <= upper_inclusive ({upper_inclusive})"
            ),
        }
    }
}

impl Error for CalibrationOptimizerParameterBoundError {}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CalibrationOptimizerAdapterConfiguration {
    pub backend_id: String,
    pub random_seed: u64,
    pub solver_settings: CalibrationOptimizerSettings,
    #[serde(default)]
    pub parameter_bounds: Vec<CalibrationOptimizerParameterBound>,
}

impl CalibrationOptimizerAdapterConfiguration {
    pub fn try_new(
        backend_id: impl Into<String>,
        random_seed: u64,
        solver_settings: CalibrationOptimizerSettings,
        parameter_bounds: Vec<CalibrationOptimizerParameterBound>,
    ) -> Result<Self, CalibrationOptimizerAdapterConfigurationError> {
        let configuration = Self {
            backend_id: backend_id.into(),
            random_seed,
            solver_settings,
            parameter_bounds,
        };
        configuration.validate()?;
        Ok(configuration)
    }

    fn validate(&self) -> Result<(), CalibrationOptimizerAdapterConfigurationError> {
        if self.backend_id.trim().is_empty() {
            return Err(CalibrationOptimizerAdapterConfigurationError::MissingBackendId);
        }

        self.solver_settings.validate().map_err(|source| {
            CalibrationOptimizerAdapterConfigurationError::InvalidSolverSettings { source }
        })?;

        let mut seen_parameter_names = BTreeSet::new();
        for bound in &self.parameter_bounds {
            bound.validate().map_err(|source| {
                CalibrationOptimizerAdapterConfigurationError::InvalidParameterBound {
                    parameter_name: bound.parameter_name.clone(),
                    source,
                }
            })?;
            if !seen_parameter_names.insert(bound.parameter_name.as_str()) {
                return Err(
                    CalibrationOptimizerAdapterConfigurationError::DuplicateParameterBound {
                        parameter_name: bound.parameter_name.clone(),
                    },
                );
            }
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum CalibrationOptimizerAdapterConfigurationError {
    MissingBackendId,
    InvalidSolverSettings {
        source: CalibrationOptimizerSettingsError,
    },
    InvalidParameterBound {
        parameter_name: String,
        source: CalibrationOptimizerParameterBoundError,
    },
    DuplicateParameterBound {
        parameter_name: String,
    },
}

impl Display for CalibrationOptimizerAdapterConfigurationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingBackendId => {
                write!(formatter, "optimizer adapter backend_id must be non-empty")
            }
            Self::InvalidSolverSettings { source } => {
                write!(
                    formatter,
                    "optimizer adapter solver_settings are invalid: {source}"
                )
            }
            Self::InvalidParameterBound {
                parameter_name,
                source,
            } => {
                write!(
                    formatter,
                    "optimizer adapter parameter bound `{parameter_name}` is invalid: {source}"
                )
            }
            Self::DuplicateParameterBound { parameter_name } => write!(
                formatter,
                "optimizer adapter contains duplicate bound for parameter `{parameter_name}`"
            ),
        }
    }
}

impl Error for CalibrationOptimizerAdapterConfigurationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidSolverSettings { source } => Some(source),
            Self::InvalidParameterBound { source, .. } => Some(source),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CalibrationOptimizerCandidateParameterValue {
    pub parameter_name: String,
    pub value: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CalibrationOptimizerAdapterCandidateInput {
    pub candidate_id: String,
    pub objective_terms: Vec<CalibrationObjectiveTermInput>,
    pub parameter_values: Vec<CalibrationOptimizerCandidateParameterValue>,
    pub external_constraint_violation_count: usize,
}

impl CalibrationOptimizerAdapterCandidateInput {
    fn validate(&self) -> Result<(), CalibrationOptimizerAdapterCandidateInputError> {
        if self.candidate_id.trim().is_empty() {
            return Err(CalibrationOptimizerAdapterCandidateInputError::MissingCandidateId);
        }
        if self.objective_terms.is_empty() {
            return Err(CalibrationOptimizerAdapterCandidateInputError::MissingObjectiveTerms);
        }

        let mut seen_parameter_names = BTreeSet::new();
        for parameter_value in &self.parameter_values {
            if parameter_value.parameter_name.trim().is_empty() {
                return Err(
                    CalibrationOptimizerAdapterCandidateInputError::MissingParameterValueName {
                        candidate_id: self.candidate_id.clone(),
                    },
                );
            }

            if !parameter_value.value.is_finite() {
                return Err(
                    CalibrationOptimizerAdapterCandidateInputError::InvalidParameterValue {
                        candidate_id: self.candidate_id.clone(),
                        parameter_name: parameter_value.parameter_name.clone(),
                        value: parameter_value.value,
                    },
                );
            }

            if !seen_parameter_names.insert(parameter_value.parameter_name.as_str()) {
                return Err(
                    CalibrationOptimizerAdapterCandidateInputError::DuplicateParameterValue {
                        candidate_id: self.candidate_id.clone(),
                        parameter_name: parameter_value.parameter_name.clone(),
                    },
                );
            }
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum CalibrationOptimizerAdapterCandidateInputError {
    MissingCandidateId,
    MissingObjectiveTerms,
    MissingParameterValueName {
        candidate_id: String,
    },
    DuplicateParameterValue {
        candidate_id: String,
        parameter_name: String,
    },
    InvalidParameterValue {
        candidate_id: String,
        parameter_name: String,
        value: f64,
    },
}

impl Display for CalibrationOptimizerAdapterCandidateInputError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingCandidateId => {
                write!(
                    formatter,
                    "optimizer adapter candidate_id must be non-empty"
                )
            }
            Self::MissingObjectiveTerms => write!(
                formatter,
                "optimizer adapter candidate objective_terms must include at least one term"
            ),
            Self::MissingParameterValueName { candidate_id } => write!(
                formatter,
                "optimizer adapter candidate `{candidate_id}` has parameter_values entry with empty parameter_name"
            ),
            Self::DuplicateParameterValue {
                candidate_id,
                parameter_name,
            } => write!(
                formatter,
                "optimizer adapter candidate `{candidate_id}` has duplicate parameter value for `{parameter_name}`"
            ),
            Self::InvalidParameterValue {
                candidate_id,
                parameter_name,
                value,
            } => write!(
                formatter,
                "optimizer adapter candidate `{candidate_id}` has non-finite value `{value}` for parameter `{parameter_name}`"
            ),
        }
    }
}

impl Error for CalibrationOptimizerAdapterCandidateInputError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CalibrationOptimizerBoundViolationKind {
    BelowLowerBound,
    AboveUpperBound,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CalibrationOptimizerBoundViolationDiagnostic {
    pub parameter_name: String,
    pub value: f64,
    pub lower_inclusive: f64,
    pub upper_inclusive: f64,
    pub violation_kind: CalibrationOptimizerBoundViolationKind,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CalibrationOptimizerAdapterCandidateDiagnostic {
    pub candidate_id: String,
    pub objective_composition: CalibrationObjectiveCompositionReport,
    pub raw_objective_value: f64,
    pub external_constraint_violation_count: usize,
    pub bound_violation_count: usize,
    pub total_constraint_violation_count: usize,
    pub constraint_penalty: f64,
    pub penalized_objective_value: f64,
    pub bound_violations: Vec<CalibrationOptimizerBoundViolationDiagnostic>,
    pub seeded_tiebreak_key: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CalibrationOptimizerAdapterRunMetadata {
    pub schema_version: &'static str,
    pub backend_id: String,
    pub random_seed: u64,
    pub selection_policy: &'static str,
    pub penalty_policy: &'static str,
    pub bounds_policy: &'static str,
    pub solver_settings: CalibrationOptimizerSettings,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CalibrationOptimizerAdapterRunOutput {
    pub metadata: CalibrationOptimizerAdapterRunMetadata,
    pub selected_candidate_id: Option<String>,
    pub selected_candidate_position: Option<usize>,
    pub candidate_diagnostics: Vec<CalibrationOptimizerAdapterCandidateDiagnostic>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CalibrationOptimizerAdapterError {
    InvalidConfiguration {
        source: CalibrationOptimizerAdapterConfigurationError,
    },
    MissingCandidates,
    DuplicateCandidateId {
        candidate_id: String,
    },
    InvalidCandidateInput {
        candidate_id: String,
        source: CalibrationOptimizerAdapterCandidateInputError,
    },
    InvalidObjectiveComposition {
        candidate_id: String,
        source: CalibrationObjectiveCompositionError,
    },
    MissingBoundedParameterValue {
        candidate_id: String,
        parameter_name: String,
    },
    InvalidPenalizedObjectiveValue {
        candidate_id: String,
        value: f64,
    },
}

impl Display for CalibrationOptimizerAdapterError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidConfiguration { source } => {
                write!(
                    formatter,
                    "optimizer adapter configuration is invalid: {source}"
                )
            }
            Self::MissingCandidates => write!(
                formatter,
                "optimizer adapter invocation requires at least one candidate"
            ),
            Self::DuplicateCandidateId { candidate_id } => write!(
                formatter,
                "optimizer adapter candidates contain duplicate candidate_id `{candidate_id}`"
            ),
            Self::InvalidCandidateInput {
                candidate_id,
                source,
            } => {
                write!(
                    formatter,
                    "optimizer adapter candidate `{candidate_id}` is invalid: {source}"
                )
            }
            Self::InvalidObjectiveComposition {
                candidate_id,
                source,
            } => {
                write!(
                    formatter,
                    "optimizer adapter candidate `{candidate_id}` objective composition failed: {source}"
                )
            }
            Self::MissingBoundedParameterValue {
                candidate_id,
                parameter_name,
            } => write!(
                formatter,
                "optimizer adapter candidate `{candidate_id}` is missing required bounded parameter `{parameter_name}`"
            ),
            Self::InvalidPenalizedObjectiveValue {
                candidate_id,
                value,
            } => write!(
                formatter,
                "optimizer adapter candidate `{candidate_id}` produced non-finite penalized objective value `{value}`"
            ),
        }
    }
}

impl Error for CalibrationOptimizerAdapterError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidConfiguration { source } => Some(source),
            Self::InvalidCandidateInput { source, .. } => Some(source),
            Self::InvalidObjectiveComposition { source, .. } => Some(source),
            _ => None,
        }
    }
}

pub fn run_constrained_optimizer_adapter(
    configuration: &CalibrationOptimizerAdapterConfiguration,
    candidate_inputs: &[CalibrationOptimizerAdapterCandidateInput],
) -> Result<CalibrationOptimizerAdapterRunOutput, CalibrationOptimizerAdapterError> {
    configuration
        .validate()
        .map_err(|source| CalibrationOptimizerAdapterError::InvalidConfiguration { source })?;

    if candidate_inputs.is_empty() {
        return Err(CalibrationOptimizerAdapterError::MissingCandidates);
    }

    let mut seen_candidate_ids = BTreeSet::new();
    let mut candidate_diagnostics = Vec::with_capacity(candidate_inputs.len());

    for candidate_input in candidate_inputs {
        if !seen_candidate_ids.insert(candidate_input.candidate_id.as_str()) {
            return Err(CalibrationOptimizerAdapterError::DuplicateCandidateId {
                candidate_id: candidate_input.candidate_id.clone(),
            });
        }

        candidate_input.validate().map_err(|source| {
            CalibrationOptimizerAdapterError::InvalidCandidateInput {
                candidate_id: candidate_input.candidate_id.clone(),
                source,
            }
        })?;

        let objective_composition =
            compose_calibration_weighted_objective(candidate_input.objective_terms.as_slice())
                .map_err(|source| {
                    CalibrationOptimizerAdapterError::InvalidObjectiveComposition {
                        candidate_id: candidate_input.candidate_id.clone(),
                        source,
                    }
                })?;

        let parameter_values = candidate_input
            .parameter_values
            .iter()
            .map(|parameter_value| {
                (
                    parameter_value.parameter_name.as_str(),
                    parameter_value.value,
                )
            })
            .collect::<BTreeMap<_, _>>();

        let mut bound_violations = Vec::new();
        for parameter_bound in &configuration.parameter_bounds {
            let Some(parameter_value) =
                parameter_values.get(parameter_bound.parameter_name.as_str())
            else {
                return Err(
                    CalibrationOptimizerAdapterError::MissingBoundedParameterValue {
                        candidate_id: candidate_input.candidate_id.clone(),
                        parameter_name: parameter_bound.parameter_name.clone(),
                    },
                );
            };

            if *parameter_value < parameter_bound.lower_inclusive {
                bound_violations.push(CalibrationOptimizerBoundViolationDiagnostic {
                    parameter_name: parameter_bound.parameter_name.clone(),
                    value: *parameter_value,
                    lower_inclusive: parameter_bound.lower_inclusive,
                    upper_inclusive: parameter_bound.upper_inclusive,
                    violation_kind: CalibrationOptimizerBoundViolationKind::BelowLowerBound,
                });
            } else if *parameter_value > parameter_bound.upper_inclusive {
                bound_violations.push(CalibrationOptimizerBoundViolationDiagnostic {
                    parameter_name: parameter_bound.parameter_name.clone(),
                    value: *parameter_value,
                    lower_inclusive: parameter_bound.lower_inclusive,
                    upper_inclusive: parameter_bound.upper_inclusive,
                    violation_kind: CalibrationOptimizerBoundViolationKind::AboveUpperBound,
                });
            }
        }

        let bound_violation_count = bound_violations.len();
        let total_constraint_violation_count =
            candidate_input.external_constraint_violation_count + bound_violation_count;
        let constraint_penalty = configuration.solver_settings.constraint_penalty_weight
            * total_constraint_violation_count as f64;
        let penalized_objective_value =
            objective_composition.weighted_objective_value + constraint_penalty;

        if !penalized_objective_value.is_finite() {
            return Err(
                CalibrationOptimizerAdapterError::InvalidPenalizedObjectiveValue {
                    candidate_id: candidate_input.candidate_id.clone(),
                    value: penalized_objective_value,
                },
            );
        }

        candidate_diagnostics.push(CalibrationOptimizerAdapterCandidateDiagnostic {
            candidate_id: candidate_input.candidate_id.clone(),
            raw_objective_value: objective_composition.weighted_objective_value,
            objective_composition,
            external_constraint_violation_count: candidate_input
                .external_constraint_violation_count,
            bound_violation_count,
            total_constraint_violation_count,
            constraint_penalty,
            penalized_objective_value,
            bound_violations,
            seeded_tiebreak_key: seeded_tiebreak_key(
                configuration.random_seed,
                candidate_input.candidate_id.as_str(),
            ),
        });
    }

    let selected_candidate_position = candidate_diagnostics
        .iter()
        .enumerate()
        .filter(|(_, candidate_diagnostic)| {
            candidate_diagnostic.total_constraint_violation_count == 0
        })
        .map(|(candidate_position, _)| candidate_position)
        .reduce(|best_position, candidate_position| {
            let candidate = &candidate_diagnostics[candidate_position];
            let current_best = &candidate_diagnostics[best_position];

            match candidate
                .penalized_objective_value
                .total_cmp(&current_best.penalized_objective_value)
            {
                std::cmp::Ordering::Less => candidate_position,
                std::cmp::Ordering::Greater => best_position,
                std::cmp::Ordering::Equal => match candidate
                    .raw_objective_value
                    .total_cmp(&current_best.raw_objective_value)
                {
                    std::cmp::Ordering::Less => candidate_position,
                    std::cmp::Ordering::Greater => best_position,
                    std::cmp::Ordering::Equal => match candidate
                        .seeded_tiebreak_key
                        .cmp(&current_best.seeded_tiebreak_key)
                    {
                        std::cmp::Ordering::Less => candidate_position,
                        std::cmp::Ordering::Greater => best_position,
                        std::cmp::Ordering::Equal => {
                            if candidate.candidate_id < current_best.candidate_id {
                                candidate_position
                            } else {
                                best_position
                            }
                        }
                    },
                },
            }
        });

    Ok(CalibrationOptimizerAdapterRunOutput {
        metadata: CalibrationOptimizerAdapterRunMetadata {
            schema_version: CALIBRATION_CONSTRAINED_OPTIMIZER_ADAPTER_RUN_SCHEMA_VERSION,
            backend_id: configuration.backend_id.clone(),
            random_seed: configuration.random_seed,
            selection_policy: CALIBRATION_CONSTRAINED_OPTIMIZER_ADAPTER_SELECTION_POLICY,
            penalty_policy: CALIBRATION_CONSTRAINED_OPTIMIZER_ADAPTER_PENALTY_POLICY,
            bounds_policy: CALIBRATION_CONSTRAINED_OPTIMIZER_ADAPTER_BOUNDS_POLICY,
            solver_settings: configuration.solver_settings.clone(),
        },
        selected_candidate_id: selected_candidate_position
            .map(|position| candidate_diagnostics[position].candidate_id.clone()),
        selected_candidate_position,
        candidate_diagnostics,
    })
}

fn seeded_tiebreak_key(seed: u64, candidate_id: &str) -> u64 {
    let mut mixed = seed ^ 0x6A09_E667_F3BC_C909;
    for byte in candidate_id.as_bytes() {
        mixed = mixed.rotate_left(7) ^ (*byte as u64);
        mixed = mixed.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    }

    splitmix64_output_mixer(mixed)
}
