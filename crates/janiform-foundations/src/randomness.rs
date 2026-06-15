//! Deterministic simulation randomness with explicit seed and substream policy.

pub const SIMULATION_RANDOMNESS_STREAM_POLICY: &str =
    "splitmix64_sequence_with_ieee754_unit_interval_projection.v1";
pub const SIMULATION_RANDOMNESS_SUBSTREAM_DERIVATION_POLICY: &str =
    "root_seed_domain_and_index_mixed_by_splitmix64.v1";

// SplitMix64 uses this fixed Weyl increment as part of the published generator definition.
const SPLITMIX64_GAMMA: u64 = 0x9E37_79B9_7F4A_7C15;
// These fixed mixers are part of the deterministic substream derivation contract.
const SUBSTREAM_DOMAIN_MIXER: u64 = 0xA24B_AED4_963E_E407;
// This constant separates substream indices without introducing a tunable runtime policy.
const SUBSTREAM_INDEX_MIXER: u64 = 0x9FB2_1C65_1E98_DF25;

/// Explicit substream domains for seeded simulation and validation workflows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u64)]
pub enum SimulationRandomnessSubstreamDomain {
    EndogenousBuyEventCount = 1,
    EndogenousSellEventCount = 2,
    EndogenousBuyEventTimestamp = 3,
    EndogenousSellEventTimestamp = 4,
    ExogenousNewsCount = 5,
    ExogenousNewsTimestamp = 6,
    ExogenousNewsSeverity = 7,
    ExogenousNewsReturnJump = 8,
    ScaleConsistencyPairedExperiment = 9,
    ExogenousNewsMarks = 10,
    CoarseTradingTimeReturn = 11,
    CoarseStatisticsSampling = 12,
    StylizedFactsValidationSimulationPath = 13,
    ObservedOptionsPhysicalDistributionPath = 14,
    LongVarianceDiffusion = 15,
    TimestampBatchingObservation = 16,
}

impl SimulationRandomnessSubstreamDomain {
    pub const fn domain_tag(self) -> u64 {
        self as u64
    }
}

/// Deterministic substream seed derivation from one root seed.
///
/// Policy:
/// - root seed is mixed with the explicit domain tag and substream index
/// - splitmix64 output mixer produces the derived substream seed
pub const fn derive_simulation_substream_seed(
    root_seed: u64,
    substream_domain: SimulationRandomnessSubstreamDomain,
    substream_index: u64,
) -> u64 {
    splitmix64_output_mixer(
        root_seed
            ^ substream_domain
                .domain_tag()
                .wrapping_mul(SUBSTREAM_DOMAIN_MIXER)
            ^ substream_index.wrapping_mul(SUBSTREAM_INDEX_MIXER),
    )
}

/// Uniform randomness stream contract used by simulation code paths.
pub trait SimulationRandomnessStream {
    fn next_u64(&mut self) -> u64;

    fn next_unit_interval_f64(&mut self) -> f64 {
        // IEEE-754 `f64` stores 53 bits of mantissa, so this is the exact projection factor.
        const UNIT_INTERVAL_NORMALIZATION: f64 = 1.0 / ((1_u64 << 53) as f64);
        let most_significant_bits = self.next_u64() >> 11;
        (most_significant_bits as f64) * UNIT_INTERVAL_NORMALIZATION
    }
}

/// Adapter that exposes a simulation randomness stream as a `rand` RNG.
pub struct SimulationRandomnessRng<'a, R>
where
    R: SimulationRandomnessStream + ?Sized,
{
    randomness_stream: &'a mut R,
}

impl<'a, R> SimulationRandomnessRng<'a, R>
where
    R: SimulationRandomnessStream + ?Sized,
{
    /// Wrap a simulation stream for APIs that expect `rand::RngCore`.
    pub fn new(randomness_stream: &'a mut R) -> Self {
        Self { randomness_stream }
    }
}

impl<R> rand::RngCore for SimulationRandomnessRng<'_, R>
where
    R: SimulationRandomnessStream + ?Sized,
{
    fn next_u32(&mut self) -> u32 {
        self.randomness_stream.next_u64() as u32
    }

    fn next_u64(&mut self) -> u64 {
        self.randomness_stream.next_u64()
    }

    fn fill_bytes(&mut self, destination: &mut [u8]) {
        let mut exact_chunks = destination.chunks_exact_mut(std::mem::size_of::<u64>());

        for chunk in &mut exact_chunks {
            chunk.copy_from_slice(&self.next_u64().to_le_bytes());
        }

        let remainder = exact_chunks.into_remainder();
        if remainder.is_empty() {
            return;
        }

        let next_bytes = self.next_u64().to_le_bytes();
        remainder.copy_from_slice(&next_bytes[..remainder.len()]);
    }

    fn try_fill_bytes(&mut self, destination: &mut [u8]) -> Result<(), rand::Error> {
        self.fill_bytes(destination);

        Ok(())
    }
}

/// Draw one standard normal variate using the shared simulation randomness stream.
pub fn sample_standard_normal_draw<R>(randomness_stream: &mut R) -> f64
where
    R: SimulationRandomnessStream + ?Sized,
{
    let first_uniform_draw = randomness_stream.next_unit_interval_f64();
    let second_uniform_draw = randomness_stream.next_unit_interval_f64();

    standard_normal_draw_from_unit_interval_draws(first_uniform_draw, second_uniform_draw)
}

