## Split Calibration Tests Into Submodules

### Description
Refactor `crates/janiform/src/calibration/tests.rs` into `crates/janiform/src/calibration/tests/` with `mod.rs` and split test helpers while preserving module visibility for test-only consumers.

### Acceptance Criteria
- Create `crates/janiform/src/calibration/tests/` and migrate test definitions from the single file into organized test submodules.
- Preserve all current test references under `#[cfg(test)]` and avoid exposing non-test public APIs.
- Keep `mod.rs` as just the module entrypoint.
- Keep `mod tests;` wiring in `crates/janiform/src/calibration/mod.rs` with no path-level changes required.
- Confirm all test suites compile and pass.

### Unit tests
- Use existing test harness validation by running `cargo test` and target-specific suites.

### Notes/Constraints
- This issue is internal to test organization and should not affect public crate API.
