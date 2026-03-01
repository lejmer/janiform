# Staged Calibration Failure Playbook

This playbook defines deterministic triage and remediation for staged calibration orchestration failures.

## Scope

- Runner: `run_staged_calibration_with_checkpointing`
- Report type: `CalibrationStagedRunReport`
- Report schema: `calibration.orchestration.staged_run.report.v1`
- Failure triage policy: `mode_aware_failure_kind_and_checkpoint_context.v1`

The report is machine-readable JSON through `serde` serialization and human-readable through this guide.

## Required triage metadata

On every failure, capture these fields from `CalibrationStagedRunReport`:

- `schema_version`
- `failure_triage_policy`
- `run_id`
- `execution_mode`
- `started_from_stage`
- `resumed_from_checkpoint_reference`
- `resumed_from_last_successful_stage`
- `stage_reports` (including stage-level `schema_version` and per-stage metadata)
- `emitted_checkpoints`
- `failure.failure_kind`

## Failure classes and operator actions

### `invalid_run_contract`
- Triage:
  - Check stale or mismatched contract schema versions.
  - Confirm stage order equals `resolve_calibration_stage_execution_order`.
- Remediation:
  - Rebuild the run contract from the current run spec and stage graph.
  - Re-run with the rebuilt contract only.

### `missing_memory_checkpoint_store`
- Triage:
  - Confirm `execution_mode=memory`.
  - Confirm resume source is `same_process`.
- Remediation:
  - Pass `CalibrationMemoryCheckpointStore` to the runner.
  - If cross-process resume is required, switch to `execution_mode=disk`.

### `missing_disk_checkpoint_directory`
- Triage:
  - Confirm `execution_mode=disk`.
  - Confirm directory argument was provided to the runner.
- Remediation:
  - Provide a writable checkpoint directory.
  - Re-run from the beginning or from a persisted checkpoint reference.

### `missing_resume_checkpoint_for_run`
- Triage:
  - Confirm in-memory store has prior checkpoints for `run_id`.
  - Confirm the previous run emitted at least one successful stage checkpoint.
- Remediation:
  - Execute a non-resume run first to seed checkpoints.
  - Retry same-process resume in the same process lifetime.

### `missing_stage_input`
- Triage:
  - Identify missing stage from `failure.stage`.
  - Confirm staged inputs include the required payload for that stage.
- Remediation:
  - Provide missing stage input and rerun.
  - For partial reruns, resume from a checkpoint that already covers upstream stages.

### `missing_in_memory_upstream_payload`
- Triage:
  - Confirm in-memory handoff route is expected for this stage edge.
  - Verify upstream stage was executed in current process before dependent stage.
- Remediation:
  - Run upstream stage in-process before dependent stage.
  - Or switch handoff route to persisted artifact reference in disk mode.

### `invalid_checkpoint_reference`
- Triage:
  - Validate checkpoint URI format `checkpoint://{run_id}/{stage-token-or-last-success}`.
- Remediation:
  - Correct the checkpoint reference string.
  - Re-run resume with the corrected reference.

### `checkpoint_load_failed`
- Triage:
  - Check whether checkpoint file exists at resolved path.
  - Check filesystem read permissions and integrity.
- Remediation:
  - Restore missing checkpoint artifact or point to an available checkpoint.
  - If artifact is unavailable, rerun calibration from the beginning.

### `checkpoint_compatibility_failed`
- Triage:
  - Compare checkpoint `run_id`, `execution_mode`, schema versions, and stage order with current run contract.
  - Check `resume_from_stage` exists in execution order.
- Remediation:
  - Use a checkpoint produced by the same run contract configuration.
  - Use mode-compatible resume source:
    - `memory`: same-process only
    - `disk`: persisted checkpoint reference only

### `checkpoint_persist_failed`
- Triage:
  - Check checkpoint directory write permissions and free space.
  - Check filesystem behavior for atomic rename.
- Remediation:
  - Fix directory permissions or storage capacity.
  - Re-run from last successful checkpoint or from start.

### `missing_checkpoint_metadata_for_successful_stage`
- Triage:
  - Confirm successful stage report includes metadata.
  - Confirm stage metadata contains seed and solver settings.
- Remediation:
  - Treat as contract violation and fix stage report emission.
  - Re-run after patching.

### `stage_execution_failed`
- Triage:
  - Identify failing stage from `failure.stage`.
  - Inspect that stage report in `stage_reports` for stage-specific failure details.
- Remediation:
  - Apply stage-specific fix, then rerun:
    - same-process resume in `memory` mode
    - persisted-checkpoint resume in `disk` mode

## Machine-readable playbook

Machine-readable playbook entries for automation are in:

- `docs/development/guides/staged_calibration_failure_playbook.json`
