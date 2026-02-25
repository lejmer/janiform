//! Core Rust library for Janiform's stochastic process models.

#![forbid(unsafe_code)]

pub mod affine_oracle;
pub mod calendar;
pub mod calibration;
pub mod complex_math;
pub mod hawkes;
pub mod hourly_factor_provider;
pub mod hourly_factors;
pub mod news;
pub mod nig;
pub mod pricing;
pub mod randomness;
pub mod return_decomposition;
pub mod seasonality;
pub mod simulation;
pub mod validation;
