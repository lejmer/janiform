use janiform_core::affine_oracle::ExponentialAffineTransformEquationSpecification;
use janiform_core::calibration::CalibrationExecutionMode;
use janiform_core::hawkes::{HawkesExcitationState, HawkesKernelLoadings, HawkesKernelParameters};
use janiform_core::news::GammaSeverityMarkConfiguration;
use janiform_core::nig::NigSharedPlusShiftConfiguration;
use janiform_core::risk::{
    SufficientStatisticsRiskIntervalFactors, SufficientStatisticsRiskModelInputs,
};
use janiform_core::verification::{
    SCALE_CONSISTENCY_EXECUTION_MODE_POLICY, SCALE_CONSISTENCY_INTERVAL_RELATIONSHIP_POLICY,
    SCALE_CONSISTENCY_PAIRED_RUN_CONTRACT_SCHEMA_VERSION,
    SCALE_CONSISTENCY_PAIRED_RUN_SPEC_SCHEMA_VERSION, SCALE_CONSISTENCY_RANDOM_SEED_POLICY,
    ScaleConsistencyArtifactOutputConfig, ScaleConsistencyPairedExperimentScenarioSpecError,
    ScaleConsistencyPairedRunContract, ScaleConsistencyPairedRunSpec,
    ScaleConsistencyPairedRunSpecError, ScaleConsistencyPairedRunSpecErrorClassification,
    ScaleConsistencyRandomSeedPolicy,
};
use serde_json::json;

#[test]
fn scale_consistency_run_spec_round_trip_serialization_preserves_contract_fields() {
    let run_spec = build_valid_run_spec();

    assert_eq!(
        ScaleConsistencyPairedRunSpec::schema_version(),
        SCALE_CONSISTENCY_PAIRED_RUN_SPEC_SCHEMA_VERSION
    );

    let serialized = serde_json::to_value(&run_spec).expect("run spec should serialize");
    assert_eq!(serialized["run_id"], json!("paired-run-001"));
    assert_eq!(serialized["execution_mode"], json!("memory"));
    assert_eq!(
        serialized["scenarios"][0]["scenario_id"],
        json!("baseline-hourly-vs-quarter-hour")
    );
    assert_eq!(
        serialized["scenarios"][0]["random_seed_policy"]["kind"],
        json!("shared_random_control")
    );
    assert!(serialized.get("artifact_output").is_none());

    let parsed: ScaleConsistencyPairedRunSpec =
        serde_json::from_value(serialized).expect("run spec should deserialize");
    assert_eq!(parsed, run_spec);
}

#[test]
fn scale_consistency_run_spec_parsing_validation_and_contract_construction_succeed_for_valid_input()
{
    let serialized =
        serde_json::to_value(build_valid_run_spec()).expect("fixture should serialize");
    let parsed: ScaleConsistencyPairedRunSpec =
        serde_json::from_value(serialized).expect("fixture should deserialize");
    parsed.validate().expect("parsed run spec should validate");

    let contract =
        ScaleConsistencyPairedRunContract::try_new(parsed).expect("run contract should validate");
    assert_eq!(
        ScaleConsistencyPairedRunContract::schema_version(),
        SCALE_CONSISTENCY_PAIRED_RUN_CONTRACT_SCHEMA_VERSION
    );
    assert_eq!(
        contract.interval_relationship_policy,
        SCALE_CONSISTENCY_INTERVAL_RELATIONSHIP_POLICY
    );
    assert_eq!(
        contract.random_seed_policy,
        SCALE_CONSISTENCY_RANDOM_SEED_POLICY
    );
    assert_eq!(
        contract.execution_mode_policy,
        SCALE_CONSISTENCY_EXECUTION_MODE_POLICY
    );
}

#[test]
fn scale_consistency_run_spec_rejects_invalid_interval_relationships() {
    let mut run_spec = build_valid_run_spec();
    run_spec.scenarios[0].fine_interval_delta = 0.3;

    let error = run_spec
        .validate()
        .expect_err("non-integer interval ratio must fail validation");

    assert!(matches!(
        error,
        ScaleConsistencyPairedRunSpecError::InvalidScenario {
            source: ScaleConsistencyPairedExperimentScenarioSpecError::IntervalDeltaRatioMismatch { .. },
            ..
        }
    ));
}

#[test]
fn scale_consistency_run_spec_rejects_implicit_fine_factor_interval_defaults() {
    let mut serialized =
        serde_json::to_value(build_valid_run_spec()).expect("fixture should serialize");
    serialized["scenarios"][0]["parameter_set"]["fine_interval_factors"]
        .as_object_mut()
        .expect("fine_interval_factors should be an object")
        .remove("interval_delta");

    let parsed: ScaleConsistencyPairedRunSpec =
        serde_json::from_value(serialized).expect("payload should deserialize");
    let error = parsed
        .validate()
        .expect_err("missing explicit factor interval delta must fail validation");

    assert!(matches!(
        error,
        ScaleConsistencyPairedRunSpecError::InvalidScenario {
            source: ScaleConsistencyPairedExperimentScenarioSpecError::InvalidParameterSet { .. },
            ..
        }
    ));
    assert!(
        error
            .to_string()
            .contains("fine_interval_factors.interval_delta must be provided explicitly"),
        "unexpected error: {error}"
    );
}

