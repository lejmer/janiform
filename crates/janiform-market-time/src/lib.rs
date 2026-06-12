//! Shared market-time infrastructure for Janiform.

#![forbid(unsafe_code)]

pub mod business_time_clock;
pub mod calendar;
pub mod hourly_factors;
pub mod time {
    pub use janiform_foundations::time::NANOS_PER_HOUR;
}
