## Project 004: Calibration Artifact Registry

Provide the optional `disk`-mode persistence backend for calibration outputs so runs are reloadable, traceable, and comparable over time when persistence is enabled.

### Context and requirements
- Define artifact schemas for datasets, fitted parameters, diagnostics, and metadata.
- Implement persistence and reload paths for calibration outputs and comparisons when `execution_mode=disk`.
- Support reproducibility and traceability for persisted calibration runs.
- Define persistence policy explicitly: persisted artifacts are optional, default ON for CLI runs, and default OFF for imported package usage unless explicitly enabled.

### Definition of Done
- Artifact format is documented and versioned with an explicit pre-v0.1 schema-evolution policy where breaking changes are allowed.
- `disk`-mode calibration runs can persist complete artifacts that can be reloaded for validation and pricing.
- `memory`-mode calibration runs complete without requiring artifact-registry writes.
- Persistence defaults are documented and enforced: CLI defaults to persisted artifacts ON, package API defaults to persisted artifacts OFF.
- Tests verify round-trip integrity and schema validation failures for persisted artifact flows.

### Agent implementation notes
- Keep artifact metadata explicit for code version, data snapshot, and solver configuration.
- Provide lightweight indexing helpers for experiment lookup in persisted runs.

### Initiative alignment
- Delivers Initiative 005 success signal for versioned and reloadable calibration artifacts in persisted workflows.
- Enables longitudinal model quality tracking across iterations when persistence is enabled.
