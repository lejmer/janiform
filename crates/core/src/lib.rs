//! Core Rust library for Janiform's stochastic process models.

#![forbid(unsafe_code)]

#[path = "facade/affine_oracle.rs"]
pub mod affine_oracle;
pub mod benchmarking;
pub mod calendar;
pub mod calibration;
#[path = "facade/complex_math.rs"]
pub mod complex_math;
#[path = "facade/hawkes.rs"]
pub mod hawkes;
pub mod hourly_factors;
#[path = "facade/news.rs"]
pub mod news;
#[path = "facade/nig.rs"]
pub mod nig;
pub mod pricing;
#[path = "facade/randomness.rs"]
pub mod randomness;
#[path = "facade/return_decomposition.rs"]
pub mod return_decomposition;
pub mod risk;
#[path = "facade/risk_neutral.rs"]
pub mod risk_neutral;
pub mod seasonality;
pub mod simulation;
pub mod verification;
