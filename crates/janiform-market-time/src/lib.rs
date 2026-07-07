//! Shared market-time infrastructure for Janiform.

#![forbid(unsafe_code)]

pub mod business_time_clock;
pub mod calendar;
pub mod interval_factor_grids;
pub mod time {
    pub use janiform_foundations::time::NANOS_PER_HOUR;
}
