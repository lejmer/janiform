## ISSUE-002: CI Workflow For Build Type And Core Verification

### Description
Implement CI workflow jobs (GitHub Actions workflows) for Rust build verification, Python importability checks, and core repository verification commands.

### Acceptance Criteria
- CI runs build and verification commands required by v0.1 gate policy
- Workflow fails fast on required-check failures with actionable logs
- Job outputs expose clear status per gate category
- Workflow configuration is documented for contributors
- CI plan includes a bounded workflow that validates at least one file-driven calibration path with fixture data and clear failure diagnostics

### Unit tests
- CI config validation checks for required job presence and smoke checks for expected command invocation paths
- Failure classification and status output consistency
- Coverage that workflow metadata and logs identify which user-facing calibration scenario failed when the file-driven check breaks

### Notes/Constraints
- Keep workflows deterministic and portable across runners
- Reuse existing project verification commands where possible
- Pre-v0.1 policy applies: breaking contract and schema changes are allowed when they improve correctness and clarity.
