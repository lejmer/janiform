use crate::stages::mean_reversion::MeanReversionStageExecutionFailure;

pub(super) fn observed_pressure_replay_failure<T>(
    error_message: &str,
) -> Result<T, MeanReversionStageExecutionFailure> {
    Err(
        MeanReversionStageExecutionFailure::EndogenousAnchorMaterializationFailed {
            error_message: error_message.to_string(),
        },
    )
}
