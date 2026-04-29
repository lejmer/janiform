//! Validation workflow observer and coordination contracts.

use super::progress::StylizedFactsValidationProgressEvent;

/// Observer called as the stylized-facts validation workflow advances.
pub type StylizedFactsValidationWorkflowObserver<'a> =
    dyn FnMut(StylizedFactsValidationProgressEvent) + 'a;
