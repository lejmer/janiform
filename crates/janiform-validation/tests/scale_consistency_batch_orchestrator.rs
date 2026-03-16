use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use janiform_affine_oracle::affine_oracle::ExponentialAffineTransformEquationSpecification;
use janiform_calibration_artifacts::CalibrationExecutionMode;
use janiform_engine_risk::{
    SufficientStatisticsRiskIntervalFactors, SufficientStatisticsRiskModelInputs,
};
use janiform_model_kernels::hawkes::{
    HawkesExcitationState, HawkesKernelLoadings, HawkesKernelParameters,
};
use janiform_model_kernels::news::GammaSeverityMarkConfiguration;
use janiform_model_kernels::nig::NigSharedPlusShiftConfiguration;
use janiform_validation::{
    RegressionCaseStatus, ScaleConsistencyArtifactOutputConfig,
    ScaleConsistencyBatchMemoryCheckpointStore, ScaleConsistencyBatchResumeSettings,
    ScaleConsistencyBatchRunRequest, ScaleConsistencyBatchScenarioRecordSource,
    ScaleConsistencyBatchScenarioSelection, ScaleConsistencyBatchScenarioSummaryMetrics,
    ScaleConsistencyExperimentParameterSet, ScaleConsistencyPairedExperimentScenarioSpec,
    ScaleConsistencyPairedRunContract, ScaleConsistencyPairedRunSpec,
    ScaleConsistencyRandomSeedPolicy, ScaleConsistencyScenarioExecutionOutcome,
    ScaleConsistencyScenarioExecutionRequest, ScaleConsistencyScenarioExecutor,
    run_scale_consistency_batch_orchestrator,
};

#[test]
fn scale_consistency_batch_orchestrator_preserves_spec_order_and_collates_results() {
    let run_contract = build_run_contract(
        CalibrationExecutionMode::Memory,
        vec![
            build_scenario("scenario-beta", "regime-beta", 0.5, 0.25, 3, 101),
            build_scenario("scenario-alpha", "regime-alpha", 1.0, 0.25, 2, 202),
            build_scenario("scenario-gamma", "regime-gamma", 0.75, 0.25, 2, 303),
        ],
        None,
    );
    let request = ScaleConsistencyBatchRunRequest::try_new(run_contract)
        .expect("batch request should validate");
    let executor = RecordingExecutor::new([
        outcome_entry(
            "scenario-beta",
            Ok(success_outcome(
                RegressionCaseStatus::Pass,
                6,
                6,
                0,
                0,
                Some(0.03),
            )),
        ),
        outcome_entry(
            "scenario-alpha",
            Ok(success_outcome(
                RegressionCaseStatus::Fail,
                6,
                5,
                1,
                2,
                Some(0.12),
            )),
        ),
        outcome_entry("scenario-gamma", Err("solver diverged".to_string())),
    ]);

    let report = run_scale_consistency_batch_orchestrator(&request, &executor, None)
        .expect("batch run should complete");

    assert_eq!(
        executor.calls(),
        vec![
            "scenario-beta".to_string(),
            "scenario-alpha".to_string(),
            "scenario-gamma".to_string(),
        ]
    );
    assert_eq!(report.run_id, "scale-batch-run");
    assert_eq!(report.summary.total_scenario_count, 3);
    assert_eq!(report.summary.executed_scenario_count, 3);
    assert_eq!(report.summary.resumed_scenario_count, 0);
    assert_eq!(report.summary.passed_scenario_count, 1);
    assert_eq!(report.summary.failed_scenario_count, 1);
    assert_eq!(report.summary.execution_error_scenario_count, 1);
    assert_eq!(report.scenario_reports.len(), 3);
    assert_eq!(report.scenario_reports[0].scenario_id, "scenario-beta");
    assert_eq!(report.scenario_reports[1].scenario_id, "scenario-alpha");
    assert_eq!(report.scenario_reports[2].scenario_id, "scenario-gamma");
    assert_eq!(
        report.scenario_reports[0]
            .execution_metadata
            .random_control_metadata
            .paired_run_index,
        0
    );
    assert_eq!(
        report.scenario_reports[1]
            .execution_metadata
            .random_control_metadata
            .paired_run_index,
        1
    );
    assert_eq!(
        report.scenario_reports[2].status,
        janiform_validation::ScaleConsistencyBatchScenarioStatus::ExecutionError
    );
    assert_eq!(
        report.scenario_reports[2].error_message.as_deref(),
        Some("solver diverged")
    );
}

