use std::collections::BTreeSet;
use std::error::Error;
use std::fmt::{Display, Formatter};

use serde::{Deserialize, Serialize};

use super::{
    CALIBRATION_ARTIFACT_MANIFEST_SCHEMA_FAMILY, CALIBRATION_ARTIFACT_MANIFEST_SCHEMA_GENERATION,
    CALIBRATION_ARTIFACT_MANIFEST_SCHEMA_VERSION, CalibrationExecutionMode, CalibrationStage,
};

/// Minimal top-level manifest for persisted calibration artifacts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalibrationArtifactManifest {
    pub schema_version: String,
    pub run_id: String,
    pub execution_mode: CalibrationExecutionMode,
    pub artifact_references: Vec<CalibrationArtifactReference>,
}

impl CalibrationArtifactManifest {
    pub fn try_new(
        run_id: impl Into<String>,
        artifact_references: Vec<CalibrationArtifactReference>,
    ) -> Result<Self, CalibrationArtifactManifestError> {
        let manifest = Self {
            schema_version: CALIBRATION_ARTIFACT_MANIFEST_SCHEMA_VERSION.to_string(),
            run_id: run_id.into(),
            execution_mode: CalibrationExecutionMode::Disk,
            artifact_references,
        };
        manifest.validate()?;
        Ok(manifest)
    }

    pub fn validate(&self) -> Result<(), CalibrationArtifactManifestError> {
        validate_artifact_manifest_schema_version(self.schema_version.as_str())?;

        if self.run_id.trim().is_empty() {
            return Err(CalibrationArtifactManifestError::MissingRunId);
        }

        if self.execution_mode != CalibrationExecutionMode::Disk {
            return Err(CalibrationArtifactManifestError::UnsupportedExecutionMode {
                expected: CalibrationExecutionMode::Disk,
                found: self.execution_mode,
            });
        }

        if self.artifact_references.is_empty() {
            return Err(CalibrationArtifactManifestError::MissingArtifactReferences);
        }

        let mut artifact_ids = BTreeSet::new();
        for artifact_reference in &self.artifact_references {
            if artifact_reference.artifact_id.trim().is_empty() {
                return Err(CalibrationArtifactManifestError::EmptyArtifactId);
            }

            if artifact_reference.artifact_reference.trim().is_empty() {
                return Err(CalibrationArtifactManifestError::EmptyArtifactReference {
                    artifact_id: artifact_reference.artifact_id.clone(),
                });
            }

            if !artifact_ids.insert(artifact_reference.artifact_id.clone()) {
                return Err(CalibrationArtifactManifestError::DuplicateArtifactId {
                    artifact_id: artifact_reference.artifact_id.clone(),
                });
            }
        }

        Ok(())
    }

    pub const fn schema_version() -> &'static str {
        CALIBRATION_ARTIFACT_MANIFEST_SCHEMA_VERSION
    }
}

/// Referenced persisted artifact entry recorded in the manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalibrationArtifactReference {
    pub artifact_id: String,
    pub artifact_kind: CalibrationArtifactKind,
    pub artifact_reference: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stage: Option<CalibrationStage>,
}

/// High-level artifact categories linked from one persisted run manifest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CalibrationArtifactKind {
    RunContract,
    DatasetSnapshot,
    StageOutput,
    StageCheckpoint,
    Diagnostics,
    Comparison,
    Metadata,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CalibrationArtifactManifestError {
    InvalidSchemaVersionFormat {
        found: String,
    },
    UnexpectedSchemaFamily {
        expected: String,
        found: String,
    },
    UnsupportedSchemaGeneration {
        expected: String,
        found: String,
    },
    MissingRunId,
    UnsupportedExecutionMode {
        expected: CalibrationExecutionMode,
        found: CalibrationExecutionMode,
    },
    MissingArtifactReferences,
    EmptyArtifactId,
    EmptyArtifactReference {
        artifact_id: String,
    },
    DuplicateArtifactId {
        artifact_id: String,
    },
}

impl Display for CalibrationArtifactManifestError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidSchemaVersionFormat { found } => write!(
                formatter,
                "artifact manifest schema version `{found}` does not match the expected `<schema-family>.<generation>` format"
            ),
            Self::UnexpectedSchemaFamily { expected, found } => write!(
                formatter,
                "artifact manifest schema family `{found}` is unsupported, expected `{expected}`"
            ),
            Self::UnsupportedSchemaGeneration { expected, found } => write!(
                formatter,
                "artifact manifest schema generation `{found}` is unsupported, expected `{expected}`"
            ),
            Self::MissingRunId => write!(formatter, "run_id is required"),
            Self::UnsupportedExecutionMode { expected, found } => write!(
                formatter,
                "artifact manifest requires execution_mode={expected:?}, found `{found:?}`"
            ),
            Self::MissingArtifactReferences => write!(formatter, "artifact_references is required"),
            Self::EmptyArtifactId => write!(
                formatter,
                "artifact_references entries must use a non-empty artifact_id"
            ),
            Self::EmptyArtifactReference { artifact_id } => write!(
                formatter,
                "artifact_references entry `{artifact_id}` must use a non-empty artifact_reference"
            ),
            Self::DuplicateArtifactId { artifact_id } => write!(
                formatter,
                "artifact_references contains duplicate artifact_id `{artifact_id}`"
            ),
        }
    }
}

impl Error for CalibrationArtifactManifestError {}

fn validate_artifact_manifest_schema_version(
    schema_version: &str,
) -> Result<(), CalibrationArtifactManifestError> {
    let Some((schema_family, schema_generation_number)) = schema_version.rsplit_once(".v") else {
        return Err(
            CalibrationArtifactManifestError::InvalidSchemaVersionFormat {
                found: schema_version.to_string(),
            },
        );
    };

    if schema_family != CALIBRATION_ARTIFACT_MANIFEST_SCHEMA_FAMILY {
        return Err(CalibrationArtifactManifestError::UnexpectedSchemaFamily {
            expected: CALIBRATION_ARTIFACT_MANIFEST_SCHEMA_FAMILY.to_string(),
            found: schema_family.to_string(),
        });
    }

    let schema_generation = format!("v{schema_generation_number}");
    if schema_generation != CALIBRATION_ARTIFACT_MANIFEST_SCHEMA_GENERATION {
        return Err(
            CalibrationArtifactManifestError::UnsupportedSchemaGeneration {
                expected: CALIBRATION_ARTIFACT_MANIFEST_SCHEMA_GENERATION.to_string(),
                found: schema_generation,
            },
        );
    }

    Ok(())
}
