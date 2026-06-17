use std::error::Error;
use std::fmt::{Display, Formatter};

use janiform_model_kernels::return_clock::SideSourceActivityDecomposition;

use super::contracts::ReturnComponentsObservedOpenSessionReturn;

pub(crate) fn open_session_returns_with_source_activities(
    observed_returns: &[ReturnComponentsObservedOpenSessionReturn],
    source_activities: &[SideSourceActivityDecomposition],
) -> Result<
    Vec<ReturnComponentsObservedOpenSessionReturn>,
    ReturnComponentsSourceActivityOverlayError,
> {
    let observed_return_count = observed_returns.len();
    let source_activity_count = source_activities.len();

    if source_activity_count != observed_return_count {
        return Err(ReturnComponentsSourceActivityOverlayError::CountMismatch {
            observed_return_count,
            source_activity_count,
        });
    }

    Ok(observed_returns
        .iter()
        .copied()
        .zip(source_activities.iter().copied())
        .map(|(observation, source_activity)| observation.with_source_activity(source_activity))
        .collect())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReturnComponentsSourceActivityOverlayError {
    CountMismatch {
        observed_return_count: usize,
        source_activity_count: usize,
    },
}

impl Display for ReturnComponentsSourceActivityOverlayError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CountMismatch {
                observed_return_count,
                source_activity_count,
            } => write!(
                formatter,
                "source activity count {source_activity_count} must match observed return count {observed_return_count}"
            ),
        }
    }
}

impl Error for ReturnComponentsSourceActivityOverlayError {}