#[test]
fn scale_consistency_batch_orchestrator_supports_explicit_scenario_filtering() {
    let run_contract = build_run_contract(
        CalibrationExecutionMode::Memory,
        vec![
            build_scenario("scenario-one", "regime-one", 1.0, 0.25, 2, 1),
            build_scenario("scenario-two", "regime-two", 0.5, 0.25, 2, 2),
            build_scenario("scenario-three", "regime-three", 0.75, 0.25, 2, 3),
        ],
        None,
    );
    let request = ScaleConsistencyBatchRunRequest::try_new(run_contract)
        .expect("batch request should validate")
        .with_scenario_selection(ScaleConsistencyBatchScenarioSelection::ScenarioIds {
            scenario_ids: vec!["scenario-three".to_string(), "scenario-one".to_string()],
        });
    let executor = RecordingExecutor::new([
        outcome_entry(
            "scenario-one",
            Ok(success_outcome(
                RegressionCaseStatus::Pass,
                4,
                4,
                0,
                0,
                Some(0.01),
            )),
        ),
        outcome_entry(
            "scenario-three",
            Ok(success_outcome(
                RegressionCaseStatus::Pass,
                4,
                4,
                0,
                0,
                Some(0.02),
            )),
        ),
    ]);

    let report = run_scale_consistency_batch_orchestrator(&request, &executor, None)
        .expect("filtered batch run should complete");

    assert_eq!(
        executor.calls(),
        vec!["scenario-one".to_string(), "scenario-three".to_string()]
    );
    assert_eq!(report.summary.total_scenario_count, 2);
    assert_eq!(
        report
            .scenario_reports
            .iter()
            .map(|report| report.scenario_id.as_str())
            .collect::<Vec<_>>(),
        vec!["scenario-one", "scenario-three"]
    );
}

#[test]
fn scale_consistency_batch_orchestrator_reruns_only_failed_scenarios_in_memory_mode() {
    let run_contract = build_run_contract(
        CalibrationExecutionMode::Memory,
        vec![
            build_scenario("scenario-a", "regime-a", 1.0, 0.25, 2, 11),
            build_scenario("scenario-b", "regime-b", 0.5, 0.25, 2, 22),
            build_scenario("scenario-c", "regime-c", 0.75, 0.25, 2, 33),
        ],
        None,
    );
    let initial_request = ScaleConsistencyBatchRunRequest::try_new(run_contract.clone())
        .expect("initial batch request should validate");
    let initial_executor = RecordingExecutor::new([
        outcome_entry(
            "scenario-a",
            Ok(success_outcome(
                RegressionCaseStatus::Pass,
                5,
                5,
                0,
                0,
                Some(0.01),
            )),
        ),
        outcome_entry(
            "scenario-b",
            Ok(success_outcome(
                RegressionCaseStatus::Fail,
                5,
                4,
                1,
                2,
                Some(0.15),
            )),
        ),
        outcome_entry("scenario-c", Err("missing comparison input".to_string())),
    ]);
    let mut checkpoint_store = ScaleConsistencyBatchMemoryCheckpointStore::new();

    let initial_report = run_scale_consistency_batch_orchestrator(
        &initial_request,
        &initial_executor,
        Some(&mut checkpoint_store),
    )
    .expect("initial batch run should complete");
    assert_eq!(initial_report.summary.failed_scenario_count, 1);
    assert_eq!(initial_report.summary.execution_error_scenario_count, 1);

    let rerun_request = ScaleConsistencyBatchRunRequest::try_new(run_contract)
        .expect("rerun batch request should validate")
        .with_resume_settings(ScaleConsistencyBatchResumeSettings::same_process())
        .with_scenario_selection(ScaleConsistencyBatchScenarioSelection::FailedFromResume);
    let rerun_executor = RecordingExecutor::new([
        outcome_entry(
            "scenario-b",
            Ok(success_outcome(
                RegressionCaseStatus::Pass,
                5,
                5,
                0,
                0,
                Some(0.04),
            )),
        ),
        outcome_entry(
            "scenario-c",
            Ok(success_outcome(
                RegressionCaseStatus::Pass,
                5,
                5,
                0,
                0,
                Some(0.03),
            )),
        ),
    ]);

    let rerun_report = run_scale_consistency_batch_orchestrator(
        &rerun_request,
        &rerun_executor,
        Some(&mut checkpoint_store),
    )
    .expect("failed-only rerun should complete");

    assert_eq!(
        rerun_executor.calls(),
        vec!["scenario-b".to_string(), "scenario-c".to_string()]
    );
    assert_eq!(rerun_report.summary.total_scenario_count, 3);
    assert_eq!(rerun_report.summary.executed_scenario_count, 2);
    assert_eq!(rerun_report.summary.resumed_scenario_count, 1);
    assert_eq!(rerun_report.summary.passed_scenario_count, 3);
    assert_eq!(rerun_report.summary.failed_scenario_count, 0);
    assert_eq!(rerun_report.summary.execution_error_scenario_count, 0);

    let resumed_success = rerun_report
        .scenario_reports
        .iter()
        .find(|report| report.scenario_id == "scenario-a")
        .expect("resumed success should be present");
    assert_eq!(
        resumed_success.execution_metadata.record_source,
        ScaleConsistencyBatchScenarioRecordSource::LoadedFromResume
    );
    let rerun_failure = rerun_report
        .scenario_reports
        .iter()
        .find(|report| report.scenario_id == "scenario-b")
        .expect("rerun failure should be present");
    assert_eq!(
        rerun_failure.execution_metadata.record_source,
        ScaleConsistencyBatchScenarioRecordSource::ExecutedInCurrentRun
    );
}

