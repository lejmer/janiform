mod coordinates;
mod pricer;
mod sensitivity;

pub(crate) use pricer::run_single_option_pricing;
pub use pricer::run_vanilla_option_grid_batch_pricing_with_observer_and_theta_warm_start_state;
pub(crate) use pricer::{
    TerminalAffineExpectedLogReturnInput, TerminalAffineExplicitLogPriceDomain,
    TerminalAffineMaturitySolveInput, TerminalAffineMeasurePolicy,
    TerminalAffineSpectralDomainInput, solve_terminal_affine_expected_log_return,
    solve_terminal_affine_maturity_law,
};
