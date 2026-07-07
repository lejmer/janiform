//! Shared validation workflow contracts.

mod artifacts;
mod empirical;
pub mod execution;
mod labels;
mod metrics;
pub mod output;
pub mod progress;
pub mod request;
mod request_resolution;
mod simulation;
mod validation_error;
pub mod workflow;

pub use execution::{build_stylized_facts_validation_plan, run_stylized_facts_validation_workflow};
pub use labels::{
    STYLIZED_FACTS_VALIDATION_STAGE_LABELS, stylized_facts_validation_phase_label,
    stylized_facts_validation_scope_label,
};
pub use output::{
    STYLIZED_FACTS_VALIDATION_PLAN_SCHEMA_VERSION, StylizedFactsValidationCacheLevel,
    StylizedFactsValidationDecisionMode, StylizedFactsValidationExecutionMode,
    StylizedFactsValidationIntervalPlan, StylizedFactsValidationPlan,
    StylizedFactsValidationWorkflowOutput,
};
pub use progress::{StylizedFactsValidationProgressEvent, StylizedFactsValidationProgressPhase};
pub use request::StylizedFactsValidationWorkflowRequest;
pub use workflow::StylizedFactsValidationWorkflowObserver;