#[test]
fn scale_consistency_batch_orchestrator_supports_persisted_disk_resume() {
    let temp_directory = unique_temp_directory("scale-consistency-batch");
    let run_contract = build_run_contract(
        CalibrationExecutionMode::Disk,
        vec![
            build_scenario("scenario-left", "regime-left", 1.0, 0.25, 2, 41),
            build_scenario("scenario-right", "regime-right", 0.5, 0.25, 2, 42),
        ],
        Some(ScaleConsistencyArtifactOutputConfig {
            artifact_root_uri: temp_directory.to_string_lossy().into_owned(),
            file_stem: "disk_resume_fixture".to_string(),
        }),
    );
    let initial_request = ScaleConsistencyBatchRunRequest::try_new(run_contract.clone())
        .expect("initial disk request should validate");
    let initial_executor = RecordingExecutor::new([
        outcome_entry(
            "scenario-left",
            Ok(success_outcome(
                RegressionCaseStatus::Pass,
                4,
                4,
                0,
                0,
                Some(0.01),
            )),
        ),
        outcome_entry(
            "scenario-right",
            Ok(success_outcome(
                RegressionCaseStatus::Fail,
                4,
                3,
                1,
                1,
                Some(0.11),
            )),
        ),
    ]);

    let initial_report =
        run_scale_consistency_batch_orchestrator(&initial_request, &initial_executor, None)
            .expect("initial disk batch run should complete");
    let checkpoint_reference = initial_report
        .artifact_export
        .as_ref()
        .expect("disk run should persist artifacts")
        .checkpoint_reference
        .clone();
    assert!(PathBuf::from(&checkpoint_reference).exists());

    let rerun_request = ScaleConsistencyBatchRunRequest::try_new(run_contract)
        .expect("rerun disk request should validate")
        .with_resume_settings(
            ScaleConsistencyBatchResumeSettings::persisted_checkpoint_reference(
                checkpoint_reference.clone(),
            ),
        )
        .with_scenario_selection(ScaleConsistencyBatchScenarioSelection::FailedFromResume);
    let rerun_executor = RecordingExecutor::new([outcome_entry(
        "scenario-right",
        Ok(success_outcome(
            RegressionCaseStatus::Pass,
            4,
            4,
            0,
            0,
            Some(0.03),
        )),
    )]);

    let rerun_report =
        run_scale_consistency_batch_orchestrator(&rerun_request, &rerun_executor, None)
            .expect("persisted rerun should complete");

    assert_eq!(rerun_executor.calls(), vec!["scenario-right".to_string()]);
    assert_eq!(rerun_report.summary.total_scenario_count, 2);
    assert_eq!(rerun_report.summary.executed_scenario_count, 1);
    assert_eq!(rerun_report.summary.resumed_scenario_count, 1);
    assert_eq!(rerun_report.summary.passed_scenario_count, 2);

    if temp_directory.exists() {
        fs::remove_dir_all(&temp_directory)
            .expect("temporary export directory should be removable");
    }
}

#[test]
fn scale_consistency_batch_request_rejects_mode_incompatible_resume_options() {
    let disk_contract = build_run_contract(
        CalibrationExecutionMode::Disk,
        vec![build_scenario(
            "scenario-disk",
            "regime-disk",
            1.0,
            0.25,
            2,
            9,
        )],
        None,
    );
    let disk_error = ScaleConsistencyBatchRunRequest::try_new(disk_contract)
        .expect("disk request should validate")
        .with_resume_settings(ScaleConsistencyBatchResumeSettings::same_process())
        .validate()
        .expect_err("disk mode must reject same-process resume");
    assert!(
        disk_error
            .to_string()
            .contains("resume_settings.source=`same_process` requires execution_mode=`memory`"),
        "unexpected disk error: {disk_error}"
    );

    let memory_contract = build_run_contract(
        CalibrationExecutionMode::Memory,
        vec![build_scenario(
            "scenario-memory",
            "regime-memory",
            1.0,
            0.25,
            2,
            10,
        )],
        None,
    );
    let memory_error = ScaleConsistencyBatchRunRequest::try_new(memory_contract)
        .expect("memory request should validate")
        .with_resume_settings(
            ScaleConsistencyBatchResumeSettings::persisted_checkpoint_reference("checkpoint.json"),
        )
        .validate()
        .expect_err("memory mode must reject persisted resume");
    assert!(
        memory_error.to_string().contains(
            "resume_settings.source=`persisted_checkpoint_reference` requires execution_mode=`disk`"
        ),
        "unexpected memory error: {memory_error}"
    );
}

