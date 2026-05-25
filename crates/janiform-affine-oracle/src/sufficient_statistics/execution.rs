use crate::affine_oracle::{
    ExponentialAffineTransformActivityLoadings, ExponentialAffineTransformEquationSpecification,
    ExponentialAffineTransformInput, ExponentialAffineTransformInputSensitivityOutput,
    ExponentialAffineTransformIntegrationResult, ExponentialAffineTransformIntegrationStatus,
    ExponentialAffineTransformSolverError, ExponentialAffineTransformSolverSettings,
    solve_exponential_affine_transform_with_optional_settings,
};
use janiform_foundations::complex_math::ComplexScalar;
use janiform_foundations::randomness::{
    SimulationRandomnessFactory, SimulationRandomnessStream, SimulationRandomnessSubstreamDomain,
    SplitMix64SimulationRandomnessFactory, derive_simulation_substream_seed,
};
use janiform_model_kernels::hawkes::{
    HawkesEventSide, HawkesExcitationState, HawkesKernelLoadings, HawkesKernelParameters,
    HawkesTransitionStep, evolve_hawkes_state,
};

use super::constants::{
    BATCH_SEED_PARTITION_SUBSTREAM_DOMAIN, MAX_INTERVAL_STATISTICS_COUNT,
    SUFFICIENT_STATISTICS_BATCH_SEED_PARTITION_POLICY,
    TRANSFORM_SLICE_NEGATIVE_EXPECTATION_TOLERANCE,
};
use super::contracts::{
    SufficientStatisticsBoundaryStateUpdateSnapshot,
    SufficientStatisticsExpectedIntervalStatistics, SufficientStatisticsIntervalStatistics,
    SufficientStatisticsIntervalStatisticsChunk, SufficientStatisticsIntervalStatisticsPathChunk,
    SufficientStatisticsStreamingChunkingSettings, SufficientStatisticsTransformSliceContext,
    SufficientStatisticsTransformSliceSettings, SufficientStatisticsTransformSlices,
};
use super::errors::{
    SufficientStatisticsBoundaryStateUpdateError, SufficientStatisticsTransformSliceExecutionError,
    SufficientStatisticsTransformSliceSamplingError, SufficientStatisticsTransformSliceSolveError,
    SufficientStatisticsTransformSlicesError,
};
use super::validation::validate_excitation_state;

pub const fn sufficient_statistics_batch_seed_partition_policy() -> &'static str {
    SUFFICIENT_STATISTICS_BATCH_SEED_PARTITION_POLICY
}

/// Deterministically derive a per-path seed from one root batch seed.
pub fn derive_sufficient_statistics_batch_path_seed(root_seed: u64, path_index: u64) -> u64 {
    derive_simulation_substream_seed(root_seed, BATCH_SEED_PARTITION_SUBSTREAM_DOMAIN, path_index)
}

/// Solve the baseline and `A/N` probe slices used by interval sufficient-statistics sampling.
pub fn solve_sufficient_statistics_transform_slices(
    equation_specification: ExponentialAffineTransformEquationSpecification,
    context: SufficientStatisticsTransformSliceContext,
    settings: SufficientStatisticsTransformSliceSettings,
) -> Result<SufficientStatisticsTransformSlices, SufficientStatisticsTransformSliceSolveError> {
    solve_sufficient_statistics_transform_slices_with_solver_settings(
        equation_specification,
        context,
        settings,
        None,
    )
}

/// Solve transform slices with optional explicit ODE solver settings.
pub fn solve_sufficient_statistics_transform_slices_with_solver_settings(
    equation_specification: ExponentialAffineTransformEquationSpecification,
    context: SufficientStatisticsTransformSliceContext,
    settings: SufficientStatisticsTransformSliceSettings,
    solver_settings: Option<ExponentialAffineTransformSolverSettings>,
) -> Result<SufficientStatisticsTransformSlices, SufficientStatisticsTransformSliceSolveError> {
    context.validate().map_err(|source| {
        SufficientStatisticsTransformSliceSolveError::InvalidContext { source }
    })?;
    settings.validate().map_err(|source| {
        SufficientStatisticsTransformSliceSolveError::InvalidSettings { source }
    })?;

    let baseline_transform = solve_transform_slice_value(
        equation_specification,
        context,
        TransformSliceEvaluationPoint::new(
            "baseline",
            ComplexScalar::new(0.0, 0.0),
            ComplexScalar::new(0.0, 0.0),
            ComplexScalar::new(1.0, 0.0),
            ComplexScalar::new(1.0, 0.0),
        ),
        solver_settings,
    )?;

    let a_plus_transform = solve_transform_slice_value(
        equation_specification,
        context,
        TransformSliceEvaluationPoint::new(
            "a_plus",
            ComplexScalar::new(settings.a_plus_u, 0.0),
            ComplexScalar::new(0.0, 0.0),
            ComplexScalar::new(1.0, 0.0),
            ComplexScalar::new(1.0, 0.0),
        ),
        solver_settings,
    )?;

    let a_minus_transform = solve_transform_slice_value(
        equation_specification,
        context,
        TransformSliceEvaluationPoint::new(
            "a_minus",
            ComplexScalar::new(0.0, 0.0),
            ComplexScalar::new(settings.a_minus_u, 0.0),
            ComplexScalar::new(1.0, 0.0),
            ComplexScalar::new(1.0, 0.0),
        ),
        solver_settings,
    )?;

    let n_plus_transform = solve_transform_slice_value(
        equation_specification,
        context,
        TransformSliceEvaluationPoint::new(
            "n_plus",
            ComplexScalar::new(0.0, 0.0),
            ComplexScalar::new(0.0, 0.0),
            ComplexScalar::new(settings.n_plus_z, 0.0),
            ComplexScalar::new(1.0, 0.0),
        ),
        solver_settings,
    )?;

    let n_minus_transform = solve_transform_slice_value(
        equation_specification,
        context,
        TransformSliceEvaluationPoint::new(
            "n_minus",
            ComplexScalar::new(0.0, 0.0),
            ComplexScalar::new(0.0, 0.0),
            ComplexScalar::new(1.0, 0.0),
            ComplexScalar::new(settings.n_minus_z, 0.0),
        ),
        solver_settings,
    )?;

    SufficientStatisticsTransformSlices::try_new(
        equation_specification,
        context,
        settings,
        baseline_transform,
        a_plus_transform,
        a_minus_transform,
        n_plus_transform,
        n_minus_transform,
    )
    .map_err(
        |source| SufficientStatisticsTransformSliceSolveError::InvalidSolvedSlices {
            interval_index: context.interval_index,
            source,
        },
    )
}

