//! Deterministic simulation randomness with explicit seed and substream policy.

pub const SIMULATION_RANDOMNESS_STREAM_POLICY: &str =
    "splitmix64_sequence_with_ieee754_unit_interval_projection.v1";
pub const SIMULATION_RANDOMNESS_SUBSTREAM_DERIVATION_POLICY: &str =
    "root_seed_domain_and_index_mixed_by_splitmix64.v1";

const SPLITMIX64_GAMMA: u64 = 0x9E37_79B9_7F4A_7C15;
const SUBSTREAM_DOMAIN_MIXER: u64 = 0xA24B_AED4_963E_E407;
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
        const UNIT_INTERVAL_NORMALIZATION: f64 = 1.0 / ((1_u64 << 53) as f64);
        let most_significant_bits = self.next_u64() >> 11;
        (most_significant_bits as f64) * UNIT_INTERVAL_NORMALIZATION
    }
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

pub(crate) const fn splitmix64_output_mixer(mut value: u64) -> u64 {
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
        SimulationRandomnessFactory, SimulationRandomnessStream,
        SimulationRandomnessSubstreamDomain, SplitMix64SimulationRandomnessFactory,
        derive_simulation_substream_seed,
    };

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
}
