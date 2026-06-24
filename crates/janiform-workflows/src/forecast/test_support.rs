use janiform_foundations::complex_math::ComplexScalar;

pub(super) fn synthetic_terminal_law(
    characteristic_function_values: Vec<ComplexScalar>,
) -> janiform_engine_pricing::AffineForecastTerminalLaw {
    synthetic_terminal_law_on_support(characteristic_function_values, -1.0, 1.0)
}

pub(super) fn synthetic_terminal_law_on_support(
    characteristic_function_values: Vec<ComplexScalar>,
    terminal_log_return_lower_bound: f64,
    terminal_log_return_upper_bound: f64,
) -> janiform_engine_pricing::AffineForecastTerminalLaw {
    janiform_engine_pricing::AffineForecastTerminalLaw {
        maturity_interval: 1.0,
        observed_log_price: 0.0,
        expected_log_return: 0.0,
        log_expected_price_ratio: 0.0,
        terminal_log_return_lower_bound,
        terminal_log_return_upper_bound,
        terminal_log_price_lower_bound: terminal_log_return_lower_bound,
        terminal_log_price_upper_bound: terminal_log_return_upper_bound,
        node_count: characteristic_function_values.len(),
        characteristic_function_values,
    }
}
