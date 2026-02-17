## ISSUE-003: Buy Sell Event Jump Operators

### Description
Implement event-driven jump operators for buy and sell arrivals that apply endogenous excitation increments to the correct state components.

### Acceptance Criteria
- Buy-event jump updates apply increments defined by `k_plus = (alpha_++, 0, alpha_-+, 0)` and sell-event jump updates apply increments defined by `k_minus = (0, alpha_+-, 0, alpha_--)`
- Operators are deterministic and composable with drift transitions
- Event-side routing is explicit and rejects unsupported side values

### Unit tests
- Tests verifying exact component increments for buy and sell events and sequence tests for multiple event jumps with interleaved sides
- Negative tests for invalid event-side payloads

### Notes/Constraints
- Keep event-side mapping centralized to avoid duplicated sign logic
- Do not apply exogenous loading in these operators
