## Project 001: Repository Documentation Cleanup Program

Bring the entire repository Markdown surface up to production-grade quality for professional market users, open-source adopters, and maintainers. This project covers all tracked `.md` files, not only `docs/`, and treats documentation as a governed product surface with explicit audience boundaries, canonical ownership, executable examples, and release-quality controls.

### Context and requirements
- Classify every tracked Markdown file by audience and purpose so the repository no longer mixes public docs, contributor guidance, internal design records, planning records, generated READMEs, and archive material into one implied doc set.
- Establish a canonical information architecture for published docs, root entrypoints, contributor docs, internal design material, and planning records.
- Rewrite the top-level trust surface so first-contact pages such as `README.md`, release notes, and changelog entrypoints reflect a professional open-source repository rather than an internal workbench.
- Prioritize the market-user path from installation through successful market-data calibration, simulation, pricing, and interpretation before exposing advanced contract or implementation detail.
- Normalize local READMEs and other repo-adjacent Markdown so crate READMEs, fixture READMEs, and planning records each follow templates appropriate to their role.
- Define documentation style, review, verification, ownership, and release-governance rules strong enough for a financially literate external audience.

### Definition of Done
- The repository has a documented documentation policy that classifies all Markdown content and defines which content is canonical for each audience.
- An explicit migration ledger exists for every tracked `.md` file with disposition states such as keep, rewrite, move, generate, archive, or delete.
- Public entrypoints and published docs provide a coherent onboarding path with clear boundaries between user docs, contributor docs, internal design records, and planning records.
- Public examples are executable, linked content is validated, empty or trust-eroding placeholder docs are removed or replaced, and documentation quality checks are enforced in CI.
- Documentation ownership and release gates are documented so the docs remain professional after the initial rewrite instead of drifting back into ad hoc sprawl.

### Agent implementation notes
- Treat repository Markdown as a portfolio of distinct surfaces, not as one flat documentation tree.
- Prefer moving or archiving low-value historical material over polishing it into first-class docs.
- Keep public docs concise, authoritative, and task-oriented, with explicit assumptions, limitations, and data/provenance language where relevant.
- Preserve planning documents as planning documents. Do not rewrite planning artifacts into user docs verbatim.
- Keep generated READMEs clearly marked as generated and ensure their source-of-truth workflow is documented.

### Initiative alignment
- Delivers Initiative 012 by turning documentation from an ad hoc byproduct into a maintained professional surface.
- Supports a user experience that is credible to market practitioners, open-source adopters, and internal maintainers.
