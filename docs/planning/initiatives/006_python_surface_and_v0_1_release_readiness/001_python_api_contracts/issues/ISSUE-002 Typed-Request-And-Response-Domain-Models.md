## ISSUE-002: Typed Request And Response Domain Models

### Description
Implement explicit typed request and response models for core workflows so Python users interact with stable domain objects instead of loosely typed payloads.

### Acceptance Criteria
- Typed models exist for simulation, calibration, and pricing request and response contracts.
- Calibration request models include `execution_mode` with enum values `memory` and `disk`.
- Calibration request default is documented as `execution_mode=memory` for importable API usage.
- Models include validation rules and field-level documentation.
- Serialization behavior is deterministic for logging and persisted artifact use cases.
- Model naming remains consistent across workflows.

### Unit tests
- Valid and invalid model construction and field-level validation errors and messages.
- Deterministic serialization and deserialization.
- Defaulting and override tests for `execution_mode` in calibration request models.

### Notes/Constraints
- Reuse shared types for repeated concepts across workflows.
- Favor explicit classes and enums over unstructured dictionaries.
