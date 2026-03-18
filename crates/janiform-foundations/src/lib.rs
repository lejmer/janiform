//! Low-level shared foundation primitives for Janiform.

#![forbid(unsafe_code)]

pub mod complex_math;
pub mod market_state;
pub mod numeric;
pub mod randomness;
pub mod vector_math;

pub const fn splitmix64_output_mixer(mut value: u64) -> u64 {
    value ^= value >> 30;
    value = value.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    value ^= value >> 27;
    value = value.wrapping_mul(0x94D0_49BB_1331_11EB);
    value ^ (value >> 31)
}
