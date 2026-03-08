## Calendar Preprocessing CLI Command

### Description
Add a CLI command for timestamp-event preprocessing against the exchange calendar registry, backed by the existing public Python seasonality and calendar APIs.

This command should let users validate event timestamps, apply session filtering, and inspect preprocessing diagnostics without first writing a Python script. It is a natural utility workflow for anyone preparing seasonality, validation, or market-data inputs.

### Acceptance Criteria
- The CLI exposes a preprocessing command that accepts event-record input plus calendar-registry input.
- The command supports the existing preprocessing controls such as exchange code, timestamp field name, session filter, and sparse-hour threshold.
- Output includes the canonical preprocessing result payload and preserves the same semantics as the public Python API.
- CLI errors correctly distinguish malformed documents, invalid exchange identifiers, timezone problems, and invalid timestamps.
- Command docs include examples that show preprocessing as a standalone step before downstream seasonality or calibration workflows.

### Unit tests
- Parser tests for preprocessing-specific arguments and defaults.
- CLI tests for successful preprocessing against fixture-backed calendar and event inputs.
- Failure-path tests for unknown exchanges, missing timestamp fields, malformed timestamps, and invalid session-filter values.

### Notes/Constraints
- Reuse the existing public Python preprocessing function and calendar error taxonomy.
- Keep the command file-driven and deterministic so it can also support future workflow debugging and support triage.
