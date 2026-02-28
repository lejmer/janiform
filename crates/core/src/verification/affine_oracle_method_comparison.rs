//! Affine-oracle method comparison harness and tolerance-policy contracts.

use std::collections::BTreeSet;
use std::error::Error;
use std::fmt::{Display, Formatter};

use serde::{Deserialize, Serialize};

use crate::affine_oracle::{
    ExponentialAffineTransformGuardFailureCode, ExponentialAffineTransformInput,
    ExponentialAffineTransformIntegrationResult, ExponentialAffineTransformIntegrationStatus,
    ExponentialAffineTransformIntegratorMethod, ExponentialAffineTransformOutput,
    ExponentialAffineTransformSolverError, ExponentialAffineTransformSolverSettings,
    ExponentialAffineTransformSolverSettingsError,
    solve_exponential_affine_transform_with_settings,
};
use crate::nig::ComplexScalar;

pub const EXPONENTIAL_AFFINE_TRANSFORM_METHOD_COMPARISON_TOLERANCE_POLICY_SCHEMA_VERSION: &str =
    "affine_oracle.method_comparison.tolerance_policy.v1";
pub const EXPONENTIAL_AFFINE_TRANSFORM_METHOD_COMPARISON_REPORT_SCHEMA_VERSION: &str =
    "affine_oracle.method_comparison.report.v1";
pub const EXPONENTIAL_AFFINE_TRANSFORM_METHOD_COMPARISON_OUTPUT_SCHEMA_METADATA_VERSION: &str =
    "affine_oracle.method_comparison.output_schema_metadata.v1";
pub const EXPONENTIAL_AFFINE_TRANSFORM_METHOD_COMPARISON_AGREEMENT_POLICY: &str =
    "require_completed_status_and_absolute_tolerance_agreement.v1";
pub const EXPONENTIAL_AFFINE_TRANSFORM_METHOD_COMPARISON_INTERVAL_SELECTION_POLICY: &str =
    "first_band_with_delta_upper_bound_greater_or_equal_to_delta.v1";
pub const EXPONENTIAL_AFFINE_TRANSFORM_METHOD_COMPARISON_REFERENCE_FIXTURE_POLICY: &str =
    "method_comparison_checks_are_complementary_to_reference_fixtures.v1";
pub const EXPONENTIAL_AFFINE_TRANSFORM_METHOD_COMPARISON_REQUIRED_REPORT_FIELDS: [&str; 12] = [
    "schema_version",
    "output_schema_metadata",
    "policy_schema_version",
    "policy_version",
    "agreement_policy",
    "interval_selection_policy",
    "reference_fixture_policy",
    "input_delta",
    "tolerance_band",
    "baseline_run",
    "alternative_runs",
    "all_checks_passed",
];
pub const EXPONENTIAL_AFFINE_TRANSFORM_METHOD_COMPARISON_REQUIRED_TOLERANCE_BAND_FIELDS: [&str; 3] = [
    "interval_grid_label",
    "delta_upper_bound",
    "tolerance_window",
];
pub const EXPONENTIAL_AFFINE_TRANSFORM_METHOD_COMPARISON_REQUIRED_TOLERANCE_WINDOW_FIELDS: [&str;
    3] = [
    "c_at_delta_absolute_tolerance",
    "d_component_absolute_tolerance",
    "transform_value_absolute_tolerance",
];
pub const EXPONENTIAL_AFFINE_TRANSFORM_METHOD_COMPARISON_REQUIRED_RUN_SUMMARY_FIELDS: [&str; 4] = [
    "integrator_method",
    "step_count",
    "status",
    "guard_failure_code",
];
pub const EXPONENTIAL_AFFINE_TRANSFORM_METHOD_COMPARISON_REQUIRED_ALTERNATIVE_RUN_FIELDS: [&str;
    6] = [
    "run_summary",
    "status_matches_baseline",
    "output_difference",
    "tolerance_window",
    "passed",
    "failure_codes",
];
pub const EXPONENTIAL_AFFINE_TRANSFORM_METHOD_COMPARISON_REQUIRED_OUTPUT_DIFFERENCE_FIELDS: [&str;
    3] = [
    "c_at_delta_absolute_difference",
    "d_component_absolute_difference",
    "transform_value_absolute_difference",
];

/// Machine-readable output schema metadata for method-comparison reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ExponentialAffineTransformMethodComparisonOutputSchemaMetadata {
    pub schema_version: &'static str,
    pub required_report_fields: &'static [&'static str],
    pub required_tolerance_band_fields: &'static [&'static str],
    pub required_tolerance_window_fields: &'static [&'static str],
    pub required_run_summary_fields: &'static [&'static str],
    pub required_alternative_run_fields: &'static [&'static str],
    pub required_output_difference_fields: &'static [&'static str],
}

