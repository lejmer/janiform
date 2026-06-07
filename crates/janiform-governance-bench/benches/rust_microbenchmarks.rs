use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use janiform_affine_oracle::affine_oracle::{
    ExponentialAffineTransformIntegrationStatus, solve_exponential_affine_transform_with_settings,
};
use janiform_engine_pricing::intraday_black_scholes_price;
use janiform_engine_simulation::evolve_hawkes_state;
use janiform_engine_sufficient_statistics::sample_interval_statistics_batch_from_transform_slices;
use janiform_governance_bench::benchmarking::{
    ORACLE_SOLVER_MICROBENCHMARK_PROFILE_ID, PRICING_KERNEL_MICROBENCHMARK_PROFILE_ID,
    SIMULATION_PRIMITIVE_MICROBENCHMARK_PROFILE_ID,
    SUFFICIENT_STATISTICS_SAMPLING_MICROBENCHMARK_PROFILE_ID, load_microbenchmark_fixture_set,
};

fn bench_oracle_solver_inner_path(criterion: &mut Criterion) {
    let fixture_set =
        load_microbenchmark_fixture_set().expect("microbenchmark fixtures should load");
    let case = fixture_set
        .prepare_oracle_solver_case(ORACLE_SOLVER_MICROBENCHMARK_PROFILE_ID)
        .expect("oracle solver benchmark case should prepare");
    let mut group = criterion.benchmark_group("oracle_solver_inner_path");

    group.bench_function(case.profile_id, |bench| {
        bench.iter_batched(
            || case.transform_input,
            |transform_input| {
                let result = solve_exponential_affine_transform_with_settings(
                    transform_input,
                    case.solver_settings,
                )
                .expect("oracle solver microbenchmark input should stay valid");
                assert_eq!(
                    result.status,
                    ExponentialAffineTransformIntegrationStatus::Completed
                );
                criterion::black_box(result.output.expect("completed solve should have output"));
            },
            BatchSize::SmallInput,
        );
    });

    group.finish();
}

fn bench_pricing_kernel_hot_path(criterion: &mut Criterion) {
    let fixture_set =
        load_microbenchmark_fixture_set().expect("microbenchmark fixtures should load");
    let case = fixture_set
        .prepare_pricing_kernel_case(PRICING_KERNEL_MICROBENCHMARK_PROFILE_ID)
        .expect("pricing benchmark case should prepare");
    let mut group = criterion.benchmark_group("pricing_kernel_hot_path");

    group.bench_function(case.profile_id, |bench| {
        bench.iter(|| {
            let price = intraday_black_scholes_price(
                &case.market_inputs,
                case.payoff.payoff_type,
                case.implied_volatility,
            );
            criterion::black_box(price);
        });
    });

    group.finish();
}

fn bench_sufficient_statistics_sampling_kernel(criterion: &mut Criterion) {
    let fixture_set =
        load_microbenchmark_fixture_set().expect("microbenchmark fixtures should load");
    let case = fixture_set
        .prepare_sufficient_statistics_sampling_case(
            SUFFICIENT_STATISTICS_SAMPLING_MICROBENCHMARK_PROFILE_ID,
        )
        .expect("sufficient-statistics sampling benchmark case should prepare");
    let mut group = criterion.benchmark_group("sufficient_statistics_sampling_kernel");

    group.bench_function(case.profile_id, |bench| {
        bench.iter(|| {
            let sampled_statistics = sample_interval_statistics_batch_from_transform_slices(
                case.sampling_seed,
                &case.solved_slices,
            )
            .expect("sufficient-statistics sampling microbenchmark input should stay valid");
            criterion::black_box(sampled_statistics);
        });
    });

    group.finish();
}

fn bench_simulation_transition_primitive(criterion: &mut Criterion) {
    let fixture_set =
        load_microbenchmark_fixture_set().expect("microbenchmark fixtures should load");
    let case = fixture_set
        .prepare_simulation_primitive_case(SIMULATION_PRIMITIVE_MICROBENCHMARK_PROFILE_ID)
        .expect("simulation primitive benchmark case should prepare");
    let mut group = criterion.benchmark_group("simulation_transition_primitive");

    group.bench_function(case.profile_id, |bench| {
        bench.iter(|| {
            let next_state = evolve_hawkes_state(
                case.initial_state,
                case.kernel_parameters,
                case.transition_step,
            )
            .expect("simulation primitive microbenchmark input should stay valid");
            criterion::black_box(next_state);
        });
    });

    group.finish();
}

criterion_group!(
    rust_microbenchmarks,
    bench_oracle_solver_inner_path,
    bench_pricing_kernel_hot_path,
    bench_sufficient_statistics_sampling_kernel,
    bench_simulation_transition_primitive
);
criterion_main!(rust_microbenchmarks);
