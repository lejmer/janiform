## Documentation Information Architecture And Surface Separation

### Description
Define the target information architecture for the repository's documentation surfaces and the boundaries between them. The resulting structure should clearly separate public product docs, contributor docs, internal design records, and planning records, and it should organize public docs around onboarding, concepts, workflows, and reference rather than current folder history.

### Acceptance Criteria
- A documented information architecture exists for the public docs surface with stable top-level sections for getting started, concepts, workflows, reference, and contributing.
- `docs/` is designated as the canonical user-facing documentation tree.
- Publishing boundaries are explicit so brainstorming notes, prompt templates, planning records, and milestone-specific status notes are not treated as first-class public docs.
- Root entrypoints and contributor docs have defined roles that complement the published docs instead of duplicating them.
- The architecture documents how deep internal contract and design material is linked, summarized, or excluded from public docs.
- The architecture includes a clear market-data-first path before advanced staged calibration and internal contract material.
- The proposed structure names the canonical home for installation, quickstart, model overview, workflow docs, API or CLI reference, and contributor guidance.
- Contributor docs, internal design material, and planning records each have a clear home and are not mixed into user onboarding flows.
- Historical or superseded design material has an explicit status treatment rather than remaining ambiguously current.
- The canonical surface split is explicit:
  `docs/` for public docs,
  repository root for contributor governance such as `CONTRIBUTING.md`, `documentation_policy.md`, and `documentation_style_guide.md`,
  `design/` for internal design records,
  and `planning/` for initiatives, projects, and issue tracking.
- The public docs information architecture chooses concrete homes under `docs/`: `docs/getting-started/`, `docs/concepts/`, `docs/workflows/`, and `docs/reference/`.

### Unit tests
- Docs build, navigation, or site-map validation checks that all primary sections exist.
- Published-docs validation ensures planning-only and contributor-only material is not included in the public surface by accident.

### Notes/Constraints
- Favor stable URLs and durable section names.
- Avoid mirroring the current repository tree when it produces poor user navigation.
- Preserve useful history, but do not let historical material masquerade as current guidance.
- If internal design records remain temporarily under `docs/design/` during migration, that state should be treated as transitional and not as the target architecture.
