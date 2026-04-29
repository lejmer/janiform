//! Low-level shared foundation primitives for Janiform.

#![forbid(unsafe_code)]

pub mod complex_math;
pub mod identifiers;
pub mod json;
pub mod low_discrepancy;
pub mod market_state;
pub mod matrix2;
pub mod numeric;
pub mod paths;
pub mod randomness;
pub mod strings;
pub mod time;
pub mod vector_math;

pub use randomness::splitmix64_output_mixer;
