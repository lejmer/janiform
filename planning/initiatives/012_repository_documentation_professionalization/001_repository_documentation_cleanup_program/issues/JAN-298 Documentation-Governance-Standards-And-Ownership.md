## Documentation Governance, Standards, And Ownership

### Description
Define the operating rules for Janiform documentation so the cleanup produces a maintainable system rather than a one-off rewrite. This issue combines the repository documentation policy, audience taxonomy, canonical-source rules, writing standards, review checklist, ownership model, archive or supersession handling, and release-governance expectations.

### Acceptance Criteria
- A versioned documentation policy defines the primary documentation surfaces and their intended audiences, including `public`, `contributor`, `internal design`, `planning record`, `generated`, and `archive`.
- The canonical documentation policy lives at the repository root as `documentation_policy.md`, not under `docs/`, so user-facing docs and contributor governance remain separate.
- The policy explains where root docs, published docs, local READMEs, planning artifacts, and generated Markdown belong, and it defines canonical-source rules for content that might otherwise drift across multiple locations.
- A docs style guide defines tone, structure, terminology, status labeling, and expectations for assumptions, limitations, determinism or reproducibility notes, and provenance language where relevant.
- The canonical docs style guide lives at the repository root as `documentation_style_guide.md`, and contributor onboarding links to both governance files from a root-level `CONTRIBUTING.md`.
- A practical review checklist exists for pull requests that add or change documentation.
- Documentation ownership or stewardship expectations are defined for the main docs areas, along with archive and supersession rules.
- Contributor and release-readiness guidance explains when docs updates are required and which docs checks are part of normal release quality.

### Unit tests
- Repository checks validate that the governance artifacts exist and are linked from contributor-facing entrypoints.

### Notes/Constraints
- Keep the policy and style guide short enough that contributors will actually use them.
- Optimize for sustained maintenance, not ceremonial process.
- Treat repository-root governance files as contributor-facing rules, while `docs/` remains the canonical user-facing documentation surface.