pub const EXPONENTIAL_AFFINE_TRANSFORM_METHOD_COMPARISON_OUTPUT_SCHEMA_METADATA:
    ExponentialAffineTransformMethodComparisonOutputSchemaMetadata =
    ExponentialAffineTransformMethodComparisonOutputSchemaMetadata {
        schema_version:
            EXPONENTIAL_AFFINE_TRANSFORM_METHOD_COMPARISON_OUTPUT_SCHEMA_METADATA_VERSION,
        required_report_fields:
            &EXPONENTIAL_AFFINE_TRANSFORM_METHOD_COMPARISON_REQUIRED_REPORT_FIELDS,
        required_tolerance_band_fields:
            &EXPONENTIAL_AFFINE_TRANSFORM_METHOD_COMPARISON_REQUIRED_TOLERANCE_BAND_FIELDS,
        required_tolerance_window_fields:
            &EXPONENTIAL_AFFINE_TRANSFORM_METHOD_COMPARISON_REQUIRED_TOLERANCE_WINDOW_FIELDS,
        required_run_summary_fields:
            &EXPONENTIAL_AFFINE_TRANSFORM_METHOD_COMPARISON_REQUIRED_RUN_SUMMARY_FIELDS,
        required_alternative_run_fields:
            &EXPONENTIAL_AFFINE_TRANSFORM_METHOD_COMPARISON_REQUIRED_ALTERNATIVE_RUN_FIELDS,
        required_output_difference_fields:
            &EXPONENTIAL_AFFINE_TRANSFORM_METHOD_COMPARISON_REQUIRED_OUTPUT_DIFFERENCE_FIELDS,
    };

/// Method and step configuration used by the method-comparison harness.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExponentialAffineTransformMethodComparisonSolverSettings {
    pub integrator_method: ExponentialAffineTransformIntegratorMethod,
    pub step_count: usize,
}

impl ExponentialAffineTransformMethodComparisonSolverSettings {
    pub const fn new(
        integrator_method: ExponentialAffineTransformIntegratorMethod,
        step_count: usize,
    ) -> Self {
        Self {
            integrator_method,
            step_count,
        }
    }

    pub fn try_new(
        integrator_method: ExponentialAffineTransformIntegratorMethod,
        step_count: usize,
    ) -> Result<Self, ExponentialAffineTransformSolverSettingsError> {
        let settings = Self::new(integrator_method, step_count);
        settings.validate()?;
        Ok(settings)
    }

    pub fn validate(self) -> Result<(), ExponentialAffineTransformSolverSettingsError> {
        ExponentialAffineTransformSolverSettings::new(self.integrator_method, self.step_count)
            .validate()
    }

    pub fn to_solver_settings(self) -> ExponentialAffineTransformSolverSettings {
        ExponentialAffineTransformSolverSettings::new(self.integrator_method, self.step_count)
    }
}

/// Numeric tolerances used when comparing solver outputs across integrator methods.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExponentialAffineTransformMethodComparisonToleranceWindow {
    pub c_at_delta_absolute_tolerance: f64,
    pub d_component_absolute_tolerance: f64,
    pub transform_value_absolute_tolerance: f64,
}

impl ExponentialAffineTransformMethodComparisonToleranceWindow {
    pub fn new(
        c_at_delta_absolute_tolerance: f64,
        d_component_absolute_tolerance: f64,
        transform_value_absolute_tolerance: f64,
    ) -> Self {
        Self {
            c_at_delta_absolute_tolerance,
            d_component_absolute_tolerance,
            transform_value_absolute_tolerance,
        }
    }

    pub fn try_new(
        c_at_delta_absolute_tolerance: f64,
        d_component_absolute_tolerance: f64,
        transform_value_absolute_tolerance: f64,
    ) -> Result<Self, ExponentialAffineTransformMethodComparisonToleranceWindowError> {
        let tolerance_window = Self::new(
            c_at_delta_absolute_tolerance,
            d_component_absolute_tolerance,
            transform_value_absolute_tolerance,
        );
        tolerance_window.validate()?;
        Ok(tolerance_window)
    }

    pub fn validate(
        &self,
    ) -> Result<(), ExponentialAffineTransformMethodComparisonToleranceWindowError> {
        validate_positive_finite_tolerance_value(
            "c_at_delta_absolute_tolerance",
            self.c_at_delta_absolute_tolerance,
        )?;
        validate_positive_finite_tolerance_value(
            "d_component_absolute_tolerance",
            self.d_component_absolute_tolerance,
        )?;
        validate_positive_finite_tolerance_value(
            "transform_value_absolute_tolerance",
            self.transform_value_absolute_tolerance,
        )?;

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ExponentialAffineTransformMethodComparisonToleranceWindowError {
    InvalidToleranceValue { field: &'static str, value: f64 },
}

impl Display for ExponentialAffineTransformMethodComparisonToleranceWindowError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidToleranceValue { field, value } => write!(
                formatter,
                "{field} must be finite and strictly positive, found {value}"
            ),
        }
    }
}

impl Error for ExponentialAffineTransformMethodComparisonToleranceWindowError {}

/// Interval-specific tolerance band for method comparison.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExponentialAffineTransformMethodComparisonIntervalToleranceBand {
    pub interval_grid_label: String,
    pub delta_upper_bound: f64,
    pub tolerance_window: ExponentialAffineTransformMethodComparisonToleranceWindow,
}

impl ExponentialAffineTransformMethodComparisonIntervalToleranceBand {
    pub fn new(
        interval_grid_label: String,
        delta_upper_bound: f64,
        tolerance_window: ExponentialAffineTransformMethodComparisonToleranceWindow,
    ) -> Self {
        Self {
            interval_grid_label,
            delta_upper_bound,
            tolerance_window,
        }
    }

