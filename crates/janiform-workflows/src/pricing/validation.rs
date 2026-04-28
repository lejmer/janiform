use crate::error::WorkflowError;

pub(super) fn pricing_validation_error(
    field: &'static str,
    message: impl Into<String>,
) -> WorkflowError {
    WorkflowError::PricingValidationFailed {
        field,
        message: message.into(),
    }
}
