use crate::error::WorkflowError;

pub(super) fn validation_error(field: &'static str, message: impl Into<String>) -> WorkflowError {
    WorkflowError::SimulationValidationFailed {
        field,
        message: message.into(),
    }
}

pub(super) fn interval_span_nanos(
    interval_duration_nanos: i64,
    interval_count: usize,
) -> Result<i64, WorkflowError> {
    let interval_count = i64::try_from(interval_count).map_err(|_| {
        validation_error(
            "interval_duration_nanos",
            "interval count does not fit in signed 64-bit arithmetic",
        )
    })?;

    interval_duration_nanos
        .checked_mul(interval_count)
        .ok_or_else(|| {
            validation_error(
                "interval_duration_nanos",
                "interval span overflowed signed 64-bit nanosecond arithmetic",
            )
        })
}
