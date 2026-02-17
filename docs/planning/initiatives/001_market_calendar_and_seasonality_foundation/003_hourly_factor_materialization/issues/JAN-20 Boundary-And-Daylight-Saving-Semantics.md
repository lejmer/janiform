## Boundary And Daylight Saving Semantics

### Description
Define and implement deterministic handling for hourly interval boundaries during daylight saving transitions and other timezone anomalies.

### Acceptance Criteria
- Boundary policy is documented for spring-forward and fall-back transitions
- Hourly factor generation remains deterministic during timezone offset changes
- Duplicate or skipped local-hour scenarios are handled with explicit rules
- Behavior is consistent across supported exchanges and date ranges

### Unit tests
- Spring-forward days with missing local-hour intervals and fall-back days with repeated local-hour intervals
- Boundary tests at transition hours verifying stable bucket assignment

### Notes/Constraints
- Ensure policy is consumable by both Rust and Python call paths
- Centralize timezone transition handling to avoid policy drift