/// Sample one interval's sufficient statistics from solved transform slices.
pub fn sample_interval_statistics_from_transform_slices(
    seed: u64,
    slices: SufficientStatisticsTransformSlices,
) -> Result<SufficientStatisticsIntervalStatistics, SufficientStatisticsTransformSliceSamplingError>
{
    let randomness_factory = SplitMix64SimulationRandomnessFactory;

    sample_interval_statistics_from_transform_slices_with_randomness_factory(
        seed,
        slices,
        &randomness_factory,
    )
}

/// Extract one interval's closed-form expected sufficient statistics from solved transform slices.
pub fn derive_expected_interval_statistics_from_transform_slices(
    slices: SufficientStatisticsTransformSlices,
) -> Result<
    SufficientStatisticsExpectedIntervalStatistics,
    SufficientStatisticsTransformSliceSamplingError,
> {
    slices.validate().map_err(|source| {
        SufficientStatisticsTransformSliceSamplingError::InvalidSlices { source }
    })?;

    derive_expected_interval_statistics_from_validated_transform_slices(slices)
}

/// Extract interval sufficient-statistic expectations from analytic transform sensitivities.
pub fn derive_expected_interval_statistics_from_transform_sensitivities(
    equation_specification: ExponentialAffineTransformEquationSpecification,
    context: SufficientStatisticsTransformSliceContext,
    sensitivities: ExponentialAffineTransformInputSensitivityOutput,
) -> Result<
    SufficientStatisticsExpectedIntervalStatistics,
    SufficientStatisticsTransformSliceSamplingError,
> {
    context.validate().map_err(|source| {
        SufficientStatisticsTransformSliceSamplingError::InvalidSlices {
            source: SufficientStatisticsTransformSlicesError::InvalidContext { source },
        }
    })?;

    let transform_value = sensitivities.output.transform_value;

    let expected_a_plus = derive_non_negative_log_derivative_expectation(
        context,
        "a_plus",
        transform_value,
        sensitivities
            .u_plus_total_activity_sensitivity()
            .transform_value_derivative
            .scale(-1.0),
    )?;

    let expected_a_minus = derive_non_negative_log_derivative_expectation(
        context,
        "a_minus",
        transform_value,
        sensitivities
            .u_minus_total_activity_sensitivity()
            .transform_value_derivative
            .scale(-1.0),
    )?;

    let expected_n_plus = derive_non_negative_log_derivative_expectation(
        context,
        "n_plus",
        transform_value,
        sensitivities.z_plus_sensitivity.transform_value_derivative,
    )?;

    let expected_n_minus = derive_non_negative_log_derivative_expectation(
        context,
        "n_minus",
        transform_value,
        sensitivities.z_minus_sensitivity.transform_value_derivative,
    )?;

    let baseline_expected_business_time =
        expected_baseline_business_time_for_context(equation_specification, context);

    let expected_endogenous_a_plus = derive_endogenous_activity_expectation(
        context,
        "endogenous_a_plus",
        expected_a_plus,
        baseline_expected_business_time,
    )?;

    let expected_endogenous_a_minus = derive_endogenous_activity_expectation(
        context,
        "endogenous_a_minus",
        expected_a_minus,
        baseline_expected_business_time,
    )?;

    SufficientStatisticsExpectedIntervalStatistics::try_new(
        baseline_expected_business_time,
        expected_endogenous_a_plus,
        baseline_expected_business_time,
        expected_endogenous_a_minus,
        expected_n_plus,
        expected_n_minus,
    )
    .map_err(|source| {
        SufficientStatisticsTransformSliceSamplingError::InvalidSampledStatistics {
            interval_index: context.interval_index,
            source,
        }
    })
}

/// Sample one interval's sufficient statistics with an injected randomness factory.
pub fn sample_interval_statistics_from_transform_slices_with_randomness_factory<F>(
    seed: u64,
    slices: SufficientStatisticsTransformSlices,
    randomness_factory: &F,
) -> Result<SufficientStatisticsIntervalStatistics, SufficientStatisticsTransformSliceSamplingError>
where
    F: SimulationRandomnessFactory,
{
    sample_interval_statistics_from_transform_slices_with_sampling_index(
        seed,
        0,
        slices,
        randomness_factory,
    )
}

/// Sample one interval from exact expected sufficient statistics.
pub fn sample_interval_statistics_from_expected_statistics_with_randomness_factory<F>(
    seed: u64,
    interval_index: usize,
    expected_statistics: SufficientStatisticsExpectedIntervalStatistics,
    randomness_factory: &F,
) -> Result<SufficientStatisticsIntervalStatistics, SufficientStatisticsTransformSliceSamplingError>
where
    F: SimulationRandomnessFactory,
{
    let substream_index = derive_transform_slice_sampling_substream_index(interval_index as u64, 0);

    sample_interval_statistics_from_expected_statistics(
        seed,
        interval_index,
        expected_statistics,
        substream_index,
        randomness_factory,
    )
}

