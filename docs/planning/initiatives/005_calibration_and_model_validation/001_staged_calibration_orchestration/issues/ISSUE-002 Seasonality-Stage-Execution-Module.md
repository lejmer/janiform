## ISSUE-002: Seasonality Stage Execution Module

### Description
Implement the seasonality stage execution module that runs deterministic calendar-factor calibration and exports normalized outputs for downstream stages.

### Acceptance Criteria
- Stage runner consumes run spec inputs and executes seasonality estimation workflow
- Output includes normalized seasonality artifacts and metadata needed by later stages
- Stage produces structured success and failure status
- Stage is callable independently for partial-pipeline runs

### Unit tests
- Successful seasonality-stage execution on reference fixtures
- Missing-input and invalid-config failure paths
- Deterministic output metadata under fixed seed and config

### Notes/Constraints
- Reuse seasonality pipeline interfaces from Initiative 001
- Keep stage output schema stable for stage handoffs
