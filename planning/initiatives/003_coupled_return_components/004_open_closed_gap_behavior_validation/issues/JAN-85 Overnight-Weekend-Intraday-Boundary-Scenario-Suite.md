## Overnight Weekend Intraday Boundary Scenario Suite

### Description
Build scenario coverage for overnight, weekend, and intraday boundaries to verify correct open versus closed behavior at session edges.

### Acceptance Criteria
- Scenario suite includes boundary-adjacent events before and after session close and open
- Weekend and overnight windows are explicitly covered with expected outcomes
- Boundary semantics for exact transition timestamps are documented and tested
- Scenario data supports extension to additional exchanges

### Unit tests
- Weekend-case tests confirming persistent closed-window handling and overnight-case tests for close-to-open transition behavior
- Boundary tests for events at exact session-close and session-open timestamps

### Notes/Constraints
- Keep boundary policies consistent with calendar service rules
- Use data-driven definitions instead of hardcoded timestamp checks