#[test]
fn scale_consistency_run_spec_rejects_mode_incompatible_artifact_output() {
    let mut run_spec = build_valid_run_spec();
    run_spec.artifact_output = Some(ScaleConsistencyArtifactOutputConfig {
        artifact_root_uri: "memory://paired-runs".to_string(),
        file_stem: "baseline".to_string(),
    });

    let error = run_spec
        .validate()
        .expect_err("memory mode must reject artifact output");

    assert_eq!(
        error.classification(),
        ScaleConsistencyPairedRunSpecErrorClassification::ModeIncompatibleOptions
    );
    assert!(matches!(
        error,
        ScaleConsistencyPairedRunSpecError::ArtifactOutputRequiresDiskMode {
            execution_mode: CalibrationExecutionMode::Memory
        }
    ));
}

#[test]
fn scale_consistency_run_spec_deserialization_rejects_missing_required_fields() {
    let mut missing_execution_mode =
        serde_json::to_value(build_valid_run_spec()).expect("fixture should serialize");
    missing_execution_mode
        .as_object_mut()
        .expect("run spec should be an object")
        .remove("execution_mode");

    let execution_mode_error =
        serde_json::from_value::<ScaleConsistencyPairedRunSpec>(missing_execution_mode)
            .expect_err("missing execution_mode must fail deserialization");
    assert!(
        execution_mode_error
            .to_string()
            .contains("missing field `execution_mode`"),
        "unexpected serde error: {execution_mode_error}"
    );

    let mut missing_parameter_set =
        serde_json::to_value(build_valid_run_spec()).expect("fixture should serialize");
    missing_parameter_set["scenarios"][0]
        .as_object_mut()
        .expect("scenario should be an object")
        .remove("parameter_set");

    let parameter_set_error =
        serde_json::from_value::<ScaleConsistencyPairedRunSpec>(missing_parameter_set)
            .expect_err("missing parameter_set must fail deserialization");
    assert!(
        parameter_set_error
            .to_string()
            .contains("missing field `parameter_set`"),
        "unexpected serde error: {parameter_set_error}"
    );
}

fn build_valid_run_spec() -> ScaleConsistencyPairedRunSpec {
    ScaleConsistencyPairedRunSpec {
        run_id: "paired-run-001".to_string(),
        execution_mode: CalibrationExecutionMode::Memory,
        scenarios: vec![build_valid_scenario()],
        artifact_output: None,
    }
}

fn build_valid_scenario()
-> janiform_core::verification::ScaleConsistencyPairedExperimentScenarioSpec {
    janiform_core::verification::ScaleConsistencyPairedExperimentScenarioSpec {
        scenario_id: "baseline-hourly-vs-quarter-hour".to_string(),
        coarse_interval_delta: 1.0,
        fine_interval_delta: 0.25,
        horizon_interval_count: 2,
        parameter_set: build_parameter_set(),
        random_seed_policy: ScaleConsistencyRandomSeedPolicy::SharedRandomControl { base_seed: 17 },
    }
}

fn build_parameter_set() -> janiform_core::verification::ScaleConsistencyExperimentParameterSet {
    janiform_core::verification::ScaleConsistencyExperimentParameterSet {
        parameter_set_id: "baseline_regime".to_string(),
        model_inputs: build_model_inputs(),
        fine_interval_factors: SufficientStatisticsRiskIntervalFactors::try_new(
            vec![1.0, 1.1, 0.9, 1.0, 1.2, 1.0, 0.95, 1.05],
            vec![0.2, 0.25, 0.15, 0.2, 0.3, 0.25, 0.2, 0.2],
            vec![0.05, 0.04, 0.03, 0.02, 0.05, 0.05, 0.04, 0.03],
            Some(0.25),
        )
        .expect("interval factors should validate"),
    }
}

fn build_model_inputs() -> SufficientStatisticsRiskModelInputs {
    let hawkes_kernel_parameters = HawkesKernelParameters::try_new(
        0.8,
        1.5,
        HawkesKernelLoadings {
            plus_plus: 0.18,
            plus_minus: 0.06,
            minus_plus: 0.09,
            minus_minus: 0.21,
        },
        HawkesKernelLoadings {
            plus_plus: 0.04,
            plus_minus: 0.02,
            minus_plus: 0.03,
            minus_minus: 0.05,
        },
    )
    .expect("hawkes parameters should validate");
    let equation_specification = ExponentialAffineTransformEquationSpecification::try_new(
        hawkes_kernel_parameters,
        GammaSeverityMarkConfiguration::try_new(2.0, 0.4)
            .expect("gamma severity marks should validate"),
    )
    .expect("equation specification should validate");
    let nig_configuration = NigSharedPlusShiftConfiguration::try_new(1.6, 0.15, 0.5, 0.0, -0.1)
        .expect("nig configuration should validate");

    SufficientStatisticsRiskModelInputs::try_new(
        equation_specification,
        nig_configuration,
        HawkesExcitationState {
            y_plus_plus: 0.05,
            y_plus_minus: 0.02,
            y_minus_plus: 0.03,
            y_minus_minus: 0.04,
        },
    )
    .expect("model inputs should validate")
}
