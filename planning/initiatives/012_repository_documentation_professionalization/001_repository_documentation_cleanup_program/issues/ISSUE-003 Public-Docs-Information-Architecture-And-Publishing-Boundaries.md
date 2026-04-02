## ISSUE-003: Public Docs Information Architecture And Publishing Boundaries

### Description
Define the target information architecture for the repository's published and non-published documentation surfaces. The resulting structure should clearly separate public product docs, contributor docs, internal design records, and planning records, and it should organize public docs around onboarding, concepts, workflows, and reference rather than current folder history.

### Acceptance Criteria
- A documented information architecture exists for the public docs surface with stable top-level sections for getting started, concepts, workflows, reference, and contributing.
- Publishing boundaries are explicit so brainstorming notes, prompt templates, planning records, and milestone-specific status notes are not treated as first-class public docs.
- Root entrypoints and contributor docs have defined roles that complement the published docs instead of duplicating them.
- The architecture documents how deep internal contract and design material is linked, summarized, or excluded from public docs.
- The architecture includes a clear market-data-first path before advanced staged calibration and internal contract material.

### Unit tests
- Docs build or navigation validation checks that all primary sections exist.
- Link or sidebar validation ensures the architecture resolves to real files and does not reference unpublished material accidentally.

### Notes/Constraints
- Favor stable URLs and durable section names.
- Avoid mirroring the current repository tree when it produces poor user navigation.
