use janiform::calibration::market_data::{
    MarketDataInputFileReference, MarketDataNormalizationPolicy, MarketDataTickDataColumnMapping,
    infer_market_data_tick_price_input_policy,
};

use crate::error::WorkflowError;

use super::super::validation::pricing_validation_error;

pub(super) fn observed_tick_normalization_policy(
    tick_data_source: &MarketDataInputFileReference,
    tick_data_column_mapping: &MarketDataTickDataColumnMapping,
    normalization_policy: &MarketDataNormalizationPolicy,
) -> Result<MarketDataNormalizationPolicy, WorkflowError> {
    if normalization_policy.tick_price_input_policy.is_some() {
        // Intentional copy because the observed-pricing workflow resolves an
        // effective boundary policy without mutating the request contract.
        return Ok(normalization_policy.clone());
    }

    let inferred_policy =
        infer_market_data_tick_price_input_policy(tick_data_source, tick_data_column_mapping)
            .map_err(|source| pricing_validation_error("tick_data_source", source.to_string()))?
            .ok_or_else(|| {
                pricing_validation_error(
                    "tick_data.price",
                    "could not infer a tick price input policy from the supplied source",
                )
            })?;

    // Intentional copy because the observed-pricing workflow injects a resolved
    // tick price policy while preserving the original request payload.
    let mut resolved_policy = normalization_policy.clone();
    resolved_policy.tick_price_input_policy = Some(inferred_policy);

    Ok(resolved_policy)
}
