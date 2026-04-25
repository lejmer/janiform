//! Core Rust library for Janiform's stochastic process models.

#![forbid(unsafe_code)]

pub mod affine_oracle;
pub mod benchmarking;
pub mod calendar;
pub mod calibration;
pub mod complex_math;
pub mod hawkes;
pub mod hourly_factors;
pub mod news;
pub mod nig;
pub mod pricing;
pub mod randomness;
pub mod return_decomposition;
pub mod risk_neutral;
pub mod seasonality;
pub mod simulation;
pub mod sufficient_statistics;
pub mod verification;
