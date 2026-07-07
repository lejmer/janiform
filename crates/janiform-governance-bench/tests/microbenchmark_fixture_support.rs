use janiform_governance_bench::benchmarking::{
    ORACLE_SOLVER_MICROBENCHMARK_PROFILE_ID, PRICING_KERNEL_MICROBENCHMARK_PROFILE_ID,
    SIMULATION_PRIMITIVE_MICROBENCHMARK_PROFILE_ID,
    SUFFICIENT_STATISTICS_SAMPLING_MICROBENCHMARK_PROFILE_ID, load_microbenchmark_fixture_set,
};

#[test]
fn microbenchmark_fixture_loader_exposes_expected_checked_in_profiles() {
    let fixture_set =
        load_microbenchmark_fixture_set().expect("microbenchmark fixtures should load");

    assert_eq!(
        fixture_set.catalog.catalog_id,
        "coarse_time_engine_benchmark_scenarios_v1"
    );
    assert_eq!(
        fixture_set.policy.policy_id,
        "oracle_pricer_sufficient_statistics_benchmark_policy_v1"
    );
    assert!(
        fixture_set
            .catalog
            .profiles
            .iter()
            .any(|profile| profile.profile_id == ORACLE_SOLVER_MICROBENCHMARK_PROFILE_ID)
    );
    assert!(
        fixture_set
            .catalog
            .profiles
            .iter()
            .any(|profile| profile.profile_id == PRICING_KERNEL_MICROBENCHMARK_PROFILE_ID)
    );
    assert!(
        fixture_set
            .catalog
            .profiles
            .iter()
            .any(|profile| profile.profile_id
                == SUFFICIENT_STATISTICS_SAMPLING_MICROBENCHMARK_PROFILE_ID)
    );
    assert!(
        fixture_set
            .catalog
            .profiles
            .iter()
            .any(|profile| profile.profile_id == SIMULATION_PRIMITIVE_MICROBENCHMARK_PROFILE_ID)
    );
}

#[test]
fn microbenchmark_cases_prepare_deterministic_component_inputs() {
    let fixture_set =
        load_microbenchmark_fixture_set().expect("microbenchmark fixtures should load");

    let oracle_case = fixture_set
        .prepare_oracle_solver_case(ORACLE_SOLVER_MICROBENCHMARK_PROFILE_ID)
        .expect("oracle solver case should prepare");
    assert_eq!(
        oracle_case.profile_id,
        ORACLE_SOLVER_MICROBENCHMARK_PROFILE_ID
    );
    assert!(oracle_case.transform_input.delta > 0.0);
    assert!(oracle_case.solver_settings.step_count > 0);

    let pricing_case = fixture_set
        .prepare_pricing_kernel_case(PRICING_KERNEL_MICROBENCHMARK_PROFILE_ID)
        .expect("pricing case should prepare");
    assert_eq!(
        pricing_case.profile_id,
        PRICING_KERNEL_MICROBENCHMARK_PROFILE_ID
    );
    assert!(
        pricing_case.market_inputs.spot
            > pricing_case
                .market_inputs
                .single_strike()
                .expect("pricing benchmark case should be single-node")
                * 0.5
    );
    assert!(pricing_case.implied_volatility.is_finite());

    let sufficient_statistics_case = fixture_set
        .prepare_sufficient_statistics_sampling_case(
            SUFFICIENT_STATISTICS_SAMPLING_MICROBENCHMARK_PROFILE_ID,
        )
        .expect("sufficient-statistics case should prepare");
    assert_eq!(
        sufficient_statistics_case.profile_id,
        SUFFICIENT_STATISTICS_SAMPLING_MICROBENCHMARK_PROFILE_ID
    );
    assert!(!sufficient_statistics_case.solved_slices.is_empty());

    let simulation_case = fixture_set
        .prepare_simulation_primitive_case(SIMULATION_PRIMITIVE_MICROBENCHMARK_PROFILE_ID)
        .expect("simulation case should prepare");
    assert_eq!(
        simulation_case.profile_id,
        SIMULATION_PRIMITIVE_MICROBENCHMARK_PROFILE_ID
    );
    assert!(simulation_case.transition_step.delta_t > 0.0);
    assert!(
        simulation_case
            .transition_step
            .exogenous_news_severity_mark
            .is_some()
    );
}
