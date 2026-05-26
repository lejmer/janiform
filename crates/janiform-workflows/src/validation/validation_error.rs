use crate::error::WorkflowError;

pub(super) fn stylized_facts_validation_error(message: impl Into<String>) -> WorkflowError {
    WorkflowError::ValidationInputFailed {
        field: "validate stylized-facts",
        message: message.into(),
    }
}
