## Interval Parametric Pricing With Hourly Default

### Description
Ensure pricing flow is interval-parametric in `Delta` while keeping hourly boundaries as the v0.1 intraday default configuration for option workflows.

### Acceptance Criteria
- Pricing API accepts explicit interval length `Delta` and does not hardcode hourly steps
- Default pricing configuration uses hourly boundaries for intraday workflows
- Interval handling remains compatible with solver and factor-provider interfaces
- Documentation clarifies hourly default versus configurable interval capability

### Unit tests
- Hourly default behavior in pricing requests and non-hourly interval requests when compatible inputs are provided
- Integration tests confirming interval value propagation through pricing pipeline

### Notes/Constraints
- Keep interval semantics consistent with oracle solver contracts
- Reject unsupported interval inputs with clear validation messages