/// Sample a batch of interval statistics from solved transform slices.
pub fn sample_interval_statistics_batch_from_transform_slices(
    seed: u64,
    slices_batch: &[SufficientStatisticsTransformSlices],
) -> Result<
    Vec<SufficientStatisticsIntervalStatistics>,
    SufficientStatisticsTransformSliceSamplingError,
> {
    let randomness_factory = SplitMix64SimulationRandomnessFactory;

    sample_interval_statistics_batch_from_transform_slices_with_randomness_factory(
        seed,
        slices_batch,
        &randomness_factory,
    )
}

/// Sample a batch of interval statistics with an injected randomness factory.
pub fn sample_interval_statistics_batch_from_transform_slices_with_randomness_factory<F>(
    seed: u64,
    slices_batch: &[SufficientStatisticsTransformSlices],
    randomness_factory: &F,
) -> Result<
    Vec<SufficientStatisticsIntervalStatistics>,
    SufficientStatisticsTransformSliceSamplingError,
>
where
    F: SimulationRandomnessFactory,
{
    if slices_batch.is_empty() {
        return Ok(Vec::new());
    }

    let chunking_settings = SufficientStatisticsStreamingChunkingSettings::try_new(
        slices_batch.len(),
    )
    .map_err(|source| {
        SufficientStatisticsTransformSliceSamplingError::InvalidChunkingSettings { source }
    })?;

    let mut sampled_statistics = Vec::with_capacity(slices_batch.len());

    stream_interval_statistics_from_transform_slices_with_randomness_factory_internal(
        seed,
        slices_batch,
        chunking_settings,
        randomness_factory,
        |interval_chunk| sampled_statistics.extend(interval_chunk.records),
    )?;

    Ok(sampled_statistics)
}

/// Stream sampled interval statistics in deterministic chunks.
pub fn stream_interval_statistics_from_transform_slices<C>(
    seed: u64,
    slices_batch: &[SufficientStatisticsTransformSlices],
    chunking_settings: SufficientStatisticsStreamingChunkingSettings,
    emit_chunk: C,
) -> Result<(), SufficientStatisticsTransformSliceExecutionError>
where
    C: FnMut(SufficientStatisticsIntervalStatisticsChunk),
{
    let randomness_factory = SplitMix64SimulationRandomnessFactory;

    stream_interval_statistics_from_transform_slices_with_randomness_factory(
        seed,
        slices_batch,
        chunking_settings,
        &randomness_factory,
        emit_chunk,
    )
}

/// Stream sampled interval statistics in deterministic chunks with an injected randomness factory.
pub fn stream_interval_statistics_from_transform_slices_with_randomness_factory<F, C>(
    seed: u64,
    slices_batch: &[SufficientStatisticsTransformSlices],
    chunking_settings: SufficientStatisticsStreamingChunkingSettings,
    randomness_factory: &F,
    emit_chunk: C,
) -> Result<(), SufficientStatisticsTransformSliceExecutionError>
where
    F: SimulationRandomnessFactory,
    C: FnMut(SufficientStatisticsIntervalStatisticsChunk),
{
    chunking_settings.validate().map_err(|source| {
        SufficientStatisticsTransformSliceExecutionError::InvalidChunkingSettings { source }
    })?;

    stream_interval_statistics_from_transform_slices_with_randomness_factory_internal(
        seed,
        slices_batch,
        chunking_settings,
        randomness_factory,
        emit_chunk,
    )
    .map_err(
        |source| SufficientStatisticsTransformSliceExecutionError::SamplingFailure {
            source: Box::new(source),
        },
    )
}

/// Stream multi-path sampled interval statistics with deterministic path-seed partitioning.
pub fn stream_interval_statistics_multi_path_batch_from_transform_slices<C>(
    root_seed: u64,
    slices_path_batch: &[Vec<SufficientStatisticsTransformSlices>],
    chunking_settings: SufficientStatisticsStreamingChunkingSettings,
    emit_chunk: C,
) -> Result<(), SufficientStatisticsTransformSliceExecutionError>
where
    C: FnMut(SufficientStatisticsIntervalStatisticsPathChunk),
{
    let randomness_factory = SplitMix64SimulationRandomnessFactory;

    stream_interval_statistics_multi_path_batch_from_transform_slices_with_randomness_factory(
        root_seed,
        slices_path_batch,
        chunking_settings,
        &randomness_factory,
        emit_chunk,
    )
}

/// Stream multi-path sampled interval statistics with deterministic path-seed partitioning.
pub fn stream_interval_statistics_multi_path_batch_from_transform_slices_with_randomness_factory<
    F,
    C,
>(
    root_seed: u64,
    slices_path_batch: &[Vec<SufficientStatisticsTransformSlices>],
    chunking_settings: SufficientStatisticsStreamingChunkingSettings,
    randomness_factory: &F,
    mut emit_chunk: C,
) -> Result<(), SufficientStatisticsTransformSliceExecutionError>
where
    F: SimulationRandomnessFactory,
    C: FnMut(SufficientStatisticsIntervalStatisticsPathChunk),
{
    chunking_settings.validate().map_err(|source| {
        SufficientStatisticsTransformSliceExecutionError::InvalidChunkingSettings { source }
    })?;

    for (path_index, path_slices) in slices_path_batch.iter().enumerate() {
        let path_seed = derive_sufficient_statistics_batch_path_seed(root_seed, path_index as u64);

        stream_interval_statistics_from_transform_slices_with_randomness_factory_internal(
            path_seed,
            path_slices,
            chunking_settings,
            randomness_factory,
            |interval_chunk| {
                emit_chunk(SufficientStatisticsIntervalStatisticsPathChunk {
                    path_index,
                    path_seed,
                    interval_chunk,
                });
            },
        )
        .map_err(|source| {
            SufficientStatisticsTransformSliceExecutionError::PathSamplingFailure {
                path_index,
                source: Box::new(source),
            }
        })?;
    }

    Ok(())
}

