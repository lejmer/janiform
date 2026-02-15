## ISSUE-008: Round Trip Integrity And Corruption Regression Suite

### Description
Create regression suite that verifies artifact round-trip integrity and robust handling of corrupted or incomplete artifact sets.

### Acceptance Criteria
- Suite validates write and read round-trip fidelity across all artifact categories.
- Corruption scenarios include missing files, malformed payloads, and incompatible versions.
- Regression outputs include actionable diagnostics with run and artifact context.
- Fixtures and expected outcomes are versioned.

### Unit tests
- Round-trip integrity tests for full staged calibration outputs.
- Corruption-case tests for each artifact category.
- Regression tests for stable diagnostic reporting on failures.

### Notes/Constraints
- Keep fixture set bounded for fast regression execution.
- Separate corruption robustness tests from performance benchmarking jobs.
