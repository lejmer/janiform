use crate::stages::joint_refinement::execution::JointRefinementActiveParameterLayout;

pub(super) fn test_active_parameter_layout() -> JointRefinementActiveParameterLayout {
    JointRefinementActiveParameterLayout {
        hawkes_block: vec![0, 1, 2, 3],
        nig_block: vec![4, 5, 6, 7],
        return_clock_block: vec![8, 9],
        long_variance_block: vec![10, 11, 12, 13, 14],
        option_pricing_kernel_block: vec![15, 16, 17],
        coupling_block: Vec::new(),
        log_stationary_event_rate_scale_index: 0,
        log_beta_scale_index: 1,
        log_self_excitation_scale_index: 2,
        log_cross_excitation_scale_index: 3,
        log_buy_excitation_return_clock_scale_index: 8,
        log_sell_excitation_return_clock_scale_index: 9,
        log_long_variance_mean_reversion_rate_scale_index: 10,
        long_variance_vol_of_vol_index: 11,
        long_variance_return_energy_loading_index: 12,
        long_variance_buy_event_loading_index: 13,
        long_variance_sell_event_loading_index: 14,
        option_pricing_log_buy_activity_premium_index: 15,
        option_pricing_log_sell_activity_premium_index: 16,
        option_pricing_pressure_signed_imbalance_exposure_risk_price_index: 17,
        known_activity_scale_index: None,
        surprise_activity_scale_index: None,
        surprise_baseline_scale_index: None,
    }
}
