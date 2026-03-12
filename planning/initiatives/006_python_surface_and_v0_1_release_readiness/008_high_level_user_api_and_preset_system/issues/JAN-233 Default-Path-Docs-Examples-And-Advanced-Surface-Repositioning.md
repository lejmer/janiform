## Default Path Docs Examples And Advanced Surface Repositioning

### Description
Rewrite the primary documentation and examples around the new high-level default workflow so Janiform onboarding starts with presets, fitting, model inspection, and simulation, while staged calibration and manual runtime wiring move into explicitly advanced documentation.

### Implementation details
- Update quickstart, examples, and primary workflow docs to demonstrate:
  - market-data-first fitting through `JaniformEstimator`
  - named preset and market-profile usage
  - automatic session inference as the default common-case path
  - explicit opt-in known-news usage
  - downstream simulation and pricing from `JaniformModel`
- Reclassify staged calibration contracts, raw session JSON, and manual runtime construction as advanced or debugging material instead of first-contact onboarding.
- Add examples that show how users inspect preset definitions, resolved configuration, and fitted-model provenance after a run.
- Ensure documentation explains the small bundled v1 preset set and how users can override or replace it with custom registries.
- Coordinate with the Docusaurus docs-platform project so the published docs information architecture reflects the new default path instead of mirroring internal planning structure.

### Acceptance Criteria
- Primary docs and examples use the new high-level workflow as the default recommended Janiform experience.
- Advanced staged-calibration and manual-runtime examples remain available but are clearly marked as expert workflows.
- Docs explain preset categories, override behavior, session-inference behavior, and known-news opt-in policy in user-facing language.
- Example and docs coverage includes preset inspection and fitted-model provenance visibility.
- Release-readiness planning treats the high-level API and preset flow as first-class v0.1 evidence.

### Unit tests
- Smoke checks for the documented high-level examples and preset-inspection flows.
- Documentation validation tests for required quickstart, preset, and provenance sections.
- Regression checks ensuring advanced examples are labeled as advanced and do not replace the primary onboarding path.

### Notes/Constraints
- Keep examples realistic and concise. The goal is a short reliable path for normal users, not exhaustive coverage of every low-level option.
- Coordinate terminology and navigation with Project 004 and Project 006 documentation work so the public docs stay consistent.
