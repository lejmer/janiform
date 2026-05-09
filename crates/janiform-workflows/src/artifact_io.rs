use std::path::Path;
use std::path::PathBuf;

use janiform_foundations::json::document::{JsonDocumentWriteError, write_pretty_json_document};
use serde::Serialize;

use crate::error::WorkflowError;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ArtifactWriteKind {
    Fit,
    Pricing,
    Simulation,
    Validation,
}

impl ArtifactWriteKind {
    fn write_error(self, path: PathBuf, source: std::io::Error) -> WorkflowError {
        match self {
            Self::Fit => WorkflowError::FitArtifactWrite { path, source },
            Self::Pricing => WorkflowError::PricingArtifactWrite { path, source },
            Self::Simulation => WorkflowError::SimulationArtifactWrite { path, source },
            Self::Validation => WorkflowError::ValidationArtifactWrite { path, source },
        }
    }
}

pub(crate) fn write_pretty_json_file<T>(
    path: &Path,
    payload: &T,
    artifact_kind: ArtifactWriteKind,
) -> Result<(), WorkflowError>
where
    T: Serialize + ?Sized,
{
    write_pretty_json_document(path, payload).map_err(|source| match source {
        JsonDocumentWriteError::CreateDirectory { path, source }
        | JsonDocumentWriteError::Create { path, source }
        | JsonDocumentWriteError::Write { path, source } => artifact_kind.write_error(path, source),
        JsonDocumentWriteError::Serialize { source } => WorkflowError::OutputSerialize { source },
    })
}
