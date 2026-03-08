## ISSUE-004: Docs CI Versioning And Zero-Budget Publishing Path

### Description
Finish the Docusaurus migration by adding CI validation, release-versioning policy, and a no-budget publishing path. The goal is for Janiform docs to be reviewable before public hosting is enabled and immediately publishable once the repository is ready.

### Acceptance Criteria
- CI builds the docs site on every relevant change and fails on broken builds or broken internal links.
- The project has a documented docs versioning policy aligned with Janiform releases, including how pre-v0.1 and post-v0.1 docs are handled.
- A default static hosting target is documented, with GitHub Pages treated as the baseline zero-budget option unless replaced later.
- The repository includes the scripts or workflow definitions needed to produce a deployable static site artifact.
- Release-readiness documentation includes docs build verification and publishing readiness as explicit checklist items.
- Contributor and maintainer instructions explain the docs CI workflow, release-versioning steps, and the path from local validation to published static artifacts.

### Unit tests
- CI includes a docs build job and a deploy-preview or artifact-generation path that proves the site can be published.
- Versioned-docs workflow checks cover at least one representative release or snapshot flow.

### Notes/Constraints
- Publishing enablement should not block local and CI adoption of Docusaurus.
- Keep vendor lock-in low by relying on Docusaurus static output rather than host-specific features.
- Prefer one default publishing path over multiple half-supported options.