/// Batch sample multi-path interval statistics with deterministic path-seed partitioning.
pub fn sample_interval_statistics_multi_path_batch_from_transform_slices(
    root_seed: u64,
    slices_path_batch: &[Vec<SufficientStatisticsTransformSlices>],
    chunking_settings: SufficientStatisticsStreamingChunkingSettings,
) -> Result<
    Vec<Vec<SufficientStatisticsIntervalStatistics>>,
    SufficientStatisticsTransformSliceExecutionError,
> {
    let randomness_factory = SplitMix64SimulationRandomnessFactory;

    sample_interval_statistics_multi_path_batch_from_transform_slices_with_randomness_factory(
        root_seed,
        slices_path_batch,
        chunking_settings,
        &randomness_factory,
    )
}

/// Batch sample multi-path interval statistics with deterministic path-seed partitioning.
pub fn sample_interval_statistics_multi_path_batch_from_transform_slices_with_randomness_factory<
    F,
>(
    root_seed: u64,
    slices_path_batch: &[Vec<SufficientStatisticsTransformSlices>],
    chunking_settings: SufficientStatisticsStreamingChunkingSettings,
    randomness_factory: &F,
) -> Result<
    Vec<Vec<SufficientStatisticsIntervalStatistics>>,
    SufficientStatisticsTransformSliceExecutionError,
>
where
    F: SimulationRandomnessFactory,
{
    let mut sampled_paths = vec![Vec::new(); slices_path_batch.len()];

    stream_interval_statistics_multi_path_batch_from_transform_slices_with_randomness_factory(
        root_seed,
        slices_path_batch,
        chunking_settings,
        randomness_factory,
        |path_chunk| sampled_paths[path_chunk.path_index].extend(path_chunk.interval_chunk.records),
    )?;

    Ok(sampled_paths)
}

fn stream_interval_statistics_from_transform_slices_with_randomness_factory_internal<F, C>(
    seed: u64,
    slices_batch: &[SufficientStatisticsTransformSlices],
    chunking_settings: SufficientStatisticsStreamingChunkingSettings,
    randomness_factory: &F,
    mut emit_chunk: C,
) -> Result<(), SufficientStatisticsTransformSliceSamplingError>
where
    F: SimulationRandomnessFactory,
    C: FnMut(SufficientStatisticsIntervalStatisticsChunk),
{
    let mut chunk_index = 0_usize;
    let mut chunk_start_interval_index = 0_usize;
    let mut pending_chunk_records: Vec<SufficientStatisticsIntervalStatistics> =
        Vec::with_capacity(chunking_settings.max_intervals_per_chunk);

    for (batch_position, slices) in slices_batch.iter().copied().enumerate() {
        if pending_chunk_records.is_empty() {
            chunk_start_interval_index = batch_position;
        }

        pending_chunk_records.push(
            sample_interval_statistics_from_transform_slices_with_sampling_index(
                seed,
                batch_position as u64,
                slices,
                randomness_factory,
            )?,
        );

        if pending_chunk_records.len() >= chunking_settings.max_intervals_per_chunk {
            flush_interval_statistics_chunk(
                &mut pending_chunk_records,
                &mut chunk_index,
                chunk_start_interval_index,
                &mut emit_chunk,
            );
        }
    }

    if !pending_chunk_records.is_empty() {
        flush_interval_statistics_chunk(
            &mut pending_chunk_records,
            &mut chunk_index,
            chunk_start_interval_index,
            &mut emit_chunk,
        );
    }

    Ok(())
}

