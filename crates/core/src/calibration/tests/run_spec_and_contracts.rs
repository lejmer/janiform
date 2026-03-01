use super::support::*;

#[test]
fn calibration_run_spec_round_trip_serialization_preserves_contract_fields() {
    let run_spec = build_valid_run_spec();

    assert_eq!(
        CalibrationRunSpec::schema_version(),
        CALIBRATION_RUN_SPEC_SCHEMA_VERSION
    );

    let serialized = serde_json::to_value(&run_spec).expect("run spec should serialize");
    assert_eq!(serialized["run_id"], json!("run-001"));
    assert_eq!(serialized["execution_mode"], json!("memory"));
    assert_eq!(serialized["selected_stages"][0], json!("seasonality"));
    assert_eq!(
        serialized["optimizer_settings_by_stage"][1]["stage"],
        json!("hawkes")
    );
    assert_eq!(serialized["seed_settings"]["global_seed"], json!(17));
    assert!(serialized.get("stage_handoffs").is_none());
    assert!(serialized.get("resume_settings").is_none());

    let parsed: CalibrationRunSpec =
        serde_json::from_value(serialized).expect("run spec should deserialize");
    assert_eq!(parsed, run_spec);
}

#[test]
fn calibration_run_spec_deserialization_rejects_invalid_execution_mode() {
    let mut serialized =
        serde_json::to_value(build_valid_run_spec()).expect("run spec fixture should serialize");
    serialized["execution_mode"] = json!("unsupported_mode");

    let parse_result = serde_json::from_value::<CalibrationRunSpec>(serialized);
    assert!(parse_result.is_err());
}

#[test]
fn calibration_stage_seed_derivation_is_deterministic_and_honors_overrides() {
    let run_spec_with_overrides = build_valid_run_spec();
    let seasonality_seed = resolve_calibration_stage_seed_metadata(
        &run_spec_with_overrides,
        CalibrationStage::Seasonality,
    );
    let hawkes_seed =
        resolve_calibration_stage_seed_metadata(&run_spec_with_overrides, CalibrationStage::Hawkes);
    let return_seed_with_overrides = resolve_calibration_stage_seed_metadata(
        &run_spec_with_overrides,
        CalibrationStage::ReturnComponents,
    );

    assert_eq!(seasonality_seed.run_global_seed, 17);
    assert_eq!(seasonality_seed.stage_seed, 101);
    assert_eq!(
        seasonality_seed.stage_seed_source,
        CalibrationStageSeedSource::StageOverride
    );
    assert_eq!(hawkes_seed.stage_seed, 202);
    assert_eq!(
        hawkes_seed.stage_seed_source,
        CalibrationStageSeedSource::StageOverride
    );
    assert_eq!(
        return_seed_with_overrides.stage_seed_source,
        CalibrationStageSeedSource::DerivedFromGlobalSeed
    );
    assert_eq!(
        return_seed_with_overrides.stage_seed,
        derive_calibration_stage_seed_from_global_seed(17, CalibrationStage::ReturnComponents)
    );
    assert_eq!(
        CALIBRATION_STAGE_SEED_DERIVATION_POLICY,
        "global_seed_splitmix64_per_stage_with_override_precedence.v1"
    );

    let run_spec_without_overrides = CalibrationRunSpec::try_new(
        "run-derived-seeds-only",
        build_data_inputs(),
        CalibrationExecutionMode::Memory,
        vec![
            CalibrationStage::Seasonality,
            CalibrationStage::Hawkes,
            CalibrationStage::ReturnComponents,
        ],
        CalibrationRunSeedSettings::try_new(17, Vec::new())
            .expect("seed settings without overrides should validate"),
        build_solver_settings_for_all_stages(),
    )
    .expect("run spec should validate");

    let derived_seasonality_seed = resolve_calibration_stage_seed_metadata(
        &run_spec_without_overrides,
        CalibrationStage::Seasonality,
    );
    let derived_hawkes_seed = resolve_calibration_stage_seed_metadata(
        &run_spec_without_overrides,
        CalibrationStage::Hawkes,
    );
    let derived_return_seed = resolve_calibration_stage_seed_metadata(
        &run_spec_without_overrides,
        CalibrationStage::ReturnComponents,
    );

    assert_eq!(
        derived_seasonality_seed,
        resolve_calibration_stage_seed_metadata(
            &run_spec_without_overrides,
            CalibrationStage::Seasonality
        )
    );
    assert_eq!(
        derived_seasonality_seed.stage_seed,
        derive_calibration_stage_seed_from_global_seed(17, CalibrationStage::Seasonality)
    );
    assert_eq!(
        derived_hawkes_seed.stage_seed,
        derive_calibration_stage_seed_from_global_seed(17, CalibrationStage::Hawkes)
    );
    assert_eq!(
        derived_return_seed.stage_seed,
        derive_calibration_stage_seed_from_global_seed(17, CalibrationStage::ReturnComponents)
    );
    assert_ne!(
        derived_seasonality_seed.stage_seed,
        derived_hawkes_seed.stage_seed
    );
    assert_ne!(
        derived_hawkes_seed.stage_seed,
        derived_return_seed.stage_seed
    );
}

