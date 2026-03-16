//! Stylized-fact metric catalog error types.

use std::error::Error;
use std::fmt::{Display, Formatter};

use super::core::{StylizedFactMetricClass, StylizedFactMetricFamily};

#[derive(Debug, Clone, PartialEq)]
pub enum StylizedFactMetricCatalogValidationError {
    UnsupportedSchemaVersion {
        expected: &'static str,
        actual: String,
    },
    InvalidMetadataField {
        field_name: &'static str,
        expected: &'static str,
        actual: String,
    },
    EmptyField {
        field_name: &'static str,
    },
    InvalidRequiredFieldList {
        field_name: &'static str,
        expected: Vec<String>,
        actual: Vec<String>,
    },
    InvalidUncertaintyPolicyConfiguration {
        field_name: &'static str,
        details: String,
    },
    InvalidInsufficientDataSemantics {
        field_name: &'static str,
        details: String,
    },
    EmptyMetricList,
    DuplicateMetricId {
        metric_id: String,
    },
    DuplicateMetricName {
        metric_name: String,
    },
    InvalidMetricIdFormat {
        metric_id: String,
    },
    InvalidMetricDefinition {
        metric_id: String,
        field_name: &'static str,
        details: String,
    },
    InvalidMetricClassFamilyCombination {
        metric_id: String,
        metric_class: StylizedFactMetricClass,
        metric_family: StylizedFactMetricFamily,
    },
    MissingEstimatorMetadata {
        metric_id: String,
    },
    UnexpectedEstimatorMetadata {
        metric_id: String,
    },
    MissingSecondaryMomentVariant {
        metric_id: String,
    },
    UnexpectedSecondaryMomentVariant {
        metric_id: String,
    },
    InvalidSecondaryMomentVariant {
        metric_id: String,
        details: String,
    },
    InvalidMetricUncertaintyConfiguration {
        metric_id: String,
        field_name: &'static str,
        details: String,
    },
    InvalidMetricSampleSufficiency {
        metric_id: String,
        field_name: &'static str,
        details: String,
    },
}

impl Display for StylizedFactMetricCatalogValidationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedSchemaVersion { expected, actual } => write!(
                formatter,
                "stylized fact metric catalog schema version is unsupported, expected `{expected}`, found `{actual}`"
            ),
            Self::InvalidMetadataField {
                field_name,
                expected,
                actual,
            } => write!(
                formatter,
                "metadata field `{field_name}` must be `{expected}`, found `{actual}`"
            ),
            Self::EmptyField { field_name } => {
                write!(formatter, "field `{field_name}` must be a non-empty string")
            }
            Self::InvalidRequiredFieldList {
                field_name,
                expected,
                actual,
            } => write!(
                formatter,
                "field list `{field_name}` is invalid, expected {:?}, found {:?}",
                expected, actual
            ),
            Self::InvalidUncertaintyPolicyConfiguration {
                field_name,
                details,
            } => write!(
                formatter,
                "uncertainty policy field `{field_name}` is invalid: {details}"
            ),
            Self::InvalidInsufficientDataSemantics {
                field_name,
                details,
            } => write!(
                formatter,
                "insufficient data semantics field `{field_name}` is invalid: {details}"
            ),
            Self::EmptyMetricList => {
                write!(formatter, "metrics must contain at least one definition")
            }
            Self::DuplicateMetricId { metric_id } => {
                write!(
                    formatter,
                    "duplicate metric_id `{metric_id}` is not allowed"
                )
            }
            Self::DuplicateMetricName { metric_name } => {
                write!(
                    formatter,
                    "duplicate metric_name `{metric_name}` is not allowed"
                )
            }
            Self::InvalidMetricIdFormat { metric_id } => write!(
                formatter,
                "metric_id `{metric_id}` must be snake_case lowercase alphanumeric with underscores"
            ),
            Self::InvalidMetricDefinition {
                metric_id,
                field_name,
                details,
            } => write!(
                formatter,
                "metric `{metric_id}` field `{field_name}` is invalid: {details}"
            ),
            Self::InvalidMetricClassFamilyCombination {
                metric_id,
                metric_class,
                metric_family,
            } => write!(
                formatter,
                "metric `{metric_id}` has invalid class-family combination `{metric_class:?}` and `{metric_family:?}`"
            ),
            Self::MissingEstimatorMetadata { metric_id } => write!(
                formatter,
                "secondary_moment metric `{metric_id}` requires estimator_metadata"
            ),
            Self::UnexpectedEstimatorMetadata { metric_id } => write!(
                formatter,
                "primary_robust metric `{metric_id}` must not define estimator_metadata"
            ),
            Self::MissingSecondaryMomentVariant { metric_id } => write!(
                formatter,
                "secondary_moment metric `{metric_id}` requires secondary_moment_variant"
            ),
            Self::UnexpectedSecondaryMomentVariant { metric_id } => write!(
                formatter,
                "primary_robust metric `{metric_id}` must not define secondary_moment_variant"
            ),
            Self::InvalidSecondaryMomentVariant { metric_id, details } => write!(
                formatter,
                "secondary moment variant for metric `{metric_id}` is invalid: {details}"
            ),
            Self::InvalidMetricUncertaintyConfiguration {
                metric_id,
                field_name,
                details,
            } => write!(
                formatter,
                "metric `{metric_id}` uncertainty field `{field_name}` is invalid: {details}"
            ),
            Self::InvalidMetricSampleSufficiency {
                metric_id,
                field_name,
                details,
            } => write!(
                formatter,
                "metric `{metric_id}` sample sufficiency field `{field_name}` is invalid: {details}"
            ),
        }
    }
}

impl Error for StylizedFactMetricCatalogValidationError {}

#[derive(Debug, Clone, PartialEq)]
pub enum StylizedFactMetricCatalogLoadError {
    InvalidJson {
        source: String,
    },
    InvalidCatalog {
        source: StylizedFactMetricCatalogValidationError,
    },
}

impl Display for StylizedFactMetricCatalogLoadError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidJson { source } => {
                write!(
                    formatter,
                    "stylized fact metric catalog payload must be valid JSON: {source}"
                )
            }
            Self::InvalidCatalog { source } => {
                write!(
                    formatter,
                    "stylized fact metric catalog payload is invalid: {source}"
                )
            }
        }
    }
}

impl Error for StylizedFactMetricCatalogLoadError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidCatalog { source } => Some(source),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum StylizedFactMetricCatalogSerializeError {
    InvalidCatalog {
        source: StylizedFactMetricCatalogValidationError,
    },
    SerializationFailure {
        source: String,
    },
}

impl Display for StylizedFactMetricCatalogSerializeError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidCatalog { source } => {
                write!(
                    formatter,
                    "stylized fact metric catalog is invalid: {source}"
                )
            }
            Self::SerializationFailure { source } => {
                write!(
                    formatter,
                    "failed to serialize stylized fact metric catalog: {source}"
                )
            }
        }
    }
}

impl Error for StylizedFactMetricCatalogSerializeError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidCatalog { source } => Some(source),
            _ => None,
        }
    }
}
