use super::observed::ObservedOptionsPricingScope;

/// Human-readable scope label for fitted-model single-option pricing.
pub(super) const fn fitted_model_single_option_pricing_scope_label() -> &'static str {
    "Single vanilla option from fitted model"
}

/// Persisted workflow label for fitted-model pricing artifacts.
pub(super) const fn fitted_model_pricing_workflow_label() -> &'static str {
    "price"
}

/// Human-readable progress scope label for fitted-model single-option pricing.
pub(super) const fn fitted_model_single_option_pricing_progress_scope_label() -> &'static str {
    "Pricing"
}

/// Human-readable scope label for vanilla option grid-batch pricing.
pub(super) const fn vanilla_option_grid_batch_pricing_scope_label() -> &'static str {
    "Vanilla option grid batch"
}

/// Human-readable progress scope label for vanilla option grid-batch pricing.
pub(super) const fn vanilla_option_grid_batch_pricing_progress_scope_label() -> &'static str {
    "Pricing grid"
}

/// Human-readable scope label for vanilla option surface pricing.
pub(super) const fn vanilla_option_surface_pricing_scope_label() -> &'static str {
    "Vanilla option surface"
}

/// Human-readable progress scope label for vanilla option surface pricing.
pub(super) const fn vanilla_option_surface_pricing_progress_scope_label() -> &'static str {
    "Pricing surface"
}

/// Human-readable scope label for observed-options pricing.
pub(super) const fn observed_options_pricing_scope_label() -> &'static str {
    "Observed options"
}

/// Human-readable progress scope label for observed-options pricing.
pub(super) const fn observed_options_pricing_progress_scope_label() -> &'static str {
    "Observed options"
}

/// Persisted workflow label for observed-options pricing artifacts.
pub(super) const fn observed_options_pricing_workflow_label() -> &'static str {
    "price observed-options"
}

/// Stable token for observed-options pricing scope artifacts.
pub(super) const fn observed_options_pricing_scope_token(
    scope: ObservedOptionsPricingScope,
) -> &'static str {
    match scope {
        ObservedOptionsPricingScope::Snapshot => "snapshot",
        ObservedOptionsPricingScope::Surface => "surface",
    }
}
