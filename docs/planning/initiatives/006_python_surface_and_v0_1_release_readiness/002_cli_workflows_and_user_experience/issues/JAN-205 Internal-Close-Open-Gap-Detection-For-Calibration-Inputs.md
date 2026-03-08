## Internal Close Open Gap Detection For Calibration Inputs

### Description
Refactor the staged calibration CLI and Python boundary so users provide raw session-boundary market data instead of precomputed `detected_close_open_gap_candidates`, and Janiform derives close-open gap candidates internally.

Today the return-components calibration path forces callers to detect gap events before invoking Janiform and to format those detections into a contract-specific payload. That makes a simple, mechanical preprocessing step the user's problem and weakens the usability of the CLI and Python API. The calibration workflow should instead accept raw observed close and open prices with calendar context, classify close-open boundaries internally, and generate the gap candidates needed by return-components fitting.

### Acceptance Criteria
- Staged calibration contracts no longer require callers to provide `detected_close_open_gap_candidates` directly.
- Calibration CLI and Python API accept a raw boundary-observation input that contains the information needed to derive close-open gaps internally.
- Internal preprocessing derives close-open gap candidates deterministically from supplied observations and calendar context.
- Detection rules account for session boundaries and finite-value validation and define an explicit tolerance policy for deciding whether a boundary implies a material gap.
- The generated gap candidates continue to feed the existing return-components candidate-generation and fitting flow.
- In-repo examples, tests, and docs are migrated to the new contract in the same change.

### Unit tests
- Contract validation tests for the new raw boundary-observation input shape and required fields.
- Calibration integration tests showing that staged calibration succeeds without caller-supplied `detected_close_open_gap_candidates`.
- Deterministic regression tests for internal close-open gap detection across zero-gap, overnight-gap, and weekend-gap cases.
- CLI tests covering argument-to-contract mapping for the new input path and rejection of malformed observations.

### Notes/Constraints
- Pre-v0.1 policy applies: replace the old contract directly rather than adding dual support for both raw observations and precomputed gap candidates.
- Keep the detection path explicit and replayable so artifact provenance and diagnostics can report how gap candidates were derived.
- Reuse existing calendar services and return-components candidate-generation code instead of duplicating gap-routing logic.
- The issue is about moving gap detection to the Janiform boundary, not about changing the downstream calibration objective semantics.
