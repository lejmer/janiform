## Remaining Command Group Migration And Superseded CLI Cleanup

### Description
Migrate the remaining standalone command groups to Click, then remove stale parser-era tests, examples, and temporary docs so the repository contains only one supported CLI story after the refactor.

The redesign is not complete if only the `fit` and `simulate` families move. `presets`, `seasonality`, `hourly-factors`, `pricing`, `stylized-facts`, and `dev` also need to live on the same Click application foundation, and the repo must stop documenting or testing the superseded `argparse` architecture and removed command roots.

This issue intentionally includes cleanup. The documentation in the repo is temporary, but it still needs to stop teaching the wrong command tree and wrong implementation architecture.

### Implementation details
- Migrate these remaining top-level groups to Click:
  - `presets`
  - `seasonality`
  - `hourly-factors`
  - `pricing`
  - `stylized-facts`
  - `dev`
- Preserve their existing workflow behavior where the CLI redesign does not intentionally change it.
- Replace parser-era tests that depend on `argparse` implementation details with Click-oriented routing and help tests.
- Update CLI smoke tests, command-reference tests, example invocations, and helper utilities to the new application structure and command tree.
- Remove any remaining CLI implementation files whose only purpose was to support the old `argparse` parser tree once the surviving command groups have been migrated.
- Remove or rewrite temporary docs that still describe:
  - the old `argparse` architecture
  - the superseded `calibration` and `simulation` roots
  - `fit-and-simulate` as a supported top-level command
- Keep doc work proportional. The goal is not to perfect the prose. The goal is to ensure nothing stale remains in supported docs, examples, or contributor guidance.
- Review `README.md`, CLI guides, and any examples under `examples/` or `tests/` that invoke CLI commands so old names are eliminated completely.

### Acceptance Criteria
- All supported CLI command groups run through the Click application.
- No active CLI test suite still depends on `argparse` parser objects or the superseded parser-module layout.
- No dead parser-era CLI implementation files remain in the supported code path after the migration is complete.
- Temporary docs, examples, and README references no longer present removed command names or the old architecture as current.
- The repository contains one coherent supported CLI story after the migration, not a mixture of new Click commands and stale parser-era references.
- Cleanup remains focused on removing stale content rather than polishing temporary docs beyond what the current development phase needs.

### Unit tests
- Click-based smoke coverage for each remaining top-level group.
- Updated help and routing tests for `presets`, `seasonality`, `hourly-factors`, `pricing`, `stylized-facts`, and `dev`.
- Example or docs validation checks that fail if removed command roots still appear in maintained CLI examples.
- Regression tests ensuring removed `argparse`-specific helper assumptions are gone from CLI test support.

### Notes/Constraints
- There are no external users and no supported legacy CLI. Do not add migration docs, alias tables, or compatibility wrappers.
- Be aggressive about deleting stale references. Temporary documentation is allowed to stay rough, but it is not allowed to stay wrong.
