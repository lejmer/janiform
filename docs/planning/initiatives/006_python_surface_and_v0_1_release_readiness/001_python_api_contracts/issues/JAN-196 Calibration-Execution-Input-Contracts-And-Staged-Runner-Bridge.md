## Calibration Execution Input Contracts And Staged Runner Bridge

### Description
Define the explicit calibration execution-input contracts and bridge layers needed for Python to drive staged calibration runs in Rust without relying on hidden dataset resolution or placeholder provenance references.

### Acceptance Criteria
- A typed Python execution-input contract exists for staged calibration and can express the concrete inputs required by the seasonality, Hawkes, and return-components stages.
- The staged calibration execution path accepts explicit stage inputs rather than attempting to infer runnable data from `trade_events_data_reference` and `option_surface_data_reference` alone.
- Dataset reference fields remain provenance and artifact metadata unless an explicit future issue introduces a documented resolver boundary.
- A canonical bridge builds the Rust calibration run spec, stage graph, staged-run inputs, and checkpoint configuration from the Python execution contract.
- The bridge supports importable API default `execution_mode=memory` and explicit override to `disk`, including explicit memory-store versus disk-directory requirements where those runtime resources are needed.
- Return-components execution inputs preserve corrected known-surprise news semantics and open-jump versus closed-gap terminology where exposed.
- PyO3 exposes a thin staged-calibration runner binding that delegates to the Rust staged runner instead of re-implementing orchestration in Python.

### Unit tests
- Contract tests for valid and invalid staged calibration execution-input construction and field-level validation failures.
- Integration tests for staged calibration execution in representative memory-mode and disk-mode runs.
- Tests verifying that Python importable API default mode remains `memory` and explicit override to `disk` is honored through the execution bridge.
- Negative tests confirming the bridge rejects attempts to execute calibration from provenance-only references without explicit stage inputs.

### Notes/Constraints
- Keep hidden I/O and undocumented dataset lookup out of the importable orchestration path.
- Reuse the existing Rust staged runner and stage-specific contracts instead of inventing parallel calibration flow glue.
- Preserve the separation between public user-facing request and response contracts and any additional execution-only contracts needed to drive the runner.
- Pre-v0.1 policy applies: breaking contract and schema changes are allowed when they improve correctness and clarity.
