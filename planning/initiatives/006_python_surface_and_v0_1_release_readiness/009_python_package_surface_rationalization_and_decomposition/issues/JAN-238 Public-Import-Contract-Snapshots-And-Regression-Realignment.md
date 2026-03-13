## Public Import Contract Snapshots And Regression Realignment

### Description
Update the public-surface regression suite so it validates the intended mix of curated root exports and explicit public submodules instead of locking the repo into the current accidental namespace policy.

### Implementation details
- Redesign public API surface snapshots to record the intended public package roots and the intended public submodules for calibration, simulation, and CLI.
- Remove checks whose only purpose is to discourage public submodule imports for workflow packages.
- Add checks that verify underscore-prefixed internals are not accidentally presented as supported public modules unless they are deliberately retained as private helpers.
- Ensure import-path regression coverage includes both public convenience imports and explicit domain-module imports where the new policy says both are supported.
- Update any docs-validation checks and examples-validation checks that currently assume root-only imports are the only correct style.

### Acceptance Criteria
- Public-surface regression tests encode the new namespace policy rather than the superseded facade-only policy.
- Snapshot outputs distinguish between curated root exports and supported public submodule paths.
- Regression coverage catches accidental exposure of true private helpers without blocking intentional package decomposition.
- Contract and docs validation tooling remains simple enough to maintain during ongoing pre-v0.1 refactors.

### Unit tests
- Snapshot and regression checks for package roots and public submodules.
- Negative tests for accidental exposure of underscore-prefixed helper modules through docs or examples.
- Smoke coverage that imports representative symbols through both curated root and explicit submodule paths where supported.

### Notes/Constraints
- Keep the checks high-signal. The goal is to protect intended structure, not to turn package organization into brittle ceremony.
- Coordinate with Project 001 contract-guardrail work rather than creating a second conflicting API-governance mechanism.
