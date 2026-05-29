mod identity;
mod parity;
mod resolution;
mod roll_adjustment;
mod source;
#[cfg(test)]
mod tests;

pub use resolution::{
    PriceCoordinateResolutionInput, PriceCoordinateResolutionOutput, resolve_price_coordinates,
};
