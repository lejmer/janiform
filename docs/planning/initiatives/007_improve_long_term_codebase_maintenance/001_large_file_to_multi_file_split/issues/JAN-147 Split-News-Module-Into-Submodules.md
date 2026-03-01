## Split News Module Into Submodules

### Description
Refactor `crates/core/src/news.rs` into `crates/core/src/news/mod.rs` with smaller submodules and preserve existing public paths.

### Acceptance Criteria
- Create a `crates/core/src/news/` module directory and split `news.rs` into logically grouped files.
- Preserve current path stability for all exported news-related APIs.
- Maintain `pub mod news;` in `lib.rs` with no caller-facing path changes.
- Validate no behavior regressions in deterministic news and coupled behavior tests.

### Unit tests
- Re-run news and coupling integration tests in the affected areas.

### Notes/Constraints
- Keep news event semantics stable while reorganizing internal layout.
