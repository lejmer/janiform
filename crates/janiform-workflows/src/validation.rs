//! Shared validation workflow contracts.

mod artifacts;
mod empirical;
pub mod execution;
mod metrics;
pub mod output;
pub mod progress;
pub mod request;
mod request_resolution;
mod simulation;
pub mod workflow;

pub use execution::{build_stylized_facts_validation_plan, run_stylized_facts_validation_workflow};
pub use output::{
    STYLIZED_FACTS_VALIDATION_PLAN_SCHEMA_VERSION, StylizedFactsValidationCacheLevel,
    StylizedFactsValidationDecisionMode, StylizedFactsValidationExecutionMode,
    StylizedFactsValidationIntervalPlan, StylizedFactsValidationPlan,
    StylizedFactsValidationWorkflowOutput,
};
pub use progress::{StylizedFactsValidationProgressEvent, StylizedFactsValidationProgressPhase};
pub use request::StylizedFactsValidationWorkflowRequest;
pub use workflow::StylizedFactsValidationWorkflowObserver;