#[test]
fn calibration_execution_mode_defaults_are_surface_specific_and_documented() {
    assert_eq!(
        CALIBRATION_EXECUTION_MODE_DEFAULT_POLICY,
        "cli_defaults_to_disk_python_importable_api_defaults_to_memory.v1"
    );
    assert_eq!(
        default_calibration_execution_mode_for_surface(CalibrationCallerSurface::Cli),
        CalibrationExecutionMode::Disk
    );
    assert_eq!(
        default_calibration_execution_mode_for_surface(
            CalibrationCallerSurface::PythonImportableApi
        ),
        CalibrationExecutionMode::Memory
    );
}

#[test]
fn calibration_execution_mode_resolution_honors_explicit_overrides_per_surface() {
    assert_eq!(
        resolve_calibration_execution_mode_for_surface(
            CalibrationCallerSurface::Cli,
            Some(CalibrationExecutionMode::Memory)
        ),
        CalibrationExecutionMode::Memory
    );
    assert_eq!(
        resolve_calibration_execution_mode_for_surface(
            CalibrationCallerSurface::PythonImportableApi,
            Some(CalibrationExecutionMode::Disk)
        ),
        CalibrationExecutionMode::Disk
    );
}

#[test]
fn calibration_run_spec_for_caller_surface_applies_default_execution_mode() {
    let cli_spec = CalibrationRunSpec::try_new(
        "run-cli-default",
        build_data_inputs(),
        resolve_calibration_execution_mode_for_surface(CalibrationCallerSurface::Cli, None),
        vec![CalibrationStage::Seasonality, CalibrationStage::Hawkes],
        build_seed_settings(),
        build_solver_settings_for_seasonality_and_hawkes(),
    )
    .expect("cli default run spec should validate");
    assert_eq!(cli_spec.execution_mode, CalibrationExecutionMode::Disk);

    let python_spec = CalibrationRunSpec::try_new(
        "run-python-default",
        build_data_inputs(),
        resolve_calibration_execution_mode_for_surface(
            CalibrationCallerSurface::PythonImportableApi,
            None,
        ),
        vec![CalibrationStage::Seasonality, CalibrationStage::Hawkes],
        build_seed_settings(),
        build_solver_settings_for_seasonality_and_hawkes(),
    )
    .expect("python default run spec should validate");
    assert_eq!(python_spec.execution_mode, CalibrationExecutionMode::Memory);
}

