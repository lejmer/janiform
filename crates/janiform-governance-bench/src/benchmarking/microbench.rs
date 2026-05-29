use std::error::Error;
use std::fmt::{Display, Formatter};

use janiform_affine_oracle::affine_oracle::{
    ExponentialAffineTransformInput, ExponentialAffineTransformSolverSettings,
    ExponentialAffineTransformSolverSettingsError,
};
use janiform_engine_pricing::internal::FourierInversionEngineSettings;
use janiform_engine_pricing::{
    VanillaOptionPayoffConfiguration, VanillaOptionPayoffType, VanillaOptionPricingMarketInputs,
};
use janiform_engine_sufficient_statistics::SufficientStatisticsTransformSlices;
use janiform_foundations::complex_math::{ComplexScalar, complex_exponential};
use janiform_model_kernels::hawkes::{
    HawkesEventSide, HawkesExcitationState, HawkesKernelParameters, HawkesTransitionStep,
};
use janiform_model_kernels::news::GammaSeverityMarkConfiguration;

use super::builders::{
    build_equation_specification, build_oracle_transform_input, build_pricing_engine_settings,
    build_pricing_market_inputs, build_sufficient_statistics_transform_slices,
};
use super::catalog::{
    BenchmarkScenarioCatalog, BenchmarkScenarioCatalogLoadError, BenchmarkScenarioWorkloadProfile,
    load_benchmark_scenario_catalog_from_json,
};
use super::harness::OraclePricerSufficientStatisticsBenchmarkHarnessError;
use super::policy::{
    OraclePricerSufficientStatisticsBenchmarkPolicy,
    OraclePricerSufficientStatisticsBenchmarkPolicyLoadError,
    load_oracle_pricer_sufficient_statistics_benchmark_policy_from_json,
};
use super::utilities::{derive_invocation_seed, fnv1a_64, splitmix64, unit_interval_from_seed};

const CANONICAL_BENCHMARK_SCENARIO_CATALOG_JSON: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/fixtures/benchmarks/benchmark_scenario_catalog.json"
));
const CANONICAL_BENCHMARK_POLICY_JSON: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/fixtures/benchmarks/oracle_pricer_sufficient_statistics_benchmark_policy.json"
));

pub const MICROBENCHMARK_REPRODUCIBILITY_SEED: u64 = 17;
pub const ORACLE_SOLVER_MICROBENCHMARK_PROFILE_ID: &str = "small_hourly_baseline";
pub const PRICING_KERNEL_MICROBENCHMARK_PROFILE_ID: &str = "medium_hourly_balanced";
pub const SUFFICIENT_STATISTICS_SAMPLING_MICROBENCHMARK_PROFILE_ID: &str = "small_hourly_baseline";
pub const SIMULATION_PRIMITIVE_MICROBENCHMARK_PROFILE_ID: &str = "medium_half_hour_balanced";

#[derive(Debug, Clone, PartialEq)]
pub enum MicrobenchmarkFixtureError {
    InvalidScenarioCatalog {
        source: BenchmarkScenarioCatalogLoadError,
    },
    InvalidBenchmarkPolicy {
        source: OraclePricerSufficientStatisticsBenchmarkPolicyLoadError,
    },
    MissingProfile {
        profile_id: String,
    },
    InvalidOracleSolverSettings {
        profile_id: String,
        source: ExponentialAffineTransformSolverSettingsError,
    },
    InvalidPreparedWorkload {
        source: OraclePricerSufficientStatisticsBenchmarkHarnessError,
    },
}

impl Display for MicrobenchmarkFixtureError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidScenarioCatalog { source } => write!(
                formatter,
                "microbenchmark fixture catalog is invalid: {source}"
            ),
            Self::InvalidBenchmarkPolicy { source } => write!(
                formatter,
                "microbenchmark policy fixture is invalid: {source}"
            ),
            Self::MissingProfile { profile_id } => write!(
                formatter,
                "microbenchmark profile `{profile_id}` is not present in the checked-in scenario catalog"
            ),
            Self::InvalidOracleSolverSettings { profile_id, source } => write!(
                formatter,
                "microbenchmark oracle solver settings are invalid for profile `{profile_id}`: {source}"
            ),
            Self::InvalidPreparedWorkload { source } => write!(
                formatter,
                "failed to prepare microbenchmark inputs: {source}"
            ),
        }
    }
}

