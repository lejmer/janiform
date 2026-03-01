## Split Risk Module Into Submodules

### Description
Refactor `crates/core/src/risk.rs` into `crates/core/src/risk/mod.rs` plus focused submodules, keeping all public contract entry points unchanged.

### Acceptance Criteria
- Create folder `crates/core/src/risk/` and replace the single top-level `risk.rs` body with a module-oriented split.
- Preserve all external import paths from `janiform::risk` and all re-exported symbols in `janiform::risk::*`.
- Keep the behavior and public docs for each symbol unchanged while improving internal organization.
- Keep `mod.rs` as just the module entrypoint.
- Ensure `crates/core/src/lib.rs` remains using `pub mod risk;` with no external path changes.
- Validate compile and test status before considering the issue done.

### Unit tests
- Do compile checks and run tests to confirm stable import paths for key `risk` symbols.

### Notes/Constraints
- Do not expose a compatibility shim.
- Keep module-private helpers in `crates/core/src/risk/` private unless needed by submodules.
