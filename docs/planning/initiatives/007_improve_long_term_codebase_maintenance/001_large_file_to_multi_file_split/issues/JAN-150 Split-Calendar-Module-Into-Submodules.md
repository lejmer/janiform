## Split Calendar Module Into Submodules

### Description
Refactor `crates/core/src/calendar.rs` into a folder-based module with `crates/core/src/calendar/mod.rs`, keeping external import paths stable.

### Acceptance Criteria
- Move responsibilities from the top-level `calendar.rs` into `crates/core/src/calendar/` submodules.
- Preserve all public `janiform::calendar` API paths and behavior.
- Keep `pub mod calendar;` unchanged in `crates/core/src/lib.rs`.
- Ensure callers do not need to update imports.

### Unit tests
- Re-run any calendar-focused unit and regression tests.

### Notes/Constraints
- Maintain current calendar construction semantics and data handling behavior.
