## Solver API For Pricing And Risk Consumers

### Description
Expose a single solver API usable by both characteristic-function pricing and sufficient-statistics risk sampling consumers.

### Acceptance Criteria
- Public solver API supports pricing and risk parameterization use cases
- Public solver API supports configurable interval length `Delta` rather than an hourly-only step
- API output includes transform value and intermediate outputs needed by downstream consumers
- Consumers in pricing and risk modules call the same solver entrypoint
- API documentation includes example usage for both consumers

### Unit tests
- Contract tests for API compatibility across consumer paths
- Integration tests for pricing-side solver calls and integration tests for risk-sampling-side solver calls

### Notes/Constraints
- Keep consumer adapters thin and conversion-focused
- Avoid consumer-specific forks of solver behavior
