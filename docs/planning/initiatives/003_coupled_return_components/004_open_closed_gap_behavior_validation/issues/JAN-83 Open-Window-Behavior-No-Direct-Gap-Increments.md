## Open Window Behavior No Direct Gap Increments

### Description
Implement targeted validation checks that ensure news during market-open windows affects activity coupling but does not add direct gap increments.

### Acceptance Criteria
- Open-window scenarios show updates to activity-related coupling path
- Open-window scenarios assert zero direct contribution from `G` component
- Validation output separates activity effect from direct gap effect
- Failures identify offending event timestamps and scenario IDs

### Unit tests
- Mixed-event tests where only open-window events are present
- Deterministic open-window test cases with expected zero gap contribution
- Regression tests that fail when open-window gap leakage appears

### Notes/Constraints
- Keep checks deterministic and scenario-driven
- Reuse shared coupling pipeline interfaces