    pub fn try_new(
        interval_grid_label: String,
        delta_upper_bound: f64,
        tolerance_window: ExponentialAffineTransformMethodComparisonToleranceWindow,
    ) -> Result<Self, ExponentialAffineTransformMethodComparisonToleranceBandError> {
        let tolerance_band = Self::new(interval_grid_label, delta_upper_bound, tolerance_window);
        tolerance_band.validate()?;
        Ok(tolerance_band)
    }

    pub fn validate(
        &self,
    ) -> Result<(), ExponentialAffineTransformMethodComparisonToleranceBandError> {
        if self.interval_grid_label.trim().is_empty() {
            return Err(
                ExponentialAffineTransformMethodComparisonToleranceBandError::EmptyIntervalGridLabel,
            );
        }

        if !self.delta_upper_bound.is_finite() || self.delta_upper_bound <= 0.0 {
            return Err(
                ExponentialAffineTransformMethodComparisonToleranceBandError::InvalidDeltaUpperBound {
                    value: self.delta_upper_bound,
                },
            );
        }

        self.tolerance_window.validate().map_err(|source| {
            ExponentialAffineTransformMethodComparisonToleranceBandError::InvalidToleranceWindow {
                source,
            }
        })?;

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ExponentialAffineTransformMethodComparisonToleranceBandError {
    EmptyIntervalGridLabel,
    InvalidDeltaUpperBound {
        value: f64,
    },
    InvalidToleranceWindow {
        source: ExponentialAffineTransformMethodComparisonToleranceWindowError,
    },
}

impl Display for ExponentialAffineTransformMethodComparisonToleranceBandError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyIntervalGridLabel => {
                write!(formatter, "interval_grid_label must be a non-empty string")
            }
            Self::InvalidDeltaUpperBound { value } => write!(
                formatter,
                "delta_upper_bound must be finite and strictly positive, found {value}"
            ),
            Self::InvalidToleranceWindow { source } => write!(formatter, "{source}"),
        }
    }
}

impl Error for ExponentialAffineTransformMethodComparisonToleranceBandError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidToleranceWindow { source } => Some(source),
            _ => None,
        }
    }
}

/// Versioned changelog entry for tolerance policy updates.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExponentialAffineTransformMethodComparisonPolicyChangeLogEntry {
    pub policy_version: String,
    pub recorded_at_utc: String,
    pub rationale: String,
}

impl ExponentialAffineTransformMethodComparisonPolicyChangeLogEntry {
    pub fn new(policy_version: String, recorded_at_utc: String, rationale: String) -> Self {
        Self {
            policy_version,
            recorded_at_utc,
            rationale,
        }
    }

    pub fn try_new(
        policy_version: String,
        recorded_at_utc: String,
        rationale: String,
    ) -> Result<Self, ExponentialAffineTransformMethodComparisonPolicyChangeLogEntryError> {
        let change_log_entry = Self::new(policy_version, recorded_at_utc, rationale);
        change_log_entry.validate()?;
        Ok(change_log_entry)
    }

    pub fn validate(
        &self,
    ) -> Result<(), ExponentialAffineTransformMethodComparisonPolicyChangeLogEntryError> {
        if self.policy_version.trim().is_empty() {
            return Err(
                ExponentialAffineTransformMethodComparisonPolicyChangeLogEntryError::EmptyPolicyVersion,
            );
        }

        if self.recorded_at_utc.trim().is_empty() {
            return Err(
                ExponentialAffineTransformMethodComparisonPolicyChangeLogEntryError::EmptyRecordedAtUtc,
            );
        }

        if self.rationale.trim().is_empty() {
            return Err(
                ExponentialAffineTransformMethodComparisonPolicyChangeLogEntryError::EmptyRationale,
            );
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ExponentialAffineTransformMethodComparisonPolicyChangeLogEntryError {
    EmptyPolicyVersion,
    EmptyRecordedAtUtc,
    EmptyRationale,
}

impl Display for ExponentialAffineTransformMethodComparisonPolicyChangeLogEntryError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyPolicyVersion => {
                write!(formatter, "policy_version must be a non-empty string")
            }
            Self::EmptyRecordedAtUtc => {
                write!(formatter, "recorded_at_utc must be a non-empty string")
            }
            Self::EmptyRationale => write!(formatter, "rationale must be a non-empty string"),
        }
    }
}

impl Error for ExponentialAffineTransformMethodComparisonPolicyChangeLogEntryError {}

/// Versioned tolerance policy for method comparison checks.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ExponentialAffineTransformMethodComparisonTolerancePolicy {
    pub schema_version: String,
    pub policy_version: String,
    pub baseline_solver_settings: ExponentialAffineTransformMethodComparisonSolverSettings,
    pub alternative_solver_settings: Vec<ExponentialAffineTransformMethodComparisonSolverSettings>,
    pub interval_tolerance_bands:
        Vec<ExponentialAffineTransformMethodComparisonIntervalToleranceBand>,
    pub change_log: Vec<ExponentialAffineTransformMethodComparisonPolicyChangeLogEntry>,
}

