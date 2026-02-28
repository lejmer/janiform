## Verification Regression And Fixture Alignment For News Semantics

### Description
Refactor verification harnesses and regression fixtures so validation explicitly covers known and surprise news, open-window direct jumps, closed-window gaps, and activity-coupling effects.

### Implementation details
- Refactor verification fixture catalogs to include explicit scenario classes for known-only, surprise-only, and mixed-news flows with both open-window and closed-window event timings.
- Update regression harness contracts so expected outputs include component-level assertions for trading return, open-jump contribution, closed-gap contribution, and activity-coupling response.
- Extend diagnostics outputs in stylized-fact and scale-consistency harnesses to tag mismatches by semantic bucket instead of only aggregate error classes.
- Add deterministic replay fixtures that bind event calendars, jump and gap observations, and candidate-selection metadata to one reproducible baseline per scenario class.
- Align naming and metadata keys across calibration, simulation, risk, and pricing verification outputs so cross-harness diffing uses one canonical vocabulary.
- Add explicit traceability from fixture IDs to issue-level semantics to reduce ambiguity during triage and acceptance review.

### Acceptance Criteria
- Verification harness planning includes scenarios for known-only, surprise-only, and mixed-news flows.
- Regression fixtures include explicit expectations for open-jump and closed-gap component behavior.
- Stylized-fact and consistency harness planning references the corrected component semantics.
- Failure diagnostics and diff reports isolate mismatches by activity coupling, open-jump, and closed-gap categories.

### Unit tests
- Regression harness tests for open-window direct-jump behavior and closed-window gap behavior.
- Deterministic fixture replay and diagnostics-ordering checks.
- Cross-harness consistency tests for shared naming and component metadata.
- Schema validation tests for semantic-bucketed diagnostics payloads.

### Notes/Constraints
- Pre-v0.1 policy applies: breaking contract and schema changes are allowed when they improve correctness and clarity.
- Keep fixture sets compact but representative across session boundaries and news-arrival classes.
