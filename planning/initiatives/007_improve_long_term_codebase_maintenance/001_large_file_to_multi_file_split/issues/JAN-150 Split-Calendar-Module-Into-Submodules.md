## Split Calendar Module Into Submodules

### Description
Refactor `crates/janiform/src/calendar.rs` into a folder-based module with `crates/janiform/src/calendar/mod.rs`, keeping external import paths stable.

### Acceptance Criteria
- Move responsibilities from the top-level `calendar.rs` into `crates/janiform/src/calendar/` submodules.
- Preserve all public `janiform::calendar` API paths and behavior.
- Keep `mod.rs` as just the module entrypoint.
- Keep `pub mod calendar;` unchanged in `crates/janiform/src/lib.rs`.
- Ensure callers do not need to update imports.

### Unit tests
- Re-run any calendar-focused unit and regression tests.

### Notes/Constraints
- Maintain current calendar construction semantics and data handling behavior.
