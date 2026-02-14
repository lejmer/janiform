## ISSUE-005: Shared Hourly Factor Provider API Integration

### Description
Expose a shared API that provides hourly factors to pricing and simulation modules without duplicating materialization logic.

### Acceptance Criteria
- A single provider API returns interval-aligned `s_h` and `nu_h` outputs for requested ranges.
- Pricing and simulation modules consume this shared provider instead of project-local adapters.
- Provider API returns structured errors for missing inputs and unsupported ranges.
- Integration documentation maps provider usage for each downstream engine.

### Unit tests
- Integration tests confirming pricing path consumption of provider outputs.
- Integration tests confirming simulation path consumption of provider outputs.
- Error-path tests for missing factor sources and unsupported range requests.

### Notes/Constraints
- Keep provider API stable for calibration and risk workflows.
- Avoid adding engine-specific logic to the provider layer.
