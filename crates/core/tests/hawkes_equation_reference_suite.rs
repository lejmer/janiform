use std::fmt::Write;

use janiform_core::hawkes::{
    HawkesEventSide, HawkesExcitationState, HawkesKernelLoadings, HawkesKernelParameters,
    HawkesTransitionError, HawkesTransitionStep,
};

const STATE_TOLERANCE: f64 = 1e-12;
const COMPONENT_IDENTIFIERS: [&str; 4] = [
    "y_plus_plus",
    "y_plus_minus",
    "y_minus_plus",
    "y_minus_minus",
];

#[derive(Debug, Clone, Copy)]
struct ReferenceCheckpoint {
    step_index: usize,
    expected_state: HawkesExcitationState,
}

#[derive(Debug, Clone)]
struct ReferenceScenarioFixture {
    scenario_id: &'static str,
    initial_state: HawkesExcitationState,
    kernel_parameters: HawkesKernelParameters,
    transition_steps: Vec<HawkesTransitionStep>,
    checkpoints: Vec<ReferenceCheckpoint>,
}

#[derive(Debug, Clone, PartialEq)]
struct StateMismatchDiagnostic {
    scenario_id: &'static str,
    step_index: usize,
    component_identifier: &'static str,
    expected_value: f64,
    actual_value: f64,
    absolute_difference: f64,
    tolerance: f64,
}

fn reference_kernel_parameters() -> HawkesKernelParameters {
    HawkesKernelParameters::try_new(
        std::f64::consts::LN_2,
        HawkesKernelLoadings {
            plus_plus: 0.30,
            plus_minus: 0.10,
            minus_plus: 0.20,
            minus_minus: 0.40,
        },
        HawkesKernelLoadings {
            plus_plus: 0.05,
            plus_minus: 0.02,
            minus_plus: 0.03,
            minus_minus: 0.04,
        },
    )
    .expect("reference kernel fixture must be valid")
}

fn drift_only_fixture() -> ReferenceScenarioFixture {
    ReferenceScenarioFixture {
        scenario_id: "drift_only",
        initial_state: HawkesExcitationState {
            y_plus_plus: 1.0,
            y_plus_minus: 0.5,
            y_minus_plus: 0.25,
            y_minus_minus: 0.75,
        },
        kernel_parameters: reference_kernel_parameters(),
        transition_steps: vec![
            HawkesTransitionStep::new(1.0, None, None),
            HawkesTransitionStep::new(1.0, None, None),
            HawkesTransitionStep::new(0.0, None, None),
        ],
        checkpoints: vec![
            ReferenceCheckpoint {
                step_index: 1,
                expected_state: HawkesExcitationState {
                    y_plus_plus: 0.5,
                    y_plus_minus: 0.25,
                    y_minus_plus: 0.125,
                    y_minus_minus: 0.375,
                },
            },
            ReferenceCheckpoint {
                step_index: 2,
                expected_state: HawkesExcitationState {
                    y_plus_plus: 0.25,
                    y_plus_minus: 0.125,
                    y_minus_plus: 0.0625,
                    y_minus_minus: 0.1875,
                },
            },
            ReferenceCheckpoint {
                step_index: 3,
                expected_state: HawkesExcitationState {
                    y_plus_plus: 0.25,
                    y_plus_minus: 0.125,
                    y_minus_plus: 0.0625,
                    y_minus_minus: 0.1875,
                },
            },
        ],
    }
}

fn endogenous_event_only_fixture() -> ReferenceScenarioFixture {
    ReferenceScenarioFixture {
        scenario_id: "endogenous_event_only",
        initial_state: HawkesExcitationState {
            y_plus_plus: 0.2,
            y_plus_minus: 0.4,
            y_minus_plus: 0.6,
            y_minus_minus: 0.8,
        },
        kernel_parameters: reference_kernel_parameters(),
        transition_steps: vec![
            HawkesTransitionStep::new(0.0, Some(HawkesEventSide::Buy), None),
            HawkesTransitionStep::new(0.0, Some(HawkesEventSide::Sell), None),
            HawkesTransitionStep::new(0.0, Some(HawkesEventSide::Buy), None),
        ],
        checkpoints: vec![
            ReferenceCheckpoint {
                step_index: 1,
                expected_state: HawkesExcitationState {
                    y_plus_plus: 0.5,
                    y_plus_minus: 0.4,
                    y_minus_plus: 0.8,
                    y_minus_minus: 0.8,
                },
            },
            ReferenceCheckpoint {
                step_index: 2,
                expected_state: HawkesExcitationState {
                    y_plus_plus: 0.5,
                    y_plus_minus: 0.5,
                    y_minus_plus: 0.8,
                    y_minus_minus: 1.2,
                },
            },
            ReferenceCheckpoint {
                step_index: 3,
                expected_state: HawkesExcitationState {
                    y_plus_plus: 0.8,
                    y_plus_minus: 0.5,
                    y_minus_plus: 1.0,
                    y_minus_minus: 1.2,
                },
            },
        ],
    }
}

