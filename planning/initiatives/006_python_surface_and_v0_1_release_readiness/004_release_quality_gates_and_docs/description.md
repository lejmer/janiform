## Project 004: Release Quality Gates And Docs

Establish CI quality gates, v0.1 docs completeness, and objective release signoff criteria.

### Context and requirements
- Establish CI quality gates for build, type checks, core verification commands, and packaging smoke tests.
- Produce v0.1 documentation for installation, quickstart, model overview, and workflow examples.
- Define release checklist and readiness criteria.
- Treat market-data-first calibration as a primary release workflow, not an optional appendix.

### Definition of Done
- CI workflow enforces required checks for merge and release branch validation.
- Documentation set covers onboarding and day-one workflows for target users, including calibration from `csv` or `parquet` market data with session and known-news inputs.
- Release checklist is documented with owners and objective exit criteria.

### Agent implementation notes
- Keep docs examples executable against current repository commands.
- Store release criteria in versioned docs to support future iteration.
- Pre-v0.1 policy applies to contracts and schemas. Breaking changes are allowed when they improve correctness and simplify usage.
- Make sure examples, smoke checks, and release evidence reflect the user-facing calibration path instead of only internal execution-input examples.

### Initiative alignment
- Delivers Initiative 006 success signals for CI readiness and documentation completeness.
- Provides a concrete path from development to a consumable v0.1 release that matches market-user expectations.
