use janiform_calibration_artifacts::CalibrationStage;
use janiform_calibration_runtime::{
    CalibrationMarketDataWorkflowScope, CalibrationStagedExecutionInput,
};

use super::labels::workflow_scope_label;

/// Stable workflow-scope identifier used by fit artifacts and events.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkflowScopeId {
    /// Only the seasonality stage is selected.
    SeasonalityOnly,
    /// Underlying calibration without option-surface refinement.
    FullWithoutOptionSurface,
    /// Full calibration with option-surface refinement.
    FullWithOptionSurface,
}

impl WorkflowScopeId {
    /// Stable serialized identifier.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SeasonalityOnly => "seasonality_only",
            Self::FullWithoutOptionSurface => "full_without_option_surface",
            Self::FullWithOptionSurface => "full_with_option_surface",
        }
    }

    /// Typed workflow scope.
    pub const fn workflow_scope(self) -> CalibrationMarketDataWorkflowScope {
        match self {
            Self::SeasonalityOnly => CalibrationMarketDataWorkflowScope::SeasonalityOnly,
            Self::FullWithoutOptionSurface => {
                CalibrationMarketDataWorkflowScope::FullWithoutOptionSurface
            }
            Self::FullWithOptionSurface => {
                CalibrationMarketDataWorkflowScope::FullWithOptionSurface
            }
        }
    }

    /// Human-readable workflow scope.
    pub const fn label(self) -> &'static str {
        workflow_scope_label(self.workflow_scope())
    }
}

/// Resolve the workflow-scope identifier represented by a staged execution input.
pub fn workflow_scope_id(execution_input: &CalibrationStagedExecutionInput) -> WorkflowScopeId {
    if execution_input.request.selected_stages == [CalibrationStage::Seasonality] {
        return WorkflowScopeId::SeasonalityOnly;
    }

    if execution_input
        .request
        .data_inputs
        .option_surface_data_reference
        .is_some()
    {
        return WorkflowScopeId::FullWithOptionSurface;
    }

    WorkflowScopeId::FullWithoutOptionSurface
}

/// Resolve the human-readable workflow scope for a staged execution input.
pub fn workflow_scope_label_for_input(
    execution_input: &CalibrationStagedExecutionInput,
) -> &'static str {
    workflow_scope_id(execution_input).label()
}