#[test]
fn calibration_run_spec_mode_extensions_support_valid_memory_and_disk_configurations() {
    let memory_run_spec = CalibrationRunSpec::builder(
        "run-memory",
        build_data_inputs(),
        CalibrationExecutionMode::Memory,
        vec![CalibrationStage::Seasonality, CalibrationStage::Hawkes],
        build_seed_settings(),
        build_solver_settings_for_seasonality_and_hawkes(),
    )
    .with_stage_handoffs(vec![build_mode_aware_stage_handoff(
        CalibrationStage::Seasonality,
        CalibrationStage::Hawkes,
        CalibrationExecutionMode::Memory,
        "artifact://seasonality/stage-001",
    )])
    .with_resume_settings(CalibrationResumeSettings::same_process(Some(
        CalibrationStage::Hawkes,
    )))
    .build()
    .expect("memory mode with in-memory handoff and same-process resume should validate");
    assert_eq!(
        memory_run_spec.execution_mode,
        CalibrationExecutionMode::Memory
    );

    let disk_run_spec = CalibrationRunSpec::builder(
        "run-disk",
        build_data_inputs(),
        CalibrationExecutionMode::Disk,
        vec![CalibrationStage::Seasonality, CalibrationStage::Hawkes],
        build_seed_settings(),
        build_solver_settings_for_seasonality_and_hawkes(),
    )
    .with_stage_handoffs(vec![build_mode_aware_stage_handoff(
        CalibrationStage::Seasonality,
        CalibrationStage::Hawkes,
        CalibrationExecutionMode::Disk,
        "artifact://seasonality/stage-001",
    )])
    .with_resume_settings(CalibrationResumeSettings::persisted_checkpoint_reference(
        "checkpoint://run-disk/last-success",
        None,
    ))
    .build()
    .expect("disk mode with persisted handoff and persisted resume should validate");
    assert_eq!(disk_run_spec.execution_mode, CalibrationExecutionMode::Disk);
}

#[test]
fn calibration_run_spec_rejects_mode_incompatible_option_settings_with_deterministic_classification()
 {
    let memory_with_persisted_handoff = CalibrationRunSpec::builder(
        "run-invalid-memory-handoff",
        build_data_inputs(),
        CalibrationExecutionMode::Memory,
        vec![CalibrationStage::Seasonality, CalibrationStage::Hawkes],
        build_seed_settings(),
        build_solver_settings_for_seasonality_and_hawkes(),
    )
    .with_stage_handoffs(vec![
        CalibrationStageHandoff::persisted_artifact_reference(
            CalibrationStage::Seasonality,
            CalibrationStage::Hawkes,
            "artifact://seasonality/stage-001",
        )
        .expect("persisted handoff fixture should be valid"),
    ])
    .build()
    .expect_err("memory mode must reject persisted artifact handoff");

    assert_eq!(
        memory_with_persisted_handoff.classification(),
        CalibrationRunSpecErrorClassification::ModeIncompatibleOptions
    );
    assert!(matches!(
        memory_with_persisted_handoff,
        CalibrationRunSpecError::InvalidStageHandoff {
            source: CalibrationStageHandoffError::PersistedArtifactHandoffRequiresDiskMode { .. }
        }
    ));

    let memory_with_persisted_resume = CalibrationRunSpec::builder(
        "run-invalid-memory-resume",
        build_data_inputs(),
        CalibrationExecutionMode::Memory,
        vec![CalibrationStage::Seasonality, CalibrationStage::Hawkes],
        build_seed_settings(),
        build_solver_settings_for_seasonality_and_hawkes(),
    )
    .with_resume_settings(CalibrationResumeSettings::persisted_checkpoint_reference(
        "checkpoint://run-memory/last-success",
        None,
    ))
    .build()
    .expect_err("memory mode must reject persisted checkpoint resume");

    assert_eq!(
        memory_with_persisted_resume.classification(),
        CalibrationRunSpecErrorClassification::ModeIncompatibleOptions
    );
    assert!(matches!(
        memory_with_persisted_resume,
        CalibrationRunSpecError::InvalidResumeSettings {
            source: CalibrationResumeSettingsError::PersistedCheckpointResumeRequiresDiskMode
        }
    ));

    let disk_with_same_process_resume = CalibrationRunSpec::builder(
        "run-invalid-disk-resume",
        build_data_inputs(),
        CalibrationExecutionMode::Disk,
        vec![CalibrationStage::Seasonality, CalibrationStage::Hawkes],
        build_seed_settings(),
        build_solver_settings_for_seasonality_and_hawkes(),
    )
    .with_resume_settings(CalibrationResumeSettings::same_process(None))
    .build()
    .expect_err("disk mode must reject same-process resume");

    assert_eq!(
        disk_with_same_process_resume.classification(),
        CalibrationRunSpecErrorClassification::ModeIncompatibleOptions
    );
    assert!(matches!(
        disk_with_same_process_resume,
        CalibrationRunSpecError::InvalidResumeSettings {
            source: CalibrationResumeSettingsError::SameProcessResumeRequiresMemoryMode
        }
    ));
}

