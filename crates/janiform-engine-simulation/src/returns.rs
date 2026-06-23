use janiform_foundations::randomness::SimulationRandomnessStream;
use janiform_model_kernels::nig::{
    NigBuySellDriverParameters, NigTradingTimeIncrementContributions,
    NigTradingTimeIncrementSamplingError,
};
use janiform_model_kernels::return_decomposition::{
    ReturnDecomposition, ReturnDecompositionEdgeMode,
};

pub use janiform_model_kernels::return_decomposition::sample_nig_trading_time_increment_with_edge_mode;

/// Sample one trading-time return increment for simulation through the shared NIG driver API.
pub fn sample_nig_trading_time_increment<R>(
    driver_parameters: NigBuySellDriverParameters,
    return_decomposition: ReturnDecomposition,
    randomness_stream: &mut R,
) -> Result<NigTradingTimeIncrementContributions, NigTradingTimeIncrementSamplingError>
where
    R: SimulationRandomnessStream,
{
    sample_nig_trading_time_increment_with_edge_mode(
        driver_parameters,
        return_decomposition,
        ReturnDecompositionEdgeMode::Disabled,
        randomness_stream,
    )
}

#[cfg(test)]
mod tests {
    use janiform_foundations::randomness::SplitMix64SimulationRandomnessStream;
    use janiform_model_kernels::nig::{
        NIG_TRADING_TIME_INCREMENT_COMPOSITION_POLICY, NigSharedPlusShiftConfiguration,
    };
    use janiform_model_kernels::return_decomposition::{
        ReturnDecomposition, ReturnDecompositionEdgeMode, ReturnDecompositionInput,
    };

    use super::{
        sample_nig_trading_time_increment, sample_nig_trading_time_increment_with_edge_mode,
    };

    #[test]
    fn trading_time_assembler_buy_only_edge_mode_keeps_sell_and_gap_components_zero() {
        let driver_parameters = valid_nig_driver_parameters();

        let decomposition = ReturnDecomposition::try_new(ReturnDecompositionInput {
            a_plus: 0.9,
            a_minus: 0.7,
            open_jump: 0.0,
            closed_gap: -0.2,
        })
        .expect("buy-only decomposition should be valid");

        let mut randomness_stream = SplitMix64SimulationRandomnessStream::from_seed(42_101);

        let contributions = sample_nig_trading_time_increment_with_edge_mode(
            driver_parameters,
            decomposition,
            ReturnDecompositionEdgeMode::BuyOnly,
            &mut randomness_stream,
        )
        .expect("buy-only assembly should sample successfully");

        assert_eq!(
            contributions.policy,
            NIG_TRADING_TIME_INCREMENT_COMPOSITION_POLICY
        );
        assert_float_close(contributions.sell_side_increment, 0.0);
        assert_float_close(
            contributions.combined_increment,
            contributions.buy_side_increment,
        );
    }

    #[test]
    fn trading_time_assembler_sell_only_edge_mode_applies_minus_sign_to_combined_increment() {
        let driver_parameters = valid_nig_driver_parameters();

        let decomposition = ReturnDecomposition::try_new(ReturnDecompositionInput {
            a_plus: 0.6,
            a_minus: 0.9,
            open_jump: 0.0,
            closed_gap: -0.1,
        })
        .expect("sell-only decomposition should be valid");

        let mut randomness_stream = SplitMix64SimulationRandomnessStream::from_seed(42_102);

        let contributions = sample_nig_trading_time_increment_with_edge_mode(
            driver_parameters,
            decomposition,
            ReturnDecompositionEdgeMode::SellOnly,
            &mut randomness_stream,
        )
        .expect("sell-only assembly should sample successfully");

        assert_eq!(
            contributions.policy,
            NIG_TRADING_TIME_INCREMENT_COMPOSITION_POLICY
        );
        assert_float_close(contributions.buy_side_increment, 0.0);
        assert_float_close(
            contributions.combined_increment,
            -contributions.sell_side_increment,
        );
    }

    #[test]
    fn trading_time_assembler_gap_only_edge_mode_zeroes_trading_components() {
        let driver_parameters = valid_nig_driver_parameters();

        let decomposition = ReturnDecomposition::try_new(ReturnDecompositionInput {
            a_plus: 0.9,
            a_minus: 0.8,
            open_jump: 0.0,
            closed_gap: -0.3,
        })
        .expect("gap-only decomposition should be valid");

        let mut randomness_stream = SplitMix64SimulationRandomnessStream::from_seed(42_104);

        let contributions = sample_nig_trading_time_increment_with_edge_mode(
            driver_parameters,
            decomposition,
            ReturnDecompositionEdgeMode::ClosedGapOnly,
            &mut randomness_stream,
        )
        .expect("gap-only assembly should sample successfully");

        assert_eq!(
            contributions.policy,
            NIG_TRADING_TIME_INCREMENT_COMPOSITION_POLICY
        );
        assert_float_close(contributions.buy_side_increment, 0.0);
        assert_float_close(contributions.sell_side_increment, 0.0);
        assert_float_close(contributions.combined_increment, 0.0);
    }

    #[test]
    fn trading_time_assembler_is_seed_deterministic() {
        let driver_parameters = valid_nig_driver_parameters();

        let decomposition = ReturnDecomposition::try_new(ReturnDecompositionInput {
            a_plus: 0.8,
            a_minus: 0.5,
            open_jump: 0.0,
            closed_gap: 0.0,
        })
        .expect("mixed decomposition should be valid");

        let mut first_stream = SplitMix64SimulationRandomnessStream::from_seed(42_103);
        let mut second_stream = SplitMix64SimulationRandomnessStream::from_seed(42_103);

        let first_contributions =
            sample_nig_trading_time_increment(driver_parameters, decomposition, &mut first_stream)
                .expect("first seeded assembly should sample successfully");

        let second_contributions =
            sample_nig_trading_time_increment(driver_parameters, decomposition, &mut second_stream)
                .expect("second seeded assembly should sample successfully");

        assert_eq!(first_contributions, second_contributions);
    }

    fn valid_nig_driver_parameters() -> janiform_model_kernels::nig::NigBuySellDriverParameters {
        NigSharedPlusShiftConfiguration::try_new(2.3, -0.8, 0.7, 0.1, -0.2)
            .expect("test shared-plus-shift payload should construct")
            .derive_driver_parameters()
            .expect("test shared-plus-shift payload should derive drivers")
    }

    fn assert_float_close(observed: f64, expected: f64) {
        let tolerance = 1.0e-12;
        assert!(
            (observed - expected).abs() <= tolerance,
            "observed {observed} differs from expected {expected} by more than {tolerance}"
        );
    }
}
