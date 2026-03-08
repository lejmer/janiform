## Report Generation Tables Plots And Artifact Export

### Description
Generate diagnostics reports as in-memory-first result objects plus optional machine-readable artifact export and human-readable tables or plots for decision workflows.

### Implementation Details
- Keep diagnostics computation and reporting contracts Rust-first in `core`.
- Implement one canonical Rust report generator that emits:
  - structured in-memory report objects for single and batched calibration runs
  - deterministic table-summary rows
  - plot-ready payloads with no rendering side effects
- Keep plot-ready payloads machine-readable and renderer-agnostic, including:
  - metric identifiers and metric-class labels (`primary_robust` versus `secondary_moment`)
  - uncertainty intervals and sample-coverage fields
  - explicit `insufficient_data` statuses and reason metadata
- Gate persisted artifact export behind `execution_mode=disk` only.
- Keep artifact paths configurable through run configuration with no hardcoded directories.
- Expose report, table, and plot-ready payloads to Python through thin bindings.
- Implement plotting as an optional Python presentation layer using Plotly over plot-ready payloads.
- Keep plotting out of Rust and avoid recomputing diagnostics in Python.

### Acceptance Criteria
- Report generator produces summary tables, plot-ready outputs, and structured in-memory report objects.
- Report outputs can be generated for single and batched calibration runs.
- Export to persisted artifacts is optional and enabled when `execution_mode=disk`.
- Output schema and metadata are machine-readable and documented.
- Reports clearly separate robust primary diagnostics from secondary L-moment and classical moment diagnostics.
- Tables and artifacts include uncertainty intervals, sample coverage metrics, and explicit `insufficient_data` statuses.
- Python plotting consumes plot-ready payloads without changing report semantics or metric classifications.

### Unit tests
- Table-summary generation on reference fixtures.
- In-memory report schema validation and required metadata checks.
- Deterministic export file structure and naming for `disk` mode.
- Validation that report rendering preserves metric-class labels and uncertainty metadata.
- Validation that Python plotting adapters preserve metric identifiers, class labels, uncertainty fields, and `insufficient_data` metadata from the Rust payload.

### Notes/Constraints
- Keep plotting layer optional so headless workflows are supported.
- Avoid hardcoded paths in export configuration.
- Keep comparison and reporting logic independent from plotting and UI concerns.
- Pre-v0.1 policy applies: breaking contract and schema changes are allowed when they improve correctness and clarity.