impl ExponentialAffineTransformMethodComparisonTolerancePolicy {
    pub fn new(
        schema_version: String,
        policy_version: String,
        baseline_solver_settings: ExponentialAffineTransformMethodComparisonSolverSettings,
        alternative_solver_settings: Vec<ExponentialAffineTransformMethodComparisonSolverSettings>,
        interval_tolerance_bands: Vec<
            ExponentialAffineTransformMethodComparisonIntervalToleranceBand,
        >,
        change_log: Vec<ExponentialAffineTransformMethodComparisonPolicyChangeLogEntry>,
    ) -> Self {
        Self {
            schema_version,
            policy_version,
            baseline_solver_settings,
            alternative_solver_settings,
            interval_tolerance_bands,
            change_log,
        }
    }

    pub fn try_new(
        schema_version: String,
        policy_version: String,
        baseline_solver_settings: ExponentialAffineTransformMethodComparisonSolverSettings,
        alternative_solver_settings: Vec<ExponentialAffineTransformMethodComparisonSolverSettings>,
        interval_tolerance_bands: Vec<
            ExponentialAffineTransformMethodComparisonIntervalToleranceBand,
        >,
        change_log: Vec<ExponentialAffineTransformMethodComparisonPolicyChangeLogEntry>,
    ) -> Result<Self, ExponentialAffineTransformMethodComparisonTolerancePolicyError> {
        let tolerance_policy = Self::new(
            schema_version,
            policy_version,
            baseline_solver_settings,
            alternative_solver_settings,
            interval_tolerance_bands,
            change_log,
        );
        tolerance_policy.validate()?;
        Ok(tolerance_policy)
    }

    pub fn validate(
        &self,
    ) -> Result<(), ExponentialAffineTransformMethodComparisonTolerancePolicyError> {
        if self.schema_version
            != EXPONENTIAL_AFFINE_TRANSFORM_METHOD_COMPARISON_TOLERANCE_POLICY_SCHEMA_VERSION
        {
            return Err(
                ExponentialAffineTransformMethodComparisonTolerancePolicyError::InvalidSchemaVersion {
                    expected:
                        EXPONENTIAL_AFFINE_TRANSFORM_METHOD_COMPARISON_TOLERANCE_POLICY_SCHEMA_VERSION,
                    actual: self.schema_version.clone(),
                },
            );
        }

        if self.policy_version.trim().is_empty() {
            return Err(
                ExponentialAffineTransformMethodComparisonTolerancePolicyError::EmptyPolicyVersion,
            );
        }

        self.baseline_solver_settings
            .validate()
            .map_err(|source| {
                ExponentialAffineTransformMethodComparisonTolerancePolicyError::InvalidBaselineSolverSettings {
                    source,
                }
            })?;

        if self.alternative_solver_settings.is_empty() {
            return Err(
                ExponentialAffineTransformMethodComparisonTolerancePolicyError::EmptyAlternativeSolverSettings,
            );
        }

        let mut observed_settings_identities = BTreeSet::new();
        for (alternative_position, alternative_settings) in
            self.alternative_solver_settings.iter().enumerate()
        {
            alternative_settings.validate().map_err(|source| {
                ExponentialAffineTransformMethodComparisonTolerancePolicyError::InvalidAlternativeSolverSettings {
                    position: alternative_position,
                    source,
                }
            })?;

            if *alternative_settings == self.baseline_solver_settings {
                return Err(
                    ExponentialAffineTransformMethodComparisonTolerancePolicyError::AlternativeMatchesBaselineSettings {
                        integrator_method: alternative_settings.integrator_method,
                        step_count: alternative_settings.step_count,
                    },
                );
            }

            let settings_identity =
                method_comparison_solver_settings_identity(*alternative_settings);
            if !observed_settings_identities.insert(settings_identity) {
                return Err(
                    ExponentialAffineTransformMethodComparisonTolerancePolicyError::DuplicateAlternativeSolverSettings {
                        integrator_method: alternative_settings.integrator_method,
                        step_count: alternative_settings.step_count,
                    },
                );
            }
        }

        if self.interval_tolerance_bands.is_empty() {
            return Err(
                ExponentialAffineTransformMethodComparisonTolerancePolicyError::EmptyIntervalToleranceBands,
            );
        }

        let mut previous_delta_upper_bound: Option<f64> = None;
        for (band_position, tolerance_band) in self.interval_tolerance_bands.iter().enumerate() {
            tolerance_band.validate().map_err(|source| {
                ExponentialAffineTransformMethodComparisonTolerancePolicyError::InvalidIntervalToleranceBand {
                    position: band_position,
                    source,
                }
            })?;

            if let Some(previous_delta_upper_bound) = previous_delta_upper_bound {
                if tolerance_band.delta_upper_bound <= previous_delta_upper_bound {
                    return Err(
                        ExponentialAffineTransformMethodComparisonTolerancePolicyError::NonIncreasingDeltaUpperBound {
                            previous: previous_delta_upper_bound,
                            current: tolerance_band.delta_upper_bound,
                        },
                    );
                }
            }

            previous_delta_upper_bound = Some(tolerance_band.delta_upper_bound);
        }

        if self.change_log.is_empty() {
            return Err(
                ExponentialAffineTransformMethodComparisonTolerancePolicyError::EmptyChangeLog,
            );
        }

        let mut observed_policy_versions = BTreeSet::new();
        let mut has_current_policy_version = false;
        for (change_log_position, change_log_entry) in self.change_log.iter().enumerate() {
            change_log_entry.validate().map_err(|source| {
                ExponentialAffineTransformMethodComparisonTolerancePolicyError::InvalidChangeLogEntry {
                    position: change_log_position,
                    source,
                }
            })?;

            if !observed_policy_versions.insert(change_log_entry.policy_version.clone()) {
                return Err(
                    ExponentialAffineTransformMethodComparisonTolerancePolicyError::DuplicateChangeLogPolicyVersion {
                        policy_version: change_log_entry.policy_version.clone(),
                    },
                );
            }

            has_current_policy_version = has_current_policy_version
                || change_log_entry.policy_version == self.policy_version;
        }

        if !has_current_policy_version {
            return Err(
                ExponentialAffineTransformMethodComparisonTolerancePolicyError::MissingCurrentPolicyVersionInChangeLog {
                    policy_version: self.policy_version.clone(),
                },
            );
        }

        Ok(())
    }

