pub mod return_decomposition_failure_diagnostics;
pub mod return_decomposition_sign_fixture;

use janiform_model_kernels::market_pressure::MarketPressureParameters;

#[allow(unused_imports)]
pub use janiform_test_support::{
    hawkes_failure_diagnostics, hawkes_invariant_catalog, scale_consistency_regression_fixture,
    stylized_fact_regression_fixture, validation_fixture,
};

#[allow(dead_code)]
pub fn neutral_market_pressure_parameters() -> MarketPressureParameters {
    MarketPressureParameters::no_anchor_impact()
}
