use janiform_model_kernels::hawkes::{
    HawkesEventSide, HawkesKernelLoadings, HawkesKernelParameters,
};
use janiform_model_kernels::long_variance::LongVarianceParameters;
use janiform_model_kernels::market_pressure::{
    MarketPressureMeasureParameters, MarketPressureParameters, MarketPressureTransitionRates,
};
use janiform_model_kernels::mean_reversion::MeanReversionFittedParameters;

use crate::current_state::CurrentStateEstimationBatchInput;

pub(crate) const NANOS_PER_HOUR: i64 = 3_600_000_000_000;

pub(crate) fn batch_input_fixture(
    state_times_utc_nanos: Vec<i64>,
) -> CurrentStateEstimationBatchInput {
    CurrentStateEstimationBatchInput {
        kernel_parameters: HawkesKernelParameters::try_new(
            0.5,
            std::f64::consts::LN_2,
            HawkesKernelLoadings {
                plus_plus: 0.30,
                plus_minus: 0.10,
                minus_plus: 0.20,
                minus_minus: 0.40,
            },
            HawkesKernelLoadings {
                plus_plus: 0.05,
                plus_minus: 0.02,
                minus_plus: 0.03,
                minus_minus: 0.04,
            },
        )
        .expect("test kernel parameters should validate"),
        long_variance_parameters: LongVarianceParameters::neutral(),
        mean_reversion_fitted_parameters: MeanReversionFittedParameters::try_new(0.25)
            .expect("test mean-reversion parameters should validate"),
        market_pressure_parameters: test_market_pressure_parameters(),
        reference_price: 100.0,
        state_times_utc_nanos,
        observation_timestamp_utc_nanos: vec![
            0,
            NANOS_PER_HOUR,
            NANOS_PER_HOUR,
            2 * NANOS_PER_HOUR,
            3 * NANOS_PER_HOUR,
        ],
        observation_trade_prices: vec![100.0, 100.25, 100.5, 101.0, 100.75],
        observation_trade_sides: vec![
            HawkesEventSide::Buy,
            HawkesEventSide::Sell,
            HawkesEventSide::Buy,
            HawkesEventSide::Sell,
            HawkesEventSide::Buy,
        ],
    }
}

fn test_market_pressure_parameters() -> MarketPressureParameters {
    let measure_parameters = MarketPressureMeasureParameters::new(
        MarketPressureTransitionRates::new(vec![0.25], vec![0.25]),
        0.01,
    );

    MarketPressureParameters::try_new(vec![-0.5, 0.5], measure_parameters)
        .expect("test market-pressure parameters should validate")
}