    pub fn select_tolerance_band_for_delta(
        &self,
        delta: f64,
    ) -> Result<
        ExponentialAffineTransformMethodComparisonIntervalToleranceBand,
        ExponentialAffineTransformMethodComparisonTolerancePolicyError,
    > {
        for tolerance_band in &self.interval_tolerance_bands {
            if delta <= tolerance_band.delta_upper_bound {
                return Ok(tolerance_band.clone());
            }
        }

        let maximum_supported_delta = self
            .interval_tolerance_bands
            .last()
            .map_or(0.0, |tolerance_band| tolerance_band.delta_upper_bound);
        Err(
            ExponentialAffineTransformMethodComparisonTolerancePolicyError::DeltaOutsideToleranceBands {
                delta,
                max_supported_delta: maximum_supported_delta,
            },
        )
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ExponentialAffineTransformMethodComparisonTolerancePolicyError {
    InvalidSchemaVersion {
        expected: &'static str,
        actual: String,
    },
    EmptyPolicyVersion,
    InvalidBaselineSolverSettings {
        source: ExponentialAffineTransformSolverSettingsError,
    },
    EmptyAlternativeSolverSettings,
    InvalidAlternativeSolverSettings {
        position: usize,
        source: ExponentialAffineTransformSolverSettingsError,
    },
    AlternativeMatchesBaselineSettings {
        integrator_method: ExponentialAffineTransformIntegratorMethod,
        step_count: usize,
    },
    DuplicateAlternativeSolverSettings {
        integrator_method: ExponentialAffineTransformIntegratorMethod,
        step_count: usize,
    },
    EmptyIntervalToleranceBands,
    InvalidIntervalToleranceBand {
        position: usize,
        source: ExponentialAffineTransformMethodComparisonToleranceBandError,
    },
    NonIncreasingDeltaUpperBound {
        previous: f64,
        current: f64,
    },
    EmptyChangeLog,
    InvalidChangeLogEntry {
        position: usize,
        source: ExponentialAffineTransformMethodComparisonPolicyChangeLogEntryError,
    },
    DuplicateChangeLogPolicyVersion {
        policy_version: String,
    },
    MissingCurrentPolicyVersionInChangeLog {
        policy_version: String,
    },
    DeltaOutsideToleranceBands {
        delta: f64,
        max_supported_delta: f64,
    },
}

impl Display for ExponentialAffineTransformMethodComparisonTolerancePolicyError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidSchemaVersion { expected, actual } => write!(
                formatter,
                "unsupported method-comparison tolerance policy schema_version: expected {expected}, found {actual}"
            ),
            Self::EmptyPolicyVersion => {
                write!(formatter, "policy_version must be a non-empty string")
            }
            Self::InvalidBaselineSolverSettings { source } => {
                write!(formatter, "baseline_solver_settings are invalid: {source}")
            }
            Self::EmptyAlternativeSolverSettings => write!(
                formatter,
                "alternative_solver_settings must include at least one method configuration"
            ),
            Self::InvalidAlternativeSolverSettings { position, source } => write!(
                formatter,
                "alternative_solver_settings[{position}] is invalid: {source}"
            ),
            Self::AlternativeMatchesBaselineSettings {
                integrator_method,
                step_count,
            } => write!(
                formatter,
                "alternative solver settings must differ from baseline settings, found method={integrator_method:?} step_count={step_count}"
            ),
            Self::DuplicateAlternativeSolverSettings {
                integrator_method,
                step_count,
            } => write!(
                formatter,
                "alternative solver settings must be unique, found duplicate method={integrator_method:?} step_count={step_count}"
            ),
            Self::EmptyIntervalToleranceBands => write!(
                formatter,
                "interval_tolerance_bands must include at least one tolerance band"
            ),
            Self::InvalidIntervalToleranceBand { position, source } => write!(
                formatter,
                "interval_tolerance_bands[{position}] is invalid: {source}"
            ),
            Self::NonIncreasingDeltaUpperBound { previous, current } => write!(
                formatter,
                "interval_tolerance_bands delta_upper_bound values must be strictly increasing, found previous={previous}, current={current}"
            ),
            Self::EmptyChangeLog => {
                write!(
                    formatter,
                    "change_log must include at least one policy history entry"
                )
            }
            Self::InvalidChangeLogEntry { position, source } => {
                write!(formatter, "change_log[{position}] is invalid: {source}")
            }
            Self::DuplicateChangeLogPolicyVersion { policy_version } => write!(
                formatter,
                "change_log contains duplicate policy_version: {policy_version}"
            ),
            Self::MissingCurrentPolicyVersionInChangeLog { policy_version } => write!(
                formatter,
                "change_log must include an entry for policy_version '{policy_version}'"
            ),
            Self::DeltaOutsideToleranceBands {
                delta,
                max_supported_delta,
            } => write!(
                formatter,
                "delta {delta} is outside configured tolerance bands, maximum supported delta is {max_supported_delta}"
            ),
        }
    }
}