#[test]
fn calibration_stage_graph_validation_resolves_explicit_execution_order() {
    let run_spec = CalibrationRunSpec::try_new(
        "run-ordered",
        build_data_inputs(),
        CalibrationExecutionMode::Memory,
        vec![
            CalibrationStage::ReturnComponents,
            CalibrationStage::Seasonality,
            CalibrationStage::Hawkes,
        ],
        build_seed_settings(),
        build_solver_settings_for_all_stages(),
    )
    .expect("run spec should validate");

    let stage_graph = CalibrationStageGraph::new(vec![
        CalibrationStageDependency {
            stage: CalibrationStage::Hawkes,
            depends_on: CalibrationStage::Seasonality,
        },
        CalibrationStageDependency {
            stage: CalibrationStage::ReturnComponents,
            depends_on: CalibrationStage::Hawkes,
        },
    ]);

    assert_eq!(
        CalibrationStageGraph::schema_version(),
        CALIBRATION_STAGE_GRAPH_SCHEMA_VERSION
    );

    let serialized_graph =
        serde_json::to_value(&stage_graph).expect("stage graph should serialize");
    let parsed_graph: CalibrationStageGraph =
        serde_json::from_value(serialized_graph).expect("stage graph should deserialize");
    assert_eq!(parsed_graph, stage_graph);

    let execution_order = resolve_calibration_stage_execution_order(&run_spec, &stage_graph)
        .expect("valid graph should resolve an explicit topological execution order");

    assert_eq!(
        execution_order,
        vec![
            CalibrationStage::Seasonality,
            CalibrationStage::Hawkes,
            CalibrationStage::ReturnComponents
        ]
    );
}

#[test]
fn calibration_stage_graph_rejects_unsupported_dependency_when_stage_is_unselected() {
    let run_spec = CalibrationRunSpec::try_new(
        "run-unsupported-dependency",
        build_data_inputs(),
        CalibrationExecutionMode::Memory,
        vec![CalibrationStage::Seasonality, CalibrationStage::Hawkes],
        build_seed_settings(),
        build_solver_settings_for_seasonality_and_hawkes(),
    )
    .expect("run spec should validate");
    let stage_graph = CalibrationStageGraph::new(vec![CalibrationStageDependency {
        stage: CalibrationStage::ReturnComponents,
        depends_on: CalibrationStage::Seasonality,
    }]);

    let error = resolve_calibration_stage_execution_order(&run_spec, &stage_graph)
        .expect_err("graph with unselected dependency stage must fail");
    assert!(matches!(
        error,
        CalibrationRunContractError::InvalidStageGraph {
            source: CalibrationStageGraphError::UnsupportedDependency {
                stage: CalibrationStage::ReturnComponents,
                depends_on: CalibrationStage::Seasonality,
                ..
            }
        }
    ));
}

