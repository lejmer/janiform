## ISSUE-001: Quality Gate Matrix And Required Check Policy

### Description
Define the v0.1 quality-gate matrix and required-check policy for merge and release validation.

### Acceptance Criteria
- Quality-gate matrix lists required checks for build, typing, verification, and packaging smoke coverage
- Policy distinguishes merge-time checks from release-branch checks
- Required-check ownership and escalation paths are documented
- Matrix is versioned and linked from release docs

### Unit tests
- Validation checks for completeness of required check IDs
- Policy-schema checks for required gate fields

### Notes/Constraints
- Keep gate policy objective and automation-friendly
- Avoid ambiguous gate definitions that depend on manual interpretation
- Pre-v0.1 policy applies: breaking contract and schema changes are allowed when they improve correctness and clarity.