impl Error for ExponentialAffineTransformMethodComparisonTolerancePolicyError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidBaselineSolverSettings { source } => Some(source),
            Self::InvalidAlternativeSolverSettings { source, .. } => Some(source),
            Self::InvalidIntervalToleranceBand { source, .. } => Some(source),
            Self::InvalidChangeLogEntry { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// Per-method run summary emitted in method-comparison reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ExponentialAffineTransformMethodComparisonRunSummary {
    pub integrator_method: ExponentialAffineTransformIntegratorMethod,
    pub step_count: usize,
    pub status: ExponentialAffineTransformIntegrationStatus,
    pub guard_failure_code: Option<ExponentialAffineTransformGuardFailureCode>,
}

/// Absolute output differences measured against baseline.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct ExponentialAffineTransformMethodComparisonOutputDifference {
    pub c_at_delta_absolute_difference: f64,
    pub d_component_absolute_difference: f64,
    pub transform_value_absolute_difference: f64,
}

/// Machine-readable reason codes for alternative-run method comparison failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExponentialAffineTransformMethodComparisonFailureCode {
    StatusMismatch,
    BaselineNotCompleted,
    AlternativeNotCompleted,
    BaselineOutputMissing,
    AlternativeOutputMissing,
    CAtDeltaToleranceExceeded,
    DComponentToleranceExceeded,
    TransformValueToleranceExceeded,
}

/// Comparison result for one alternative method configuration.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ExponentialAffineTransformMethodComparisonAlternativeRunReport {
    pub run_summary: ExponentialAffineTransformMethodComparisonRunSummary,
    pub status_matches_baseline: bool,
    pub output_difference: Option<ExponentialAffineTransformMethodComparisonOutputDifference>,
    pub tolerance_window: ExponentialAffineTransformMethodComparisonToleranceWindow,
    pub passed: bool,
    pub failure_codes: Vec<ExponentialAffineTransformMethodComparisonFailureCode>,
}

/// Full method-comparison report for one transform input.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ExponentialAffineTransformMethodComparisonReport {
    pub schema_version: &'static str,
    pub output_schema_metadata: ExponentialAffineTransformMethodComparisonOutputSchemaMetadata,
    pub policy_schema_version: String,
    pub policy_version: String,
    pub agreement_policy: &'static str,
    pub interval_selection_policy: &'static str,
    pub reference_fixture_policy: &'static str,
    pub input_delta: f64,
    pub tolerance_band: ExponentialAffineTransformMethodComparisonIntervalToleranceBand,
    pub baseline_run: ExponentialAffineTransformMethodComparisonRunSummary,
    pub alternative_runs: Vec<ExponentialAffineTransformMethodComparisonAlternativeRunReport>,
    pub all_checks_passed: bool,
}

/// Structured failure modes for method-comparison harness execution.
#[derive(Debug, Clone, PartialEq)]
pub enum ExponentialAffineTransformMethodComparisonError {
    InvalidTolerancePolicy {
        source: ExponentialAffineTransformMethodComparisonTolerancePolicyError,
    },
    BaselineSolverExecutionFailed {
        source: ExponentialAffineTransformSolverError,
    },
    AlternativeSolverExecutionFailed {
        alternative_position: usize,
        source: ExponentialAffineTransformSolverError,
    },
}

impl Display for ExponentialAffineTransformMethodComparisonError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidTolerancePolicy { source } => write!(
                formatter,
                "method-comparison tolerance policy is invalid: {source}"
            ),
            Self::BaselineSolverExecutionFailed { source } => {
                write!(formatter, "baseline solver execution failed: {source}")
            }
            Self::AlternativeSolverExecutionFailed {
                alternative_position,
                source,
            } => write!(
                formatter,
                "alternative solver execution failed for alternative_solver_settings[{alternative_position}]: {source}"
            ),
        }
    }
}

impl Error for ExponentialAffineTransformMethodComparisonError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidTolerancePolicy { source } => Some(source),
            Self::BaselineSolverExecutionFailed { source } => Some(source),
            Self::AlternativeSolverExecutionFailed { source, .. } => Some(source),
        }
    }
}

/// Compare solver outputs across configured integration methods under one tolerance policy.
///
/// This harness complements the deterministic reference fixture suite and does not replace it.
pub fn compare_exponential_affine_transform_integration_methods(
    transform_input: ExponentialAffineTransformInput,
    tolerance_policy: ExponentialAffineTransformMethodComparisonTolerancePolicy,
) -> Result<
    ExponentialAffineTransformMethodComparisonReport,
    ExponentialAffineTransformMethodComparisonError,
