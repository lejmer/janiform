## Rust-Python Reproducibility Parity Checks

### Description
Add parity checks that verify equivalent seeded simulation behavior across Rust core and Python-exposed entrypoints.

### Acceptance Criteria
- Reference seeded scenarios produce matching event tapes across Rust and Python call paths
- Parity checks include event ordering, timestamps, and side sequence
- Differences are reported with actionable diagnostics
- Parity fixtures are versioned for regression tracking
- Parity coverage includes scenarios with non-grid event-time arrivals and tied or near-tied timestamp ordering policies
- Parity coverage includes coarse-horizon outputs or derived summaries used in scale-consistency validation workflows

### Unit tests
- Failure-path tests verifying clear mismatch diagnostics
- Cross-layer parity tests for at least one short and one medium horizon scenario
- Cross-layer tests that compare aggregated tick summaries against direct coarse summaries for the same fixture inputs where coarse outputs are available

### Notes/Constraints
- Keep parity scenarios deterministic and small enough for fast CI execution
- Reuse shared fixture inputs to avoid divergence between test stacks
