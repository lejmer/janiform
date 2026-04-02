//! Exponential-affine one-interval transform equation models and RHS evaluation.
//!
//! Equation reference from `design/janiform_compact_model_spec.tex` Section 8:
//! - `F(u_+, u_-, z_+, z_- | Y_0) = exp(C(Delta) + D(Delta) · y_0)`
//! - `y_0 = [Y++, Y+-, Y-+, Y--]`
//! - `k_+ = [alpha_++, 0, alpha_-+, 0]`
//! - `k_- = [0, alpha_+-, 0, alpha_--]`
//! - `r = [rho_++, rho_+-, rho_-+, rho_--]`
//!
//! Mapping policy:
//! - All 4D components use explicit named fields in model order `[++, +-, -+, --]`
//! - `delta` is the interval length `Delta`
//! - `c_at_delta` stores `C(Delta)`
//! - `d_at_delta` stores `D(Delta)`
//! - `transform_value` stores `F`

mod constants;
mod contracts;
mod rhs;
mod serde_impls;
mod solver;

#[cfg(test)]
mod tests;

pub use constants::*;
pub use contracts::*;
pub use rhs::*;
pub use solver::*;