> {
    tolerance_policy.validate().map_err(|source| {
        ExponentialAffineTransformMethodComparisonError::InvalidTolerancePolicy { source }
    })?;
    let tolerance_band = tolerance_policy
        .select_tolerance_band_for_delta(transform_input.delta)
        .map_err(|source| {
            ExponentialAffineTransformMethodComparisonError::InvalidTolerancePolicy { source }
        })?;

    let baseline_solver_settings = tolerance_policy
        .baseline_solver_settings
        .to_solver_settings();
    let baseline_result =
        solve_exponential_affine_transform_with_settings(transform_input, baseline_solver_settings)
            .map_err(|source| {
                ExponentialAffineTransformMethodComparisonError::BaselineSolverExecutionFailed {
                    source,
                }
            })?;
    let baseline_run_summary =
        method_comparison_run_summary(tolerance_policy.baseline_solver_settings, baseline_result);

    let mut alternative_runs = Vec::new();
    for (alternative_position, alternative_solver_settings) in tolerance_policy
        .alternative_solver_settings
        .iter()
        .enumerate()
    {
        let alternative_result = solve_exponential_affine_transform_with_settings(
            transform_input,
            alternative_solver_settings.to_solver_settings(),
        )
        .map_err(|source| {
            ExponentialAffineTransformMethodComparisonError::AlternativeSolverExecutionFailed {
                alternative_position,
                source,
            }
        })?;

        alternative_runs.push(evaluate_method_comparison_alternative_run(
            baseline_result,
            *alternative_solver_settings,
            alternative_result,
            tolerance_band.tolerance_window,
        ));
    }

    let all_checks_passed = alternative_runs
        .iter()
        .all(|alternative_run| alternative_run.passed);

    Ok(ExponentialAffineTransformMethodComparisonReport {
        schema_version: EXPONENTIAL_AFFINE_TRANSFORM_METHOD_COMPARISON_REPORT_SCHEMA_VERSION,
        output_schema_metadata:
            EXPONENTIAL_AFFINE_TRANSFORM_METHOD_COMPARISON_OUTPUT_SCHEMA_METADATA,
        policy_schema_version: tolerance_policy.schema_version,
        policy_version: tolerance_policy.policy_version,
        agreement_policy: EXPONENTIAL_AFFINE_TRANSFORM_METHOD_COMPARISON_AGREEMENT_POLICY,
        interval_selection_policy:
            EXPONENTIAL_AFFINE_TRANSFORM_METHOD_COMPARISON_INTERVAL_SELECTION_POLICY,
        reference_fixture_policy:
            EXPONENTIAL_AFFINE_TRANSFORM_METHOD_COMPARISON_REFERENCE_FIXTURE_POLICY,
        input_delta: transform_input.delta,
        tolerance_band,
        baseline_run: baseline_run_summary,
        alternative_runs,
        all_checks_passed,
    })
}
fn validate_positive_finite_tolerance_value(
    field: &'static str,
    value: f64,
) -> Result<(), ExponentialAffineTransformMethodComparisonToleranceWindowError> {
    if !value.is_finite() || value <= 0.0 {
        return Err(
            ExponentialAffineTransformMethodComparisonToleranceWindowError::InvalidToleranceValue {
                field,
                value,
            },
        );
    }

    Ok(())
}

fn method_comparison_solver_settings_identity(
    settings: ExponentialAffineTransformMethodComparisonSolverSettings,
) -> String {
    format!(
        "{integrator_method}:{step_count}",
        integrator_method = method_comparison_integrator_method_label(settings.integrator_method),
        step_count = settings.step_count
    )
}

fn method_comparison_integrator_method_label(
    integrator_method: ExponentialAffineTransformIntegratorMethod,
) -> &'static str {
    match integrator_method {
        ExponentialAffineTransformIntegratorMethod::FourthOrderRungeKutta => {
            "fourth_order_runge_kutta"
        }
        ExponentialAffineTransformIntegratorMethod::ForwardEuler => "forward_euler",
    }
}

fn method_comparison_run_summary(
    solver_settings: ExponentialAffineTransformMethodComparisonSolverSettings,
    integration_result: ExponentialAffineTransformIntegrationResult,
) -> ExponentialAffineTransformMethodComparisonRunSummary {
    ExponentialAffineTransformMethodComparisonRunSummary {
        integrator_method: solver_settings.integrator_method,
        step_count: solver_settings.step_count,
        status: integration_result.status,
        guard_failure_code: integration_result
            .metadata
            .guard_failure
            .map(|guard_failure| guard_failure.code),
    }
}

