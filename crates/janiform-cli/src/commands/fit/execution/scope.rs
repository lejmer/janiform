use janiform::calibration::{
    CalibrationMarketDataWorkflowScope, CalibrationStage, CalibrationStagedExecutionInput,
};

use crate::commands::fit::labels::workflow_scope_label;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum WorkflowScopeId {
    SeasonalityOnly,
    FullWithoutOptionSurface,
    FullWithOptionSurface,
}

impl WorkflowScopeId {
    pub(super) const fn as_str(self) -> &'static str {
        match self {
            Self::SeasonalityOnly => "seasonality_only",
            Self::FullWithoutOptionSurface => "full_without_option_surface",
            Self::FullWithOptionSurface => "full_with_option_surface",
        }
    }

    pub(super) const fn workflow_scope(self) -> CalibrationMarketDataWorkflowScope {
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

    pub(super) fn label(self) -> &'static str {
        workflow_scope_label(self.workflow_scope())
    }
}

pub(super) fn workflow_scope_id(
    execution_input: &CalibrationStagedExecutionInput,
) -> WorkflowScopeId {
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

pub(super) fn workflow_scope_label_for_input(
    execution_input: &CalibrationStagedExecutionInput,
) -> &'static str {
    workflow_scope_id(execution_input).label()
}
