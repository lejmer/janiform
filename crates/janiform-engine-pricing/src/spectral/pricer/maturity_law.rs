use super::{ComplexScalar, NodePriceSharedPlusShiftComplexSensitivity};

#[derive(Debug, Clone)]
pub(in crate::spectral::pricer) struct SolvedMaturityLaw {
    pub(in crate::spectral::pricer) maturity_interval: f64,
    pub(in crate::spectral::pricer) terminal_anchor_value: f64,
    pub(in crate::spectral::pricer) deviation_lower_bound: f64,
    pub(in crate::spectral::pricer) deviation_upper_bound: f64,
    pub(in crate::spectral::pricer) node_count: usize,
    pub(in crate::spectral::pricer) observed_log_price: f64,
    pub(in crate::spectral::pricer) characteristic_function_values: Vec<ComplexScalar>,
    pub(in crate::spectral::pricer) characteristic_function_sensitivities:
        Option<Vec<NodePriceSharedPlusShiftComplexSensitivity>>,
    pub(in crate::spectral::pricer) interval_summaries:
        Vec<super::SpectralPricingIntervalDiagnostics>,
}