fn evaluate_method_comparison_alternative_run(
    baseline_result: ExponentialAffineTransformIntegrationResult,
    alternative_solver_settings: ExponentialAffineTransformMethodComparisonSolverSettings,
    alternative_result: ExponentialAffineTransformIntegrationResult,
    tolerance_window: ExponentialAffineTransformMethodComparisonToleranceWindow,
) -> ExponentialAffineTransformMethodComparisonAlternativeRunReport {
    let run_summary =
        method_comparison_run_summary(alternative_solver_settings, alternative_result);
    let status_matches_baseline = baseline_result.status == alternative_result.status;
    let mut failure_codes = Vec::new();

    if !status_matches_baseline {
        failure_codes.push(ExponentialAffineTransformMethodComparisonFailureCode::StatusMismatch);
    }
    if baseline_result.status != ExponentialAffineTransformIntegrationStatus::Completed {
        failure_codes
            .push(ExponentialAffineTransformMethodComparisonFailureCode::BaselineNotCompleted);
    }
    if alternative_result.status != ExponentialAffineTransformIntegrationStatus::Completed {
        failure_codes
            .push(ExponentialAffineTransformMethodComparisonFailureCode::AlternativeNotCompleted);
    }

    let output_difference = if baseline_result.status
        == ExponentialAffineTransformIntegrationStatus::Completed
        && alternative_result.status == ExponentialAffineTransformIntegrationStatus::Completed
    {
        match (baseline_result.output, alternative_result.output) {
            (Some(baseline_output), Some(alternative_output)) => {
                let output_difference = evaluate_method_comparison_output_difference(
                    baseline_output,
                    alternative_output,
                );

                if output_difference.c_at_delta_absolute_difference
                    > tolerance_window.c_at_delta_absolute_tolerance
                {
                    failure_codes.push(
                        ExponentialAffineTransformMethodComparisonFailureCode::CAtDeltaToleranceExceeded,
                    );
                }

                if output_difference.d_component_absolute_difference
                    > tolerance_window.d_component_absolute_tolerance
                {
                    failure_codes.push(
                        ExponentialAffineTransformMethodComparisonFailureCode::DComponentToleranceExceeded,
                    );
                }

                if output_difference.transform_value_absolute_difference
                    > tolerance_window.transform_value_absolute_tolerance
                {
                    failure_codes.push(
                        ExponentialAffineTransformMethodComparisonFailureCode::TransformValueToleranceExceeded,
                    );
                }

                Some(output_difference)
            }
            (None, Some(_)) => {
                failure_codes.push(
                    ExponentialAffineTransformMethodComparisonFailureCode::BaselineOutputMissing,
                );
                None
            }
            (Some(_), None) => {
                failure_codes.push(
                    ExponentialAffineTransformMethodComparisonFailureCode::AlternativeOutputMissing,
                );
                None
            }
            (None, None) => {
                failure_codes.push(
                    ExponentialAffineTransformMethodComparisonFailureCode::BaselineOutputMissing,
                );
                failure_codes.push(
                    ExponentialAffineTransformMethodComparisonFailureCode::AlternativeOutputMissing,
                );
                None
            }
        }
    } else {
        None
    };

    let passed = failure_codes.is_empty();

    ExponentialAffineTransformMethodComparisonAlternativeRunReport {
        run_summary,
        status_matches_baseline,
        output_difference,
        tolerance_window,
        passed,
        failure_codes,
    }
}

fn evaluate_method_comparison_output_difference(
    baseline_output: ExponentialAffineTransformOutput,
    alternative_output: ExponentialAffineTransformOutput,
) -> ExponentialAffineTransformMethodComparisonOutputDifference {
    let d_component_absolute_difference = complex_componentwise_absolute_difference(
        baseline_output.d_at_delta.plus_plus,
        alternative_output.d_at_delta.plus_plus,
    )
    .max(complex_componentwise_absolute_difference(
        baseline_output.d_at_delta.plus_minus,
        alternative_output.d_at_delta.plus_minus,
    ))
    .max(complex_componentwise_absolute_difference(
        baseline_output.d_at_delta.minus_plus,
        alternative_output.d_at_delta.minus_plus,
    ))
    .max(complex_componentwise_absolute_difference(
        baseline_output.d_at_delta.minus_minus,
        alternative_output.d_at_delta.minus_minus,
    ));

    ExponentialAffineTransformMethodComparisonOutputDifference {
        c_at_delta_absolute_difference: complex_componentwise_absolute_difference(
            baseline_output.c_at_delta,
            alternative_output.c_at_delta,
        ),
        d_component_absolute_difference,
        transform_value_absolute_difference: complex_componentwise_absolute_difference(
            baseline_output.transform_value,
            alternative_output.transform_value,
        ),
    }
}

fn complex_componentwise_absolute_difference(left: ComplexScalar, right: ComplexScalar) -> f64 {
    (left.real - right.real)
        .abs()
        .max((left.imaginary - right.imaginary).abs())
}
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExponentialAffineTransformMethodComparisonTolerancePolicyPayload {
    schema_version: String,
    policy_version: String,
    baseline_solver_settings: ExponentialAffineTransformMethodComparisonSolverSettings,
    alternative_solver_settings: Vec<ExponentialAffineTransformMethodComparisonSolverSettings>,
    interval_tolerance_bands: Vec<ExponentialAffineTransformMethodComparisonIntervalToleranceBand>,
    change_log: Vec<ExponentialAffineTransformMethodComparisonPolicyChangeLogEntry>,
}

impl<'de> Deserialize<'de> for ExponentialAffineTransformMethodComparisonTolerancePolicy {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let payload =
            ExponentialAffineTransformMethodComparisonTolerancePolicyPayload::deserialize(
                deserializer,
            )?;
        Self::try_new(
            payload.schema_version,
            payload.policy_version,
            payload.baseline_solver_settings,
            payload.alternative_solver_settings,
            payload.interval_tolerance_bands,
            payload.change_log,
        )
        .map_err(serde::de::Error::custom)
    }
}