/// Map two unit-interval draws into one standard normal variate.
pub fn standard_normal_draw_from_unit_interval_draws(
    first_unit_interval_draw: f64,
    second_unit_interval_draw: f64,
) -> f64 {
    let first_uniform_draw = first_unit_interval_draw.clamp(f64::MIN_POSITIVE, 1.0 - f64::EPSILON);
    let second_uniform_draw = second_unit_interval_draw.clamp(0.0, 1.0 - f64::EPSILON);

    let radius = (-2.0 * first_uniform_draw.ln()).sqrt();
    let angle = 2.0 * std::f64::consts::PI * second_uniform_draw;

    radius * angle.cos()
}

/// Factory abstraction used to derive deterministic domain substreams from one root seed.
pub trait SimulationRandomnessFactory {
    type Stream: SimulationRandomnessStream;

    fn create_substream(
        &self,
        root_seed: u64,
        substream_domain: SimulationRandomnessSubstreamDomain,
        substream_index: u64,
    ) -> Self::Stream;
}

/// SplitMix64 stream implementation for deterministic simulation randomness.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SplitMix64SimulationRandomnessStream {
    state: u64,
}

impl SplitMix64SimulationRandomnessStream {
    pub const fn from_seed(seed: u64) -> Self {
        Self { state: seed }
    }
}

impl SimulationRandomnessStream for SplitMix64SimulationRandomnessStream {
    fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(SPLITMIX64_GAMMA);
        splitmix64_output_mixer(self.state)
    }
}

/// Default splitmix64 factory for seeded simulation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SplitMix64SimulationRandomnessFactory;

impl SimulationRandomnessFactory for SplitMix64SimulationRandomnessFactory {
    type Stream = SplitMix64SimulationRandomnessStream;

    fn create_substream(
        &self,
        root_seed: u64,
        substream_domain: SimulationRandomnessSubstreamDomain,
        substream_index: u64,
    ) -> Self::Stream {
        let derived_seed =
            derive_simulation_substream_seed(root_seed, substream_domain, substream_index);
        SplitMix64SimulationRandomnessStream::from_seed(derived_seed)
    }
}

/// Deterministic splitmix64 output mixer used for seed derivation workflows.
pub const fn splitmix64_output_mixer(mut value: u64) -> u64 {
    value ^= value >> 30;
    value = value.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    value ^= value >> 27;
    value = value.wrapping_mul(0x94D0_49BB_1331_11EB);
    value ^ (value >> 31)
}

#[cfg(test)]
mod tests {
    use super::{
        SIMULATION_RANDOMNESS_STREAM_POLICY, SIMULATION_RANDOMNESS_SUBSTREAM_DERIVATION_POLICY,
        SimulationRandomnessFactory, SimulationRandomnessRng, SimulationRandomnessStream,
        SimulationRandomnessSubstreamDomain, SplitMix64SimulationRandomnessFactory,
        SplitMix64SimulationRandomnessStream, derive_simulation_substream_seed,
    };
    use rand::RngCore;

    #[test]
    fn splitmix64_stream_reports_policy_constants() {
        assert_eq!(
            SIMULATION_RANDOMNESS_STREAM_POLICY,
            "splitmix64_sequence_with_ieee754_unit_interval_projection.v1"
        );
        assert_eq!(
            SIMULATION_RANDOMNESS_SUBSTREAM_DERIVATION_POLICY,
            "root_seed_domain_and_index_mixed_by_splitmix64.v1"
        );
    }

    #[test]
    fn substream_seed_derivation_is_stable_for_fixed_inputs() {
        let derived_seed = derive_simulation_substream_seed(
            19_942,
            SimulationRandomnessSubstreamDomain::EndogenousBuyEventCount,
            7,
        );

        assert_eq!(derived_seed, 1_289_518_677_619_629_306);
    }

    #[test]
    fn substream_seed_derivation_changes_with_domain_and_index() {
        let root_seed = 11_007_u64;

        let count_seed = derive_simulation_substream_seed(
            root_seed,
            SimulationRandomnessSubstreamDomain::EndogenousBuyEventCount,
            0,
        );
        let sell_count_seed = derive_simulation_substream_seed(
            root_seed,
            SimulationRandomnessSubstreamDomain::EndogenousSellEventCount,
            0,
        );
        let next_interval_count_seed = derive_simulation_substream_seed(
            root_seed,
            SimulationRandomnessSubstreamDomain::EndogenousBuyEventCount,
            1,
        );

        assert_ne!(count_seed, sell_count_seed);
        assert_ne!(count_seed, next_interval_count_seed);
    }

    #[test]
    fn substreams_repeat_for_identical_seed_domain_and_index() {
        let randomness_factory = SplitMix64SimulationRandomnessFactory;

        let mut first_stream = randomness_factory.create_substream(
            723,
            SimulationRandomnessSubstreamDomain::ExogenousNewsSeverity,
            4,
        );
        let mut second_stream = randomness_factory.create_substream(
            723,
            SimulationRandomnessSubstreamDomain::ExogenousNewsSeverity,
            4,
        );

        let first_draws = [
            first_stream.next_u64(),
            first_stream.next_u64(),
            first_stream.next_u64(),
        ];
        let second_draws = [
            second_stream.next_u64(),
            second_stream.next_u64(),
            second_stream.next_u64(),
        ];

        assert_eq!(first_draws, second_draws);
    }

    #[test]
    fn simulation_randomness_rng_delegates_to_wrapped_stream() {
        let mut reference_stream = SplitMix64SimulationRandomnessStream::from_seed(17);
        let mut wrapped_stream = SplitMix64SimulationRandomnessStream::from_seed(17);
        let mut rng = SimulationRandomnessRng::new(&mut wrapped_stream);

        assert_eq!(rng.next_u64(), reference_stream.next_u64());
        assert_eq!(rng.next_u64(), reference_stream.next_u64());
    }
}
