## ISSUE-001: Calendar Domain Model And Exchange Metadata

### Description
Define the Rust core domain model for exchange calendars used by Janiform. The model must represent trading sessions, timezone context, regular session windows, and metadata required for rule resolution.

### Acceptance Criteria
- A domain model exists in `crates/core` for exchange calendar definitions and session windows.
- The model supports immutable construction from versioned data artifacts.
- Invalid or incomplete exchange definitions fail validation with actionable errors.
- The model is documented with examples of supported session concepts.

### Unit tests
- Validation tests for missing timezone, invalid session windows, and overlapping regular sessions.
- Positive tests for a valid exchange definition with multiple weekdays.

### Notes/Constraints
- Keep this model independent of simulation and pricing modules.
- Do not embed exchange-specific hardcoded behavior in model constructors.
