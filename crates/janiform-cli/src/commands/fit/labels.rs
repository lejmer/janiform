use crate::commands::options::WorkflowScope;

pub(crate) fn optional_label(value: Option<&str>) -> String {
    value.unwrap_or("none").to_string()
}

pub(crate) fn workflow_scope_argument_label(scope: WorkflowScope) -> String {
    match scope {
        WorkflowScope::Full => "Full",
        WorkflowScope::SeasonalityOnly => "Seasonality only",
        WorkflowScope::FullWithoutOptionSurface => "Full without option surface",
        WorkflowScope::FullWithOptionSurface => "Full with option surface",
    }
    .to_string()
}