fn success_outcome(
    status: RegressionCaseStatus,
    total_metric_count: usize,
    computed_metric_count: usize,
    missing_metric_count: usize,
    failed_metric_count: usize,
    max_absolute_delta: Option<f64>,
) -> ScaleConsistencyScenarioExecutionOutcome {
    ScaleConsistencyScenarioExecutionOutcome {
        status,
        summary_metrics: ScaleConsistencyBatchScenarioSummaryMetrics {
            total_metric_count,
            computed_metric_count,
            missing_metric_count,
            failed_metric_count,
            max_absolute_delta,
        },
    }
}

fn outcome_entry(
    scenario_id: &str,
    outcome: Result<ScaleConsistencyScenarioExecutionOutcome, String>,
) -> (
    String,
    Result<ScaleConsistencyScenarioExecutionOutcome, String>,
) {
    (scenario_id.to_string(), outcome)
}

#[derive(Debug)]
struct RecordingExecutor {
    outcomes_by_scenario_id:
        BTreeMap<String, Result<ScaleConsistencyScenarioExecutionOutcome, String>>,
    observed_calls: RefCell<Vec<String>>,
}

impl RecordingExecutor {
    fn new<const N: usize>(
        outcomes: [(
            String,
            Result<ScaleConsistencyScenarioExecutionOutcome, String>,
        ); N],
    ) -> Self {
        Self {
            outcomes_by_scenario_id: outcomes.into_iter().collect(),
            observed_calls: RefCell::new(Vec::new()),
        }
    }

    fn calls(&self) -> Vec<String> {
        self.observed_calls.borrow().clone()
    }
}

impl ScaleConsistencyScenarioExecutor for RecordingExecutor {
    type Error = String;

    fn execute_scenario(
        &self,
        scenario: &ScaleConsistencyPairedExperimentScenarioSpec,
        request: &ScaleConsistencyScenarioExecutionRequest,
    ) -> Result<ScaleConsistencyScenarioExecutionOutcome, Self::Error> {
        self.observed_calls
            .borrow_mut()
            .push(scenario.scenario_id.clone());
        assert_eq!(
            request.random_control_metadata.paired_run_index as usize,
            request.scenario_position
        );

        self.outcomes_by_scenario_id
            .get(scenario.scenario_id.as_str())
            .cloned()
            .expect("every executed scenario should have a configured fake outcome")
    }
}

fn build_run_contract(
    execution_mode: CalibrationExecutionMode,
    scenarios: Vec<ScaleConsistencyPairedExperimentScenarioSpec>,
    artifact_output: Option<ScaleConsistencyArtifactOutputConfig>,
) -> ScaleConsistencyPairedRunContract {
    ScaleConsistencyPairedRunContract::try_new(ScaleConsistencyPairedRunSpec {
        run_id: "scale-batch-run".to_string(),
        execution_mode,
        scenarios,
        artifact_output,
    })
    .expect("paired run contract should validate")
}

fn build_scenario(
    scenario_id: &str,
    parameter_set_id: &str,
    coarse_interval_delta: f64,
    fine_interval_delta: f64,
    horizon_interval_count: usize,
    base_seed: u64,
) -> ScaleConsistencyPairedExperimentScenarioSpec {
    ScaleConsistencyPairedExperimentScenarioSpec {
        scenario_id: scenario_id.to_string(),
        coarse_interval_delta,
        fine_interval_delta,
        horizon_interval_count,
        parameter_set: build_parameter_set(parameter_set_id, fine_interval_delta),
        random_seed_policy: ScaleConsistencyRandomSeedPolicy::SharedRandomControl { base_seed },
    }
}

fn build_parameter_set(
    parameter_set_id: &str,
    fine_interval_delta: f64,
) -> ScaleConsistencyExperimentParameterSet {
    ScaleConsistencyExperimentParameterSet {
        parameter_set_id: parameter_set_id.to_string(),
        model_inputs: build_model_inputs(),
        fine_interval_factors: SufficientStatisticsRiskIntervalFactors::try_new(
            vec![1.0, 1.1, 0.9, 1.05, 1.0, 1.2, 0.95, 1.1],
            vec![0.20, 0.18, 0.17, 0.21, 0.22, 0.19, 0.18, 0.20],
            vec![0.04, 0.03, 0.05, 0.04, 0.03, 0.05, 0.04, 0.03],
            Some(fine_interval_delta),
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

fn unique_temp_directory(prefix: &str) -> PathBuf {
    let unique_suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("current time should be after unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!("{prefix}-{unique_suffix}"))
}
