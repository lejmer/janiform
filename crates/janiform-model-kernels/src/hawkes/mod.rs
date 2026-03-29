//! Canonical bivariate Hawkes state and kernel parameter types.
//!
//! The de-seasonalized buy and sell intensities are:
//! `tilde_lambda_plus = lambda_bar + Y++ + Y+-`
//! `tilde_lambda_minus = lambda_bar + Y-+ + Y--`
//!
//! The sign mapping is:
//! - `plus` means buy (`+`)
//! - `minus` means sell (`-`)
//!
//! For component notation `X_ij`:
//! - first sign `i` is target intensity (`+` buy, `-` sell)
//! - second sign `j` is source event sign (`+` buy, `-` sell)
//!
//! This module uses explicit named fields for all 4 components to avoid
//! tuple or matrix-index ambiguity in public core interfaces.

mod analytics;
mod loadings;
mod parameters;
mod state;

pub use analytics::*;
pub use loadings::*;
pub use parameters::*;
pub use state::*;

#[cfg(test)]
mod tests;
