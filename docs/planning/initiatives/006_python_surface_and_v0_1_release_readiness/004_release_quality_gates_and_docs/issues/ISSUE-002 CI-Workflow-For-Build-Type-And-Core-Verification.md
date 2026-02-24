## ISSUE-002: CI Workflow For Build Type And Core Verification

### Description
Implement CI workflow jobs (GitHub Actions workflows) for Rust build verification, Python importability checks, and core repository verification commands.

### Acceptance Criteria
- CI runs build and verification commands required by v0.1 gate policy
- Workflow fails fast on required-check failures with actionable logs
- Job outputs expose clear status per gate category
- Workflow configuration is documented for contributors

### Unit tests
- CI config validation checks for required job presence and smoke checks for expected command invocation paths
- Failure classification and status output consistency

### Notes/Constraints
- Keep workflows deterministic and portable across runners
- Reuse existing project verification commands where possible
