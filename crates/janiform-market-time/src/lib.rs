//! Shared market-time infrastructure for Janiform.

#![forbid(unsafe_code)]

pub mod calendar;
pub mod hourly_factors;
pub mod seasonality;
pub mod time {
    pub use janiform_foundations::time::ONE_HOUR_NANOS;
}
