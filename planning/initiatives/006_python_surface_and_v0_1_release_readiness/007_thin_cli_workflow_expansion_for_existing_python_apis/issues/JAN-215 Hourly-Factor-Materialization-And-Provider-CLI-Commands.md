## Hourly Factor Materialization And Provider CLI Commands

### Description
Add CLI coverage for the existing public hourly-factor workflows, including materializing hourly seasonality factors, materializing hourly news-intensity factors, and building or providing the combined hourly-factor contract consumed by simulation and pricing workflows.

This area is already public in Python, but it is not yet scriptable from the CLI. The command design should stay task-oriented and avoid exposing raw internal native payloads directly.

### Acceptance Criteria
- The CLI exposes commands for the main hourly-factor workflows backed by the existing public Python API.
- Users can materialize hourly `s_h`, materialize hourly `nu_h`, and produce the combined hourly-factor contract through file-driven inputs.
- Command arguments clearly distinguish explicit-source and materialized-source paths where both are supported.
- Output payloads preserve the canonical hourly-factor contract and materialization report shapes already used by the Python library.
- Docs and examples explain how hourly-factor commands integrate with simulation and pricing workflows.

### Unit tests
- Parser tests covering the selected command structure and source-kind specific argument sets.
- CLI tests for successful hourly seasonality materialization, hourly news-intensity materialization, and combined contract generation from fixture-backed inputs.
- Failure-path tests for invalid ranges, missing required source inputs, and malformed calendar or timestamp data.

### Notes/Constraints
- Keep the first CLI iteration narrow and explicit rather than trying to cover every possible public helper in `janiform.hourly_factors`.
- Prefer commands that produce reusable artifacts for downstream workflows over commands that expose only intermediate implementation details.