fn exogenous_shock_only_fixture() -> ReferenceScenarioFixture {
    ReferenceScenarioFixture {
        scenario_id: "exogenous_shock_only",
        initial_state: HawkesExcitationState {
            y_plus_plus: 0.1,
            y_plus_minus: 0.2,
            y_minus_plus: 0.3,
            y_minus_minus: 0.4,
        },
        kernel_parameters: reference_kernel_parameters(),
        transition_steps: vec![
            HawkesTransitionStep::new(0.0, None, Some(1.0)),
            HawkesTransitionStep::new(0.0, None, Some(0.5)),
            HawkesTransitionStep::new(0.0, None, Some(2.0)),
        ],
        checkpoints: vec![
            ReferenceCheckpoint {
                step_index: 1,
                expected_state: HawkesExcitationState {
                    y_plus_plus: 0.15,
                    y_plus_minus: 0.22,
                    y_minus_plus: 0.33,
                    y_minus_minus: 0.44,
                },
            },
            ReferenceCheckpoint {
                step_index: 2,
                expected_state: HawkesExcitationState {
                    y_plus_plus: 0.175,
                    y_plus_minus: 0.23,
                    y_minus_plus: 0.345,
                    y_minus_minus: 0.46,
                },
            },
            ReferenceCheckpoint {
                step_index: 3,
                expected_state: HawkesExcitationState {
                    y_plus_plus: 0.275,
                    y_plus_minus: 0.27,
                    y_minus_plus: 0.405,
                    y_minus_minus: 0.54,
                },
            },
        ],
    }
}

fn mixed_update_path_fixture() -> ReferenceScenarioFixture {
    ReferenceScenarioFixture {
        scenario_id: "mixed_update_path",
        initial_state: HawkesExcitationState {
            y_plus_plus: 1.0,
            y_plus_minus: 0.0,
            y_minus_plus: 0.5,
            y_minus_minus: 0.25,
        },
        kernel_parameters: reference_kernel_parameters(),
        transition_steps: vec![
            HawkesTransitionStep::new(1.0, Some(HawkesEventSide::Buy), Some(1.5)),
            HawkesTransitionStep::new(0.0, Some(HawkesEventSide::Sell), None),
            HawkesTransitionStep::new(1.0, None, Some(0.5)),
            HawkesTransitionStep::new(0.0, Some(HawkesEventSide::Buy), None),
        ],
        checkpoints: vec![
            ReferenceCheckpoint {
                step_index: 1,
                expected_state: HawkesExcitationState {
                    y_plus_plus: 0.875,
                    y_plus_minus: 0.03,
                    y_minus_plus: 0.495,
                    y_minus_minus: 0.185,
                },
            },
            ReferenceCheckpoint {
                step_index: 3,
                expected_state: HawkesExcitationState {
                    y_plus_plus: 0.4625,
                    y_plus_minus: 0.075,
                    y_minus_plus: 0.2625,
                    y_minus_minus: 0.3125,
                },
            },
            ReferenceCheckpoint {
                step_index: 4,
                expected_state: HawkesExcitationState {
                    y_plus_plus: 0.7625,
                    y_plus_minus: 0.075,
                    y_minus_plus: 0.4625,
                    y_minus_minus: 0.3125,
                },
            },
        ],
    }
}

fn run_transition_schedule(
    initial_state: HawkesExcitationState,
    kernel_parameters: HawkesKernelParameters,
    transition_steps: &[HawkesTransitionStep],
) -> Result<Vec<HawkesExcitationState>, HawkesTransitionError> {
    let mut trajectory = Vec::with_capacity(transition_steps.len());
    let mut next_state = initial_state;

    for transition_step in transition_steps {
        next_state = next_state.apply_transition_step(kernel_parameters, *transition_step)?;
        trajectory.push(next_state);
    }

    Ok(trajectory)
}

fn collect_checkpoint_mismatches(
    scenario_id: &'static str,
    checkpoints: &[ReferenceCheckpoint],
    observed_trajectory: &[HawkesExcitationState],
    tolerance: f64,
) -> Vec<StateMismatchDiagnostic> {
    let mut mismatches = Vec::new();

    for checkpoint in checkpoints {
        let observed_state = observed_trajectory
            .get(checkpoint.step_index.saturating_sub(1))
            .expect("checkpoint step index must refer to an observed transition state");

        let expected_components = checkpoint.expected_state.as_model_ordered_array();
        let observed_components = observed_state.as_model_ordered_array();

        for ((component_identifier, expected_value), actual_value) in COMPONENT_IDENTIFIERS
            .iter()
            .zip(expected_components.iter())
            .zip(observed_components.iter())
        {
            let absolute_difference = (actual_value - expected_value).abs();
            if absolute_difference > tolerance {
                mismatches.push(StateMismatchDiagnostic {
                    scenario_id,
                    step_index: checkpoint.step_index,
                    component_identifier,
                    expected_value: *expected_value,
                    actual_value: *actual_value,
                    absolute_difference,
                    tolerance,
                });
            }
        }
    }

    mismatches
}