impl Error for MicrobenchmarkFixtureError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidScenarioCatalog { source } => Some(source),
            Self::InvalidBenchmarkPolicy { source } => Some(source),
            Self::InvalidOracleSolverSettings { source, .. } => Some(source),
            Self::InvalidPreparedWorkload { source } => Some(source),
            Self::MissingProfile { .. } => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct MicrobenchmarkFixtureSet {
    pub catalog: BenchmarkScenarioCatalog,
    pub policy: OraclePricerSufficientStatisticsBenchmarkPolicy,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PreparedOracleSolverBenchmarkCase {
    pub profile_id: &'static str,
    pub transform_input: ExponentialAffineTransformInput,
    pub solver_settings: ExponentialAffineTransformSolverSettings,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PreparedPricingKernelBenchmarkCase {
    pub profile_id: &'static str,
    pub market_inputs: VanillaOptionPricingMarketInputs,
    pub payoff: VanillaOptionPayoffConfiguration,
    pub engine_settings: FourierInversionEngineSettings,
    pub drift: f64,
    pub variance: f64,
}

impl PreparedPricingKernelBenchmarkCase {
    pub fn characteristic_function(&self, argument: ComplexScalar) -> ComplexScalar {
        let i_times_argument = ComplexScalar::new(-argument.imaginary, argument.real);
        let drift_term = i_times_argument.scale(self.drift);
        let diffusion_term = argument.square().scale(-0.5 * self.variance);
        complex_exponential(drift_term.plus(diffusion_term))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PreparedSufficientStatisticsSamplingBenchmarkCase {
    pub profile_id: &'static str,
    pub sampling_seed: u64,
    pub solved_slices: Vec<SufficientStatisticsTransformSlices>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PreparedSimulationPrimitiveBenchmarkCase {
    pub profile_id: &'static str,
    pub initial_state: HawkesExcitationState,
    pub kernel_parameters: HawkesKernelParameters,
    pub transition_step: HawkesTransitionStep,
}

pub fn load_microbenchmark_fixture_set()
-> Result<MicrobenchmarkFixtureSet, MicrobenchmarkFixtureError> {
    let catalog =
        load_benchmark_scenario_catalog_from_json(CANONICAL_BENCHMARK_SCENARIO_CATALOG_JSON)
            .map_err(|source| MicrobenchmarkFixtureError::InvalidScenarioCatalog { source })?;
    let policy = load_oracle_pricer_sufficient_statistics_benchmark_policy_from_json(
        CANONICAL_BENCHMARK_POLICY_JSON,
    )
    .map_err(|source| MicrobenchmarkFixtureError::InvalidBenchmarkPolicy { source })?;

    Ok(MicrobenchmarkFixtureSet { catalog, policy })
}

impl MicrobenchmarkFixtureSet {
    pub fn prepare_oracle_solver_case(
        &self,
        profile_id: &'static str,
    ) -> Result<PreparedOracleSolverBenchmarkCase, MicrobenchmarkFixtureError> {
        let profile = self.profile(profile_id)?;
        let equation_specification = build_equation_specification(profile, &self.policy)
            .map_err(|source| MicrobenchmarkFixtureError::InvalidPreparedWorkload { source })?;
        let solver_settings = self.oracle_solver_settings(profile)?;
        let oracle_seed = self.component_seed(
            profile,
            self.policy.seed_partition.oracle_component_seed_salt,
        );
        let transform_input = build_oracle_transform_input(
            profile,
            &self.policy,
            equation_specification,
            derive_invocation_seed(oracle_seed, 1),
        )
        .map_err(|source| MicrobenchmarkFixtureError::InvalidPreparedWorkload { source })?;

        Ok(PreparedOracleSolverBenchmarkCase {
            profile_id,
            transform_input,
            solver_settings,
        })
    }

    pub fn prepare_pricing_kernel_case(
        &self,
        profile_id: &'static str,
    ) -> Result<PreparedPricingKernelBenchmarkCase, MicrobenchmarkFixtureError> {
        let profile = self.profile(profile_id)?;

        let pricing_seed = self.component_seed(
            profile,
            self.policy.seed_partition.pricing_component_seed_salt,
        );

        let invocation_seed = derive_invocation_seed(pricing_seed, 1);

        let market_inputs = build_pricing_market_inputs(profile, &self.policy, invocation_seed)
            .map_err(
                |source| MicrobenchmarkFixtureError::InvalidPreparedWorkload {
                    source: OraclePricerSufficientStatisticsBenchmarkHarnessError::InvalidPricingMarketInputs {
                        profile_id: profile.profile_id.clone(),
                        source: Box::new(source),
                    },
                },
            )?;

        let engine_settings = build_pricing_engine_settings(profile, &self.policy)
            .map_err(|source| MicrobenchmarkFixtureError::InvalidPreparedWorkload { source })?;

        let payoff = VanillaOptionPayoffConfiguration {
            payoff_type: VanillaOptionPayoffType::Call,
        };

        let volatility = interpolate_pricing_volatility(&self.policy, invocation_seed);

        let maturity_interval = market_inputs.single_maturity_interval().map_err(|source| {
            MicrobenchmarkFixtureError::InvalidPreparedWorkload {
                source: OraclePricerSufficientStatisticsBenchmarkHarnessError::InvalidPricingMarketInputs {
                    profile_id: profile.profile_id.clone(),
                    source: Box::new(source),
                },
            }
        })?;

        let drift = (market_inputs.risk_free_rate
            - market_inputs.dividend_yield
            - 0.5 * volatility * volatility)
            * maturity_interval;

        let variance = volatility * volatility * maturity_interval;

        Ok(PreparedPricingKernelBenchmarkCase {
            profile_id,
            market_inputs,
            payoff,
            engine_settings,
            drift,
            variance,
        })
    }

    pub fn prepare_sufficient_statistics_sampling_case(
        &self,
        profile_id: &'static str,
    ) -> Result<PreparedSufficientStatisticsSamplingBenchmarkCase, MicrobenchmarkFixtureError> {
        let profile = self.profile(profile_id)?;

        let equation_specification = build_equation_specification(profile, &self.policy)
            .map_err(|source| MicrobenchmarkFixtureError::InvalidPreparedWorkload { source })?;

        let solver_settings = self.oracle_solver_settings(profile)?;

        let sufficient_statistics_seed = self.component_seed(
            profile,
            self.policy
                .seed_partition
                .sufficient_statistics_component_seed_salt,
        );

        let solved_slices = build_sufficient_statistics_transform_slices(
            profile,
            &self.policy,
            equation_specification,
            solver_settings,
            sufficient_statistics_seed,
        )
        .map_err(|source| MicrobenchmarkFixtureError::InvalidPreparedWorkload { source })?;

        Ok(PreparedSufficientStatisticsSamplingBenchmarkCase {
            profile_id,
            sampling_seed: derive_invocation_seed(sufficient_statistics_seed, 1),
            solved_slices,
        })
    }

    pub fn prepare_simulation_primitive_case(
        &self,
        profile_id: &'static str,
    ) -> Result<PreparedSimulationPrimitiveBenchmarkCase, MicrobenchmarkFixtureError> {
        let profile = self.profile(profile_id)?;

        let model_parameters = self.policy.model_parameters;

        let kernel_parameters = HawkesKernelParameters::try_new(
            profile
                .model_state
                .baseline_deseasonalized_intensity_lambda_bar,
            model_parameters.hawkes_beta,
            model_parameters.hawkes_alpha,
            model_parameters.hawkes_rho,
        )
        .map_err(
            |source| MicrobenchmarkFixtureError::InvalidPreparedWorkload {
                source: OraclePricerSufficientStatisticsBenchmarkHarnessError::InvalidModelKernelParameters {
                    profile_id: profile.profile_id.clone(),
                    source: Box::new(source),
                },
            },
        )?;

        let simulation_seed = splitmix64(
            self.component_seed(
                profile,
                self.policy
                    .seed_partition
                    .sufficient_statistics_component_seed_salt,
            ) ^ 0x5A,
        );

        let transition_step = HawkesTransitionStep::new(
            profile.interval_settings.interval_delta,
            Some(if unit_interval_from_seed(simulation_seed) >= 0.5 {
                HawkesEventSide::Buy
            } else {
                HawkesEventSide::Sell
            }),
            Some(
                unit_interval_from_seed(simulation_seed ^ 0xA5)
                    * GammaSeverityMarkConfiguration::new(
                        self.policy.model_parameters.gamma_shape_k,
                    )
                    .derived_unit_mean_scale(),
            ),
        );

        Ok(PreparedSimulationPrimitiveBenchmarkCase {
            profile_id,
            initial_state: profile.model_state.initial_excitation_state,
            kernel_parameters,
            transition_step,
        })
    }

    fn profile(
        &self,
        profile_id: &'static str,
    ) -> Result<&BenchmarkScenarioWorkloadProfile, MicrobenchmarkFixtureError> {
        self.catalog
            .profiles
            .iter()
            .find(|profile| profile.profile_id == profile_id)
            .ok_or_else(|| MicrobenchmarkFixtureError::MissingProfile {
                profile_id: profile_id.to_string(),
            })
    }

    fn oracle_solver_settings(
        &self,
        profile: &BenchmarkScenarioWorkloadProfile,
    ) -> Result<ExponentialAffineTransformSolverSettings, MicrobenchmarkFixtureError> {
        ExponentialAffineTransformSolverSettings::try_new(
            janiform_affine_oracle::affine_oracle::ExponentialAffineTransformIntegratorMethod::FourthOrderRungeKutta,
            profile.interval_settings.oracle_solver_step_count,
        )
        .map_err(
            |source| MicrobenchmarkFixtureError::InvalidOracleSolverSettings {
                profile_id: profile.profile_id.clone(),
                source,
            },
        )
    }

    fn component_seed(
        &self,
        profile: &BenchmarkScenarioWorkloadProfile,
        component_salt: u64,
    ) -> u64 {
        splitmix64(profile_seed(profile) ^ component_salt)
    }
}

fn profile_seed(profile: &BenchmarkScenarioWorkloadProfile) -> u64 {
    splitmix64(
        MICROBENCHMARK_REPRODUCIBILITY_SEED.wrapping_add(fnv1a_64(profile.profile_id.as_bytes())),
    )
}

fn interpolate_pricing_volatility(
    policy: &OraclePricerSufficientStatisticsBenchmarkPolicy,
    invocation_seed: u64,
) -> f64 {
    let noise = unit_interval_from_seed(invocation_seed ^ 0x51);

    policy.pricing_input_ranges.volatility_min
        + (policy.pricing_input_ranges.volatility_max - policy.pricing_input_ranges.volatility_min)
            * noise
}
