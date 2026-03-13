## Docs Examples And Contributor Guidance For Python Package Decomposition

### Description
Update Python docs, examples, and contributor guidance so the new package structure is explained clearly and contributors can discover both convenience imports and explicit domain submodules without reading package internals.

### Implementation details
- Rewrite Python namespace and package-structure docs to explain the new decomposition approach and why it exists.
- Update examples to use whichever import style is clearest for the scenario:
  - curated package-root imports for common onboarding paths
  - explicit submodule imports for domain-focused or contributor-oriented examples
- Update contributor docs so future large-file splits follow the new public-submodule and true-internal-helper rules.
- Document the intentional import style for calibration, simulation, and CLI command-group code after the refactor.
- Ensure any release-readiness docs that discuss public API layout reference the updated package structure and no longer repeat the old facade-only story.

### Acceptance Criteria
- Package docs and contributor docs reflect the new package organization and import guidance.
- Examples and snippets no longer rely on outdated underscore-module names.
- Docs explain when root imports are a convenience and when explicit submodule imports are clearer.
- Planning for v0.1 release readiness treats package discoverability and maintainable decomposition as part of overall Python package quality.

### Unit tests
- Documentation smoke checks for updated import examples.
- Validation checks ensuring docs do not recommend superseded underscore module paths for calibration, simulation, or CLI core structure.

### Notes/Constraints
- Keep the guidance practical and short. The audience is primarily repo contributors working in a pre-release codebase.
- Coordinate language with Project 004 and Project 008 so public docs present one coherent Python story.