fn format_mismatch_diagnostics(mismatches: &[StateMismatchDiagnostic]) -> String {
    let mut output = String::new();

    for mismatch in mismatches {
        writeln!(
            output,
            "scenario={} step={} component={} expected={} actual={} absolute_difference={} tolerance={}",
            mismatch.scenario_id,
            mismatch.step_index,
            mismatch.component_identifier,
            mismatch.expected_value,
            mismatch.actual_value,
            mismatch.absolute_difference,
            mismatch.tolerance
        )
        .expect("in-memory write should never fail");
    }

    output
}

fn seeded_transition_schedule(seed: u64, step_count: usize) -> Vec<HawkesTransitionStep> {
    let mut next_seed = seed;
    let mut transition_steps = Vec::with_capacity(step_count);

    for _ in 0..step_count {
        next_seed = next_seed.wrapping_mul(6364136223846793005).wrapping_add(1);

        let delta_t = match next_seed % 3 {
            0 => 0.0,
            1 => 0.5,
            _ => 1.0,
        };
        let endogenous_event_side = match (next_seed / 3) % 3 {
            0 => None,
            1 => Some(HawkesEventSide::Buy),
            _ => Some(HawkesEventSide::Sell),
        };
        let exogenous_news_severity_mark = match (next_seed / 9) % 4 {
            0 => None,
            1 => Some(0.25),
            2 => Some(0.75),
            _ => Some(1.25),
        };

        transition_steps.push(HawkesTransitionStep::new(
            delta_t,
            endogenous_event_side,
            exogenous_news_severity_mark,
        ));
    }

    transition_steps
}

#[test]
fn hawkes_reference_scenarios_match_expected_checkpoints() {
    let fixtures = vec![
        drift_only_fixture(),
        endogenous_event_only_fixture(),
        exogenous_shock_only_fixture(),
        mixed_update_path_fixture(),
    ];

    let mut all_mismatches = Vec::new();

    for fixture in fixtures {
        let trajectory = run_transition_schedule(
            fixture.initial_state,
            fixture.kernel_parameters,
            &fixture.transition_steps,
        )
        .expect("reference transition schedule should succeed");

        let mismatches = collect_checkpoint_mismatches(
            fixture.scenario_id,
            &fixture.checkpoints,
            &trajectory,
            STATE_TOLERANCE,
        );

        all_mismatches.extend(mismatches);
    }

    assert!(
        all_mismatches.is_empty(),
        "equation-level reference checkpoints diverged\n{}",
        format_mismatch_diagnostics(&all_mismatches)
    );
}

#[test]
fn hawkes_reference_mismatch_diagnostics_include_step_and_component_identifiers() {
    let fixture = mixed_update_path_fixture();
    let trajectory = run_transition_schedule(
        fixture.initial_state,
        fixture.kernel_parameters,
        &fixture.transition_steps,
    )
    .expect("reference transition schedule should succeed");

    let mut mismatched_checkpoints = fixture.checkpoints.clone();
    mismatched_checkpoints[1].expected_state.y_plus_minus += 0.01;

    let mismatches = collect_checkpoint_mismatches(
        fixture.scenario_id,
        &mismatched_checkpoints,
        &trajectory,
        STATE_TOLERANCE,
    );

    assert_eq!(mismatches.len(), 1);
    assert_eq!(mismatches[0].scenario_id, "mixed_update_path");
    assert_eq!(mismatches[0].step_index, 3);
    assert_eq!(mismatches[0].component_identifier, "y_plus_minus");

    let rendered_diagnostics = format_mismatch_diagnostics(&mismatches);
    assert!(rendered_diagnostics.contains("scenario=mixed_update_path"));
    assert!(rendered_diagnostics.contains("step=3"));
    assert!(rendered_diagnostics.contains("component=y_plus_minus"));
}

#[test]
fn seeded_hawkes_transition_schedule_and_state_path_are_repeatable() {
    let seed = 2026_02_19_u64;
    let step_count = 16;

    let initial_state = HawkesExcitationState {
        y_plus_plus: 0.55,
        y_plus_minus: 0.15,
        y_minus_plus: 0.35,
        y_minus_minus: 0.75,
    };
    let kernel_parameters = reference_kernel_parameters();

    let first_schedule = seeded_transition_schedule(seed, step_count);
    let second_schedule = seeded_transition_schedule(seed, step_count);

    assert_eq!(first_schedule, second_schedule);

    let first_path = run_transition_schedule(initial_state, kernel_parameters, &first_schedule)
        .expect("first seeded scenario run should succeed");
    let second_path = run_transition_schedule(initial_state, kernel_parameters, &second_schedule)
        .expect("second seeded scenario run should succeed");

    assert_eq!(first_path, second_path);
}
