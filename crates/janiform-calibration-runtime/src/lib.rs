//! Calibration runtime orchestration and staged execution.

#![forbid(unsafe_code)]

mod calendar {
    #[allow(unused_imports)]
    pub use janiform_market_time::calendar::*;
}

mod hourly_factors {
    #[allow(unused_imports)]
    pub use janiform_market_time::hourly_factors::*;
}

mod pricing {
    #[allow(unused_imports)]
    pub use janiform_engine_pricing::*;
}

mod seasonality {
    #[allow(unused_imports)]
    pub use janiform_market_time::seasonality::*;
}

mod adapters;
mod checkpointing;
mod execution_bridge;
mod market_data_loading;
mod market_data_preprocessing;
mod objective;
mod optimizer_adapter;
mod shock_mark_fitting;
mod staged_runner;
mod stages;
mod temporal;
mod diagnostics;

pub use adapters::*;
pub use checkpointing::*;
pub use execution_bridge::*;
pub use market_data_loading::*;
pub use market_data_preprocessing::*;
pub use objective::*;
pub use optimizer_adapter::*;
pub use staged_runner::*;
pub use stages::hawkes::*;
pub use stages::joint_refinement::*;
pub use stages::return_components::*;
pub use stages::seasonality::*;
