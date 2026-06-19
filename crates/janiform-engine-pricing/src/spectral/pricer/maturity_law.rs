use super::{ComplexScalar, NodePriceSharedPlusShiftComplexSensitivity};

#[derive(Debug, Clone)]
pub(crate) struct SolvedMaturityLaw {
    pub(crate) maturity_interval: f64,
    pub(crate) terminal_anchor_value: f64,
    pub(crate) deviation_lower_bound: f64,
    pub(crate) deviation_upper_bound: f64,
    pub(crate) node_count: usize,
    pub(crate) observed_log_price: f64,
    pub(crate) expected_log_return: f64,
    pub(crate) characteristic_function_values: Vec<ComplexScalar>,
    pub(crate) characteristic_function_sensitivities:
        Option<Vec<NodePriceSharedPlusShiftComplexSensitivity>>,
    pub(crate) terminal_affine_measure_change:
        Option<super::TerminalAffineMeasureChangeDiagnostics>,
    pub(crate) interval_summaries: Vec<super::SpectralPricingIntervalDiagnostics>,
}
