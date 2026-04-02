## Top-Level Trust Surface Rewrite

### Description
Rewrite the first-contact repository pages so they present Janiform as a professional, externally readable project. This includes `README.md` and any top-level release or changelog entrypoints that a user, reviewer, or due-diligence reader is likely to open first.

### Acceptance Criteria
- `README.md` becomes a concise project landing page with project overview, supported workflows, installation path, quickstart, docs entrypoints, and contribution entrypoints.
- `CONTRIBUTING.md` is established as the root contributor entrypoint, and root governance files link from it instead of being embedded in `README.md`.
- Empty or placeholder top-level docs such as release or changelog entrypoints are either populated with production-ready content or removed from the visible surface.
- The top-level docs state the current project status clearly without milestone-noise or internal-only language.
- The repository front page no longer mixes onboarding material with giant command inventories, planning language, or internal implementation sprawl.
- Root-level release-facing files have an explicit disposition: `RELEASE.md` and `changelogs/v0.x.md` are either rewritten as real release-process surfaces or removed until the repository is ready to support them.

### Unit tests
- Link checks validate all top-level links.
- Smoke checks validate any commands that the top-level quickstart claims are supported.

### Notes/Constraints
- Keep the root surface intentionally smaller than the full published docs and use it as a gateway rather than as the entire manual.
- Optimize for trust, clarity, and fast orientation.
- Do not turn the repository root into a second documentation tree. The root should contain entrypoints and governance, while substantive user guidance lives under `docs/`.