fn flush_interval_statistics_chunk<C>(
    pending_chunk_records: &mut Vec<SufficientStatisticsIntervalStatistics>,
    chunk_index: &mut usize,
    chunk_start_interval_index: usize,
    emit_chunk: &mut C,
) where
    C: FnMut(SufficientStatisticsIntervalStatisticsChunk),
{
    if pending_chunk_records.is_empty() {
        return;
    }

    let records = std::mem::replace(
        pending_chunk_records,
        Vec::with_capacity(pending_chunk_records.capacity()),
    );

    emit_chunk(SufficientStatisticsIntervalStatisticsChunk {
        chunk_index: *chunk_index,
        start_interval_index: chunk_start_interval_index,
        records,
    });

    *chunk_index = chunk_index.saturating_add(1);
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct TransformSliceEvaluationPoint {
    slice_name: &'static str,
    u_plus: ComplexScalar,
    u_minus: ComplexScalar,
    z_plus: ComplexScalar,
    z_minus: ComplexScalar,
}

impl TransformSliceEvaluationPoint {
    fn new(
        slice_name: &'static str,
        u_plus: ComplexScalar,
        u_minus: ComplexScalar,
        z_plus: ComplexScalar,
        z_minus: ComplexScalar,
    ) -> Self {
        Self {
            slice_name,
            u_plus,
            u_minus,
            z_plus,
            z_minus,
        }
    }
}

fn solve_transform_slice_value(
    equation_specification: ExponentialAffineTransformEquationSpecification,
    context: SufficientStatisticsTransformSliceContext,
    evaluation_point: TransformSliceEvaluationPoint,
    solver_settings: Option<ExponentialAffineTransformSolverSettings>,
) -> Result<ComplexScalar, SufficientStatisticsTransformSliceSolveError> {
    let transform_input = ExponentialAffineTransformInput::try_new(
        equation_specification,
        ExponentialAffineTransformActivityLoadings::total_activity(
            evaluation_point.u_plus,
            evaluation_point.u_minus,
        ),
        evaluation_point.z_plus,
        evaluation_point.z_minus,
        context.s_h,
        context.news_intensity,
        context.delta,
        context.initial_excitation_state,
    )
    .map_err(
        |source| SufficientStatisticsTransformSliceSolveError::InvalidTransformInput {
            interval_index: context.interval_index,
            slice_name: evaluation_point.slice_name,
            source,
        },
    )?;

    let solve_result =
        solve_exponential_affine_transform_with_optional_settings(transform_input, solver_settings)
            .map_err(
                |source| SufficientStatisticsTransformSliceSolveError::SolverFailure {
                    interval_index: context.interval_index,
                    slice_name: evaluation_point.slice_name,
                    source,
                },
            )?;

    if solve_result.status != ExponentialAffineTransformIntegrationStatus::Completed {
        return Err(
            SufficientStatisticsTransformSliceSolveError::IncompleteSolverOutput {
                interval_index: context.interval_index,
                slice_name: evaluation_point.slice_name,
                status: solve_result.status,
                guard_failure_code: solve_result
                    .metadata
                    .guard_failure
                    .map(|guard_failure| guard_failure.code),
            },
        );
    }

    let output = solve_result.output.ok_or(
        SufficientStatisticsTransformSliceSolveError::IncompleteSolverOutput {
            interval_index: context.interval_index,
            slice_name: evaluation_point.slice_name,
            status: solve_result.status,
            guard_failure_code: solve_result
                .metadata
                .guard_failure
                .map(|guard_failure| guard_failure.code),
        },
    )?;

    Ok(output.transform_value)
}

fn sample_interval_statistics_from_transform_slices_with_sampling_index<F>(
    seed: u64,
    batch_position: u64,
    slices: SufficientStatisticsTransformSlices,
    randomness_factory: &F,
) -> Result<SufficientStatisticsIntervalStatistics, SufficientStatisticsTransformSliceSamplingError>
where
    F: SimulationRandomnessFactory,
{
    slices.validate().map_err(|source| {
        SufficientStatisticsTransformSliceSamplingError::InvalidSlices { source }
    })?;

    let expected_statistics =
        derive_expected_interval_statistics_from_validated_transform_slices(slices)?;

    let substream_index = derive_transform_slice_sampling_substream_index(
        slices.context.interval_index as u64,
        batch_position,
    );

    sample_interval_statistics_from_expected_statistics(
        seed,
        slices.context.interval_index,
        expected_statistics,
        substream_index,
        randomness_factory,
    )
}

fn sample_interval_statistics_from_expected_statistics<F>(
    seed: u64,
    interval_index: usize,
    expected_statistics: SufficientStatisticsExpectedIntervalStatistics,
    substream_index: u64,
    randomness_factory: &F,
) -> Result<SufficientStatisticsIntervalStatistics, SufficientStatisticsTransformSliceSamplingError>
where
    F: SimulationRandomnessFactory,
{
    expected_statistics.validate().map_err(|source| {
        SufficientStatisticsTransformSliceSamplingError::InvalidSampledStatistics {
            interval_index,
            source,
        }
    })?;

    let mut n_plus_stream = randomness_factory.create_substream(
        seed,
        SimulationRandomnessSubstreamDomain::EndogenousBuyEventCount,
        substream_index,
    );

    let mut n_minus_stream = randomness_factory.create_substream(
        seed,
        SimulationRandomnessSubstreamDomain::EndogenousSellEventCount,
        substream_index,
    );

    let sampled_n_plus =
        sample_count_from_expectation(expected_statistics.expected_n_plus, &mut n_plus_stream);

    let sampled_n_minus =
        sample_count_from_expectation(expected_statistics.expected_n_minus, &mut n_minus_stream);

    SufficientStatisticsIntervalStatistics::try_new(
        expected_statistics.expected_baseline_a_plus,
        expected_statistics.expected_endogenous_a_plus,
        expected_statistics.expected_baseline_a_minus,
        expected_statistics.expected_endogenous_a_minus,
        sampled_n_plus,
        sampled_n_minus,
    )
    .map_err(|source| {
        SufficientStatisticsTransformSliceSamplingError::InvalidSampledStatistics {
            interval_index,
            source,
        }
    })
}

fn derive_expected_interval_statistics_from_validated_transform_slices(
    slices: SufficientStatisticsTransformSlices,
) -> Result<
    SufficientStatisticsExpectedIntervalStatistics,
    SufficientStatisticsTransformSliceSamplingError,
> {
    let context = slices.context;
    let settings = slices.settings;

    let baseline_transform =
        extract_real_transform_value(context, "baseline_transform", slices.baseline_transform)?;

    let a_plus_transform =
        extract_real_transform_value(context, "a_plus_transform", slices.a_plus_transform)?;

    let a_minus_transform =
        extract_real_transform_value(context, "a_minus_transform", slices.a_minus_transform)?;

    let n_plus_transform =
        extract_real_transform_value(context, "n_plus_transform", slices.n_plus_transform)?;

    let n_minus_transform =
        extract_real_transform_value(context, "n_minus_transform", slices.n_minus_transform)?;

    let normalization_baseline = baseline_transform;

    let expected_a_plus = derive_non_negative_expectation(
        context,
        "a_plus",
        baseline_transform - a_plus_transform,
        settings.a_plus_u * normalization_baseline,
    )?;

    let expected_a_minus = derive_non_negative_expectation(
        context,
        "a_minus",
        baseline_transform - a_minus_transform,
        settings.a_minus_u * normalization_baseline,
    )?;

    let expected_n_plus = derive_non_negative_expectation(
        context,
        "n_plus",
        n_plus_transform - baseline_transform,
        (settings.n_plus_z - 1.0) * normalization_baseline,
    )?;

    let expected_n_minus = derive_non_negative_expectation(
        context,
        "n_minus",
        n_minus_transform - baseline_transform,
        (settings.n_minus_z - 1.0) * normalization_baseline,
    )?;

    let baseline_expected_business_time =
        expected_baseline_business_time_for_context(slices.equation_specification, context);
    let expected_endogenous_a_plus = derive_endogenous_activity_expectation(
        context,
        "endogenous_a_plus",
        expected_a_plus,
        baseline_expected_business_time,
    )?;
    let expected_endogenous_a_minus = derive_endogenous_activity_expectation(
        context,
        "endogenous_a_minus",
        expected_a_minus,
        baseline_expected_business_time,
    )?;

    SufficientStatisticsExpectedIntervalStatistics::try_new(
        baseline_expected_business_time,
        expected_endogenous_a_plus,
        baseline_expected_business_time,
        expected_endogenous_a_minus,
        expected_n_plus,
        expected_n_minus,
    )
    .map_err(|source| {
        SufficientStatisticsTransformSliceSamplingError::InvalidSampledStatistics {
            interval_index: context.interval_index,
            source,
        }
    })
}

fn expected_baseline_business_time_for_context(
    equation_specification: ExponentialAffineTransformEquationSpecification,
    context: SufficientStatisticsTransformSliceContext,
) -> f64 {
    equation_specification.hawkes_kernel_parameters.lambda_bar * context.s_h * context.delta
}

fn derive_endogenous_activity_expectation(
    context: SufficientStatisticsTransformSliceContext,
    statistic: &'static str,
    expected_total_activity: f64,
    baseline_expected_business_time: f64,
) -> Result<f64, SufficientStatisticsTransformSliceSamplingError> {
    derive_non_negative_expectation(
        context,
        statistic,
        expected_total_activity - baseline_expected_business_time,
        1.0,
    )
}

fn extract_real_transform_value(
    context: SufficientStatisticsTransformSliceContext,
    field: &'static str,
    value: ComplexScalar,
) -> Result<f64, SufficientStatisticsTransformSliceSamplingError> {
    if !value.is_finite() {
        return Err(
            SufficientStatisticsTransformSliceSamplingError::NonRealTransformSlice {
                interval_index: context.interval_index,
                field,
                value,
                tolerance: f64::NAN,
            },
        );
    }

    let modulus = value.norm();
    if !modulus.is_finite() {
        return Err(
            SufficientStatisticsTransformSliceSamplingError::NonRealTransformSlice {
                interval_index: context.interval_index,
                field,
                value,
                tolerance: f64::NAN,
            },
        );
    }

    Ok(modulus)
}

fn derive_non_negative_expectation(
    context: SufficientStatisticsTransformSliceContext,
    statistic: &'static str,
    numerator: f64,
    denominator: f64,
) -> Result<f64, SufficientStatisticsTransformSliceSamplingError> {
    let expected_value = numerator / denominator;
    if !expected_value.is_finite() {
        return Err(
            SufficientStatisticsTransformSliceSamplingError::InvalidExpectedStatistic {
                interval_index: context.interval_index,
                statistic,
                value: expected_value,
                numerator,
                denominator,
                context,
            },
        );
    }

    if expected_value < -TRANSFORM_SLICE_NEGATIVE_EXPECTATION_TOLERANCE {
        return Err(
            SufficientStatisticsTransformSliceSamplingError::InvalidExpectedStatistic {
                interval_index: context.interval_index,
                statistic,
                value: expected_value,
                numerator,
                denominator,
                context,
            },
        );
    }

    Ok(expected_value.max(0.0))
}

fn derive_non_negative_log_derivative_expectation(
    context: SufficientStatisticsTransformSliceContext,
    statistic: &'static str,
    transform_value: ComplexScalar,
    transform_value_derivative: ComplexScalar,
) -> Result<f64, SufficientStatisticsTransformSliceSamplingError> {
    let Some(logarithmic_derivative) = transform_value_derivative.divide(transform_value) else {
        return Err(
            SufficientStatisticsTransformSliceSamplingError::InvalidExpectedStatistic {
                interval_index: context.interval_index,
                statistic,
                value: f64::NAN,
                numerator: transform_value_derivative.norm(),
                denominator: transform_value.norm(),
                context,
            },
        );
    };

    if !logarithmic_derivative.is_finite() {
        return Err(
            SufficientStatisticsTransformSliceSamplingError::InvalidExpectedStatistic {
                interval_index: context.interval_index,
                statistic,
                value: f64::NAN,
                numerator: transform_value_derivative.norm(),
                denominator: transform_value.norm(),
                context,
            },
        );
    }

    derive_non_negative_expectation(context, statistic, logarithmic_derivative.real, 1.0)
}

fn derive_transform_slice_sampling_substream_index(
    interval_index: u64,
    batch_position: u64,
) -> u64 {
    let interval_component = interval_index.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    interval_component ^ batch_position
}

fn sample_count_from_expectation<R>(expected_count: f64, randomness_stream: &mut R) -> u64
where
    R: SimulationRandomnessStream,
{
    if !expected_count.is_finite() || expected_count <= 0.0 {
        return 0;
    }

    if expected_count >= u64::MAX as f64 {
        return MAX_INTERVAL_STATISTICS_COUNT;
    }

    let floored_count = expected_count.floor() as u64;
    let fractional_count = expected_count - floored_count as f64;

    let sampled_count = if randomness_stream.next_unit_interval_f64() < fractional_count {
        floored_count.saturating_add(1)
    } else {
        floored_count
    };

    sampled_count.min(MAX_INTERVAL_STATISTICS_COUNT)
}

/// Update one interval boundary state using sampled sufficient statistics.
///
/// Composition order:
/// 1. shared-decay drift over `interval_delta`
/// 2. buy-count mapped endogenous jump
/// 3. sell-count mapped endogenous jump
pub fn update_boundary_state_from_interval_statistics(
    kernel_parameters: HawkesKernelParameters,
    snapshot: SufficientStatisticsBoundaryStateUpdateSnapshot,
) -> Result<HawkesExcitationState, SufficientStatisticsBoundaryStateUpdateError> {
    if !snapshot.interval_delta.is_finite() || snapshot.interval_delta <= 0.0 {
        return Err(
            SufficientStatisticsBoundaryStateUpdateError::InvalidIntervalDelta {
                snapshot,
                value: snapshot.interval_delta,
            },
        );
    }

    validate_excitation_state(snapshot.state_before_update).map_err(|(field, value)| {
        SufficientStatisticsBoundaryStateUpdateError::InvalidStateBeforeUpdate {
            snapshot,
            field,
            value,
        }
    })?;

    snapshot.sampled_statistics.validate().map_err(|source| {
        SufficientStatisticsBoundaryStateUpdateError::InvalidSampledStatistics { snapshot, source }
    })?;

    let drifted_state =
        evolve_hawkes_state(
            snapshot.state_before_update,
            kernel_parameters,
            HawkesTransitionStep::new(snapshot.interval_delta, None, None),
        )
        .map_err(|source| {
            SufficientStatisticsBoundaryStateUpdateError::TransitionFailure { snapshot, source }
        })?;

    let post_buy_state = apply_endogenous_count_mapped_jump(
        drifted_state,
        kernel_parameters,
        snapshot,
        HawkesEventSide::Buy,
        snapshot.sampled_statistics.n_plus,
    )?;

    let post_sell_state = apply_endogenous_count_mapped_jump(
        post_buy_state,
        kernel_parameters,
        snapshot,
        HawkesEventSide::Sell,
        snapshot.sampled_statistics.n_minus,
    )?;

    validate_excitation_state(post_sell_state).map_err(|(field, value)| {
        SufficientStatisticsBoundaryStateUpdateError::InvalidStateAfterUpdate {
            snapshot,
            field,
            value,
        }
    })?;

    Ok(post_sell_state)
}

fn apply_endogenous_count_mapped_jump(
    state_before_jump: HawkesExcitationState,
    kernel_parameters: HawkesKernelParameters,
    snapshot: SufficientStatisticsBoundaryStateUpdateSnapshot,
    side: HawkesEventSide,
    event_count: u64,
) -> Result<HawkesExcitationState, SufficientStatisticsBoundaryStateUpdateError> {
    if event_count == 0 {
        return Ok(state_before_jump);
    }

    let scaled_alpha = scale_kernel_loadings_for_event_count(
        kernel_parameters.alpha,
        snapshot,
        side,
        event_count,
    )?;

    let scaled_kernel_parameters = HawkesKernelParameters {
        alpha: scaled_alpha,
        ..kernel_parameters
    };

    evolve_hawkes_state(
        state_before_jump,
        scaled_kernel_parameters,
        HawkesTransitionStep::new(0.0, Some(side), None),
    )
    .map_err(
        |source| SufficientStatisticsBoundaryStateUpdateError::TransitionFailure {
            snapshot,
            source,
        },
    )
}

fn scale_kernel_loadings_for_event_count(
    loadings: HawkesKernelLoadings,
    snapshot: SufficientStatisticsBoundaryStateUpdateSnapshot,
    side: HawkesEventSide,
    event_count: u64,
) -> Result<HawkesKernelLoadings, SufficientStatisticsBoundaryStateUpdateError> {
    let event_count_scale = event_count as f64;

    match side {
        HawkesEventSide::Buy => {
            let scaled_plus_plus = loadings.plus_plus * event_count_scale;
            let scaled_minus_plus = loadings.minus_plus * event_count_scale;

            validate_scaled_loading(
                snapshot,
                side,
                event_count,
                "alpha.plus_plus",
                scaled_plus_plus,
            )?;
            validate_scaled_loading(
                snapshot,
                side,
                event_count,
                "alpha.minus_plus",
                scaled_minus_plus,
            )?;

            Ok(HawkesKernelLoadings {
                plus_plus: scaled_plus_plus,
                plus_minus: loadings.plus_minus,
                minus_plus: scaled_minus_plus,
                minus_minus: loadings.minus_minus,
            })
        }
        HawkesEventSide::Sell => {
            let scaled_plus_minus = loadings.plus_minus * event_count_scale;
            let scaled_minus_minus = loadings.minus_minus * event_count_scale;

            validate_scaled_loading(
                snapshot,
                side,
                event_count,
                "alpha.plus_minus",
                scaled_plus_minus,
            )?;
            validate_scaled_loading(
                snapshot,
                side,
                event_count,
                "alpha.minus_minus",
                scaled_minus_minus,
            )?;

            Ok(HawkesKernelLoadings {
                plus_plus: loadings.plus_plus,
                plus_minus: scaled_plus_minus,
                minus_plus: loadings.minus_plus,
                minus_minus: scaled_minus_minus,
            })
        }
    }
}

fn validate_scaled_loading(
    snapshot: SufficientStatisticsBoundaryStateUpdateSnapshot,
    side: HawkesEventSide,
    event_count: u64,
    field: &'static str,
    value: f64,
) -> Result<(), SufficientStatisticsBoundaryStateUpdateError> {
    if !value.is_finite() || value < 0.0 {
        return Err(
            SufficientStatisticsBoundaryStateUpdateError::InvalidScaledKernelLoading {
                snapshot,
                side,
                event_count,
                field,
                value,
            },
        );
    }

    Ok(())
}

/// Solve one-interval exponential-affine transform for sufficient-statistics sampling.
///
/// This adapter stays conversion-focused and reuses the same shared solver entrypoint as pricing.
pub fn solve_exponential_affine_transform_for_sufficient_statistics_sampling(
    transform_input: ExponentialAffineTransformInput,
) -> Result<ExponentialAffineTransformIntegrationResult, ExponentialAffineTransformSolverError> {
    solve_exponential_affine_transform_with_optional_settings(transform_input, None)
}

/// Solve one-interval exponential-affine transform for sufficient-statistics sampling with explicit settings.
pub fn solve_exponential_affine_transform_for_sufficient_statistics_sampling_with_settings(
    transform_input: ExponentialAffineTransformInput,
    solver_settings: ExponentialAffineTransformSolverSettings,
) -> Result<ExponentialAffineTransformIntegrationResult, ExponentialAffineTransformSolverError> {
    solve_exponential_affine_transform_with_optional_settings(
        transform_input,
        Some(solver_settings),
    )
}

#[cfg(test)]
mod tests {
    use janiform_foundations::complex_math::ComplexScalar;
    use janiform_model_kernels::hawkes::{
        HawkesExcitationState, HawkesKernelLoadings, HawkesKernelParameters,
    };
    use janiform_model_kernels::news::GammaSeverityMarkConfiguration;

    use super::{
        derive_expected_interval_statistics_from_transform_slices, extract_real_transform_value,
    };
    use crate::affine_oracle::{
        ExponentialAffineTransformEquationSpecification,
        ExponentialAffineTransformNewsIntensityInput,
    };
    use crate::sufficient_statistics::{
        SufficientStatisticsTransformSliceContext, SufficientStatisticsTransformSliceSettings,
        SufficientStatisticsTransformSlices,
    };

    #[test]
    fn extract_real_transform_value_uses_modulus_for_positive_transform_values() {
        let context = SufficientStatisticsTransformSliceContext::try_new(
            0,
            1.0,
            ExponentialAffineTransformNewsIntensityInput::try_new(0.0, 0.0)
                .expect("news intensity should construct"),
            1.0,
            HawkesExcitationState {
                y_plus_plus: 0.0,
                y_plus_minus: 0.0,
                y_minus_plus: 0.0,
                y_minus_minus: 0.0,
            },
        )
        .expect("slice context should construct");

        let transform_value = ComplexScalar::new(0.9999964795005776, -6.097094342473048e-6);
        let recovered =
            extract_real_transform_value(context, "baseline_transform", transform_value)
                .expect("finite positive transform should recover");

        assert_eq!(recovered, transform_value.norm());
    }

    #[test]
    fn expected_statistics_are_normalized_by_baseline_transform() {
        let context = SufficientStatisticsTransformSliceContext::try_new(
            0,
            1.0,
            ExponentialAffineTransformNewsIntensityInput::try_new(0.0, 0.0)
                .expect("news intensity should construct"),
            1.0,
            HawkesExcitationState {
                y_plus_plus: 0.0,
                y_plus_minus: 0.0,
                y_minus_plus: 0.0,
                y_minus_minus: 0.0,
            },
        )
        .expect("slice context should construct");

        let settings = SufficientStatisticsTransformSliceSettings::try_new(0.25, 0.5, 1.25, 1.5)
            .expect("slice settings should construct");

        let slices = SufficientStatisticsTransformSlices::try_new(
            ExponentialAffineTransformEquationSpecification::try_new(
                HawkesKernelParameters::try_new(
                    0.0,
                    1.0,
                    HawkesKernelLoadings {
                        plus_plus: 0.0,
                        plus_minus: 0.0,
                        minus_plus: 0.0,
                        minus_minus: 0.0,
                    },
                    HawkesKernelLoadings {
                        plus_plus: 0.0,
                        plus_minus: 0.0,
                        minus_plus: 0.0,
                        minus_minus: 0.0,
                    },
                )
                .expect("kernel parameters should construct"),
                GammaSeverityMarkConfiguration::try_new(1.0, 1.0)
                    .expect("gamma severity marks should construct"),
            )
            .expect("equation specification should construct"),
            context,
            settings,
            ComplexScalar::new(2.0, 0.0),
            ComplexScalar::new(1.5, 0.0),
            ComplexScalar::new(1.0, 0.0),
            ComplexScalar::new(2.5, 0.0),
            ComplexScalar::new(3.0, 0.0),
        )
        .expect("transform slices should construct");

        let statistics = derive_expected_interval_statistics_from_transform_slices(slices)
            .expect("expected statistics should derive");

        assert_eq!(statistics.expected_a_plus, 1.0);
        assert_eq!(statistics.expected_a_minus, 1.0);
        assert_eq!(statistics.expected_n_plus, 1.0);
        assert_eq!(statistics.expected_n_minus, 1.0);
    }
}
