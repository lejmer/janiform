## Calibration Candidate Generation For News Arrivals Marks And Coupling

### Description
Define and implement the candidate-generation planning path for news-process parameters so return-components calibration uses data-derived known and surprise arrival, mark, and coupling candidates.

### Implementation details
- Refactor `crates/core/src/calibration/stages/return_components.rs` candidate-generation planning contracts so known-news and surprise-news parameters are generated from observed signals instead of static candidate fixtures.
- Define one canonical input payload for candidate generation containing known-news calendars, realized known-news return samples, detected intraday jump candidates, and detected close-open gap candidates.
- Add explicit candidate families for known-news arrival controls, surprise-news arrival controls, return-jump mark scale controls, and news-to-Hawkes activity-coupling controls.
- Define deterministic preprocessing and binning rules for jump and gap extraction so repeated runs on identical inputs produce identical candidate sets.
- Update artifact and stage-handoff planning schemas so candidate provenance includes source windows, filters, summary statistics, and deterministic seed metadata.
- Add objective-term planning alignment so candidate evaluation reports separate fit metrics for open-jump behavior, closed-gap behavior, and post-shock activity response.

### Acceptance Criteria
- Calibration planning defines candidate-generation inputs for known-news calendars, observed return jumps, and observed close-open gaps.
- Candidate-generation outputs include parameters for surprise-arrival intensity, mark laws, and news-to-Hawkes coupling.
- Return-components stage planning integrates candidate-generation outputs without relying on manual static candidate fixtures.
- Objective-term planning includes explicit metrics for open-jump fit, closed-gap fit, and activity-coupling fit.

### Unit tests
- Candidate-generation contract validation for required inputs and output payload completeness.
- Deterministic candidate enumeration behavior under fixed seeds and identical data inputs.
- Stage-integration tests for candidate-generation handoff into return-components selection.
- Contract tests for candidate provenance fields and metric-bucket completeness.

### Notes/Constraints
- Pre-v0.1 policy applies: breaking contract and schema changes are allowed when they improve correctness and clarity.
- Do not bump schema versions. We don't have users yet. We can do whatever we want, no consequences. Not even an alpha version of the library has been released yet so we don't have to bump schemas during this refactor. All schemas should be called v1 even after the refactor.
- Keep candidate-generation contracts explicit so orchestration and artifact layers can persist and reload them cleanly.