#[test]
fn calibration_stage_graph_rejects_cycles() {
    let run_spec = CalibrationRunSpec::try_new(
        "run-cycle",
        build_data_inputs(),
        CalibrationExecutionMode::Memory,
        vec![
            CalibrationStage::Seasonality,
            CalibrationStage::Hawkes,
            CalibrationStage::ReturnComponents,
        ],
        build_seed_settings(),
        build_solver_settings_for_all_stages(),
    )
    .expect("run spec should validate");

    let stage_graph = CalibrationStageGraph::new(vec![
        CalibrationStageDependency {
            stage: CalibrationStage::Seasonality,
            depends_on: CalibrationStage::ReturnComponents,
        },
        CalibrationStageDependency {
            stage: CalibrationStage::Hawkes,
            depends_on: CalibrationStage::Seasonality,
        },
        CalibrationStageDependency {
            stage: CalibrationStage::ReturnComponents,
            depends_on: CalibrationStage::Hawkes,
        },
    ]);

    let error = resolve_calibration_stage_execution_order(&run_spec, &stage_graph)
        .expect_err("cyclic graph must fail");
    assert!(matches!(
        error,
        CalibrationRunContractError::InvalidStageGraph {
            source: CalibrationStageGraphError::CyclicDependency { .. }
        }
    ));
}

#[test]
fn calibration_run_contract_captures_solver_settings_for_auditability() {
    let run_spec = build_valid_run_spec();
    let stage_graph = CalibrationStageGraph::new(vec![
        CalibrationStageDependency {
            stage: CalibrationStage::Hawkes,
            depends_on: CalibrationStage::Seasonality,
        },
        CalibrationStageDependency {
            stage: CalibrationStage::ReturnComponents,
            depends_on: CalibrationStage::Hawkes,
        },
    ]);

    let run_contract = CalibrationRunContract::try_new(run_spec.clone(), stage_graph)
        .expect("valid run contract should build");
    assert_eq!(
        run_contract.schema_version,
        CALIBRATION_RUN_CONTRACT_SCHEMA_VERSION
    );
    assert_eq!(
        run_contract.solver_settings_capture_policy,
        CALIBRATION_SOLVER_SETTINGS_CAPTURE_POLICY
    );
    assert_eq!(
        run_contract.stage_solver_settings,
        run_spec.optimizer_settings_by_stage
    );
    assert_eq!(
        run_contract.execution_stage_order,
        vec![
            CalibrationStage::Seasonality,
            CalibrationStage::Hawkes,
            CalibrationStage::ReturnComponents
        ]
    );

    let serialized = serde_json::to_value(&run_contract).expect("run contract should serialize");
    assert_eq!(
        serialized["stage_solver_settings"]
            .as_array()
            .map(|v| v.len()),
        Some(3)
    );
    assert_eq!(
        serialized["stage_solver_settings"][0]["optimizer_settings"]["solver_name"],
        json!("lbfgs")
    );
}

#[test]
fn calibration_run_contract_construction_supports_memory_and_disk_modes() {
    let stage_graph = CalibrationStageGraph::new(vec![
        CalibrationStageDependency {
            stage: CalibrationStage::Hawkes,
            depends_on: CalibrationStage::Seasonality,
        },
        CalibrationStageDependency {
            stage: CalibrationStage::ReturnComponents,
            depends_on: CalibrationStage::Hawkes,
        },
    ]);

    for execution_mode in [
        CalibrationExecutionMode::Memory,
        CalibrationExecutionMode::Disk,
    ] {
        let run_spec = build_valid_run_spec_with_mode(execution_mode);
        let run_contract = CalibrationRunContract::try_new(run_spec, stage_graph.clone())
            .expect("run contract should build for both execution modes");
        assert_eq!(run_contract.run_spec.execution_mode, execution_mode);
    }
}

#[test]
fn calibration_run_spec_rejects_missing_solver_settings_for_selected_stage() {
    let run_spec = CalibrationRunSpec::try_new(
        "run-missing-solver",
        build_data_inputs(),
        CalibrationExecutionMode::Memory,
        vec![CalibrationStage::Seasonality, CalibrationStage::Hawkes],
        build_seed_settings(),
        vec![CalibrationStageSolverSettings {
            stage: CalibrationStage::Seasonality,
            optimizer_settings: build_solver_settings(),
        }],
    );

    assert!(matches!(
        run_spec,
        Err(CalibrationRunSpecError::MissingStageSolverSettings {
            stage: CalibrationStage::Hawkes
        })
    ));
}
