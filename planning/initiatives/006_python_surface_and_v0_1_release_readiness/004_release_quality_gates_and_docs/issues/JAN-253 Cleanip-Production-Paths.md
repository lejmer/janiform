## Cleanup Production Paths

### Description

We need to fix two refactor left-overs:

1. `python/janiform/_api_contract_guardrails.py (line 75)` is a shipped test/dev support rather than runtime library code. Its main entrypoints (`build_public_api_contract_snapshot`, hash/diff/manifest helpers) are only consumed by the snapshot maintenance script `scripts/update_public_api_contract_snapshot.py (line 4)` and tests `tests/test_api_contract_guardrails.py (line 10)`, `tests/test_experimental_api_surface.py (line 10)`. We have not found any production imports from the package runtime or CLI. This should be moved to `tests/` or `scripts/`.
2. `crates/core/src/calibration/artifact_registry/validation_metric.rs (line 641)`: `persist_calibration_validation_metric_artifact_to_run_directory` appears dead in production. The only call sites we found are calibration tests `crates/core/src/calibration/tests/experiment_index.rs (line 364)` and `crates/core/src/calibration/tests/artifact_regression_suite.rs (line 698)`, plus the re-export in test support. There are no runtime callers under `crates/core/src/calibration/**`, and the existing `#[allow(dead_code)]` is another sign this helper survived a refactor.

### Acceptance Criteria
- If the dead is definitely dead, the code is removed.
