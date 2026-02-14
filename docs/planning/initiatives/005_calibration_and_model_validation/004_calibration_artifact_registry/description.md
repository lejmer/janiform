## Project 004: Calibration Artifact Registry

### Context and requirements
- Define artifact schemas for datasets, fitted parameters, diagnostics, and metadata.
- Implement persistence and reload paths for calibration outputs and comparisons.
- Support reproducibility and traceability for every calibration run.

### Definition of Done
- Artifact format is documented and versioned with backward-compatibility policy for v0.x.
- Calibration runs persist complete artifacts that can be reloaded for validation and pricing.
- Tests verify round-trip integrity and schema validation failures.

### Agent implementation notes
- Keep artifact metadata explicit for code version, data snapshot, and solver configuration.
- Provide lightweight indexing helpers for experiment lookup.

### Initiative alignment
- Delivers Initiative 005 success signal for versioned and reloadable calibration artifacts.
- Enables longitudinal model quality tracking across iterations.
