## ISSUE-004: Return Increment Generation From Sampled Statistics

### Description
Generate interval return increments from sampled sufficient statistics using shared return decomposition modules and gap behavior rules.

### Acceptance Criteria
- Return generator consumes sampled `A_plus` and `A_minus` plus gap inputs and emits interval returns
- Sell-side sign conventions and gap gating rules are preserved through shared decomposition logic
- Output includes optional component breakdown for diagnostics
- Integration path is consistent with Initiative 003 decomposition contracts

### Unit tests
- Interval return generation in trading-only and mixed gap scenarios and sign-correct sell-side contribution behavior
- Decomposition-consistency across repeated runs

### Notes/Constraints
- Reuse shared decomposition interfaces rather than local math rewrites
- Keep component breakdown optional to control runtime overhead
