use tracing::error;

use crate::error::SeededTickSimulationRunError;

pub(crate) fn try_reserve_simulation_buffer<T>(
    buffer_name: &'static str,
    requested_capacity: usize,
) -> Result<Vec<T>, SeededTickSimulationRunError> {
    let mut values = Vec::new();

    values.try_reserve(requested_capacity).map_err(|source| {
        error!(
            buffer_name,
            requested_capacity,
            error = %source,
            "failed to reserve simulation buffer"
        );
        SeededTickSimulationRunError::BufferAllocationFailed {
            buffer_name,
            requested_capacity,
            detail: source.to_string(),
        }
    })?;

    Ok(values)
}

pub(crate) fn ensure_simulation_buffer_spare_capacity<T>(
    buffer_name: &'static str,
    values: &mut Vec<T>,
    additional_items: usize,
) -> Result<(), SeededTickSimulationRunError> {
    let spare_capacity = values.capacity().saturating_sub(values.len());
    if spare_capacity >= additional_items {
        return Ok(());
    }

    let requested_capacity = additional_items.saturating_sub(spare_capacity);

    values.try_reserve(requested_capacity).map_err(|source| {
        error!(
            buffer_name,
            requested_capacity,
            current_len = values.len(),
            current_capacity = values.capacity(),
            error = %source,
            "failed to grow simulation buffer"
        );
        SeededTickSimulationRunError::BufferAllocationFailed {
            buffer_name,
            requested_capacity,
            detail: format!(
                "current_len={}, current_capacity={}, source={source}",
                values.len(),
                values.capacity(),
            ),
        }
    })
}

pub(crate) fn push_simulation_buffer_value<T>(
    buffer_name: &'static str,
    values: &mut Vec<T>,
    value: T,
) -> Result<(), SeededTickSimulationRunError> {
    ensure_simulation_buffer_spare_capacity(buffer_name, values, 1)?;

    values.push(value);

    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::error::SeededTickSimulationRunError;

    use super::{ensure_simulation_buffer_spare_capacity, try_reserve_simulation_buffer};

    #[test]
    fn allocation_failures_are_returned_as_structured_errors() {
        let error = try_reserve_simulation_buffer::<u8>("test_buffer", usize::MAX)
            .expect_err("oversized reserve should fail");

        match error {
            SeededTickSimulationRunError::BufferAllocationFailed {
                buffer_name,
                requested_capacity,
                detail,
            } => {
                assert_eq!(buffer_name, "test_buffer");
                assert_eq!(requested_capacity, usize::MAX);
                assert!(!detail.is_empty());
            }
            _ => panic!("expected buffer allocation failure"),
        }
    }

    #[test]
    fn buffer_growth_failures_are_returned_as_structured_errors() {
        let mut values = Vec::<u8>::new();

        let error =
            ensure_simulation_buffer_spare_capacity("growth_buffer", &mut values, usize::MAX)
                .expect_err("oversized growth should fail");

        match error {
            SeededTickSimulationRunError::BufferAllocationFailed {
                buffer_name,
                requested_capacity,
                detail,
            } => {
                assert_eq!(buffer_name, "growth_buffer");
                assert_eq!(requested_capacity, usize::MAX);
                assert!(!detail.is_empty());
            }
            _ => panic!("expected buffer allocation failure"),
        }
    }
}
