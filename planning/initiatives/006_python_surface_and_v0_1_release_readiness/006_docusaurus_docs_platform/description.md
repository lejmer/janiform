## Project 006: Docusaurus Docs Platform

Move Janiform's user-facing documentation from repository-browsed Markdown into a versioned Docusaurus site that is production-ready in structure, reviewable in Git, and cheap to operate.

### Context and requirements
- Establish Docusaurus as the canonical publishing layer for user-facing and contributor-facing documentation.
- Keep documentation source in version control, but stop treating raw repository Markdown as the end-user docs experience.
- Define a clear split between public docs content, contributor docs, and internal planning records now living under `planning/`.
- Build a migration path that starts with local and CI builds, and can later publish to GitHub Pages or another static host without restructuring the docs source again.
- Keep the initial implementation pragmatic: small enough to land incrementally, but opinionated enough to avoid another round of ad hoc docs sprawl.

### Definition of Done
- A Docusaurus site exists in-repo with a stable navigation structure, local development workflow, and CI build validation.
- The public docs information architecture is implemented around onboarding, task-oriented guides, reference, and conceptual explanations rather than internal planning or brainstorming artifacts.
- Existing user-facing content has an explicit migration plan: rewrite, merge, move to contributor docs, or archive.
- API and CLI reference strategy is defined so Janiform does not drift between code, examples, and published reference pages.
- The site is ready for zero-budget hosting on a static provider, with GitHub Pages treated as the default first target unless a better host is chosen later.

### Agent implementation notes
- Prefer a small number of substantial docs pages over a large number of thin placeholders.
- Keep examples executable and aligned to the supported Python and CLI surface.
- Do not mirror the old directory layout inside Docusaurus just because it already exists on disk.
- Treat planning docs, agent prompts, and brainstorming notes as source material, not as pages to publish verbatim.

### Initiative alignment
- Strengthens Initiative 006 by turning release-readiness documentation from a milestone checklist into a maintainable public docs platform.
- Reduces the risk that Janiform reaches v0.1 with strong contracts and workflows but an unconvincing open-source documentation experience.
